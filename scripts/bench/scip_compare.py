#!/usr/bin/env python3
"""How does a running language server compare with a SCIP index, site by site?

A site is a place the index reports a reference. The server is asked for its definition, and the target is
compared by file and line, or by package and file when it lies outside the project. Every other
identifier is asked too, which shows what the index leaves out and how many calls get no answer.
Reads the project as text, runs none of it. See section 7 of docs/TYPESCRIPT.md and docs/PYTHON.md.

Usage: scip_compare.py PROJECT INDEX.scip --server "BIN ARGS" [--prefix DIR/] [--reverse] [--one-by-one] [--limit N] --out OUT.json
       scip_compare.py --same A.json B.json     (do two runs give the same answers?)

--prefix keeps other files as targets and asks about these only. --reverse opens files in the opposite
order. --one-by-one keeps one file open at a time. A target that differs from the index is asked again
from where it landed, until it stops moving (docs/TYPESCRIPT.md, section 4). Run it on a throwaway copy
that is a git repository.
"""
import argparse, json, os, re, subprocess, sys, threading, time
from collections import Counter, defaultdict

KEYWORDS = set(
    "if for while switch catch function return await typeof new super import else do case in of "
    "yield void delete throw constructor get set def class lambda not and or is elif with print".split()
)
TOKEN = re.compile(r"[A-Za-z_$][A-Za-z0-9_$]*")
COMMENT = re.compile(r"//.*?$|/\*.*?\*/", re.S | re.M)
PYTHON_COMMENT = re.compile(r"#.*?$", re.M)
LANGUAGES = {".ts": "typescript", ".mts": "typescript", ".cts": "typescript", ".tsx": "typescriptreact",
             ".js": "javascript", ".mjs": "javascript", ".cjs": "javascript", ".jsx": "javascriptreact",
             ".py": "python", ".pyi": "python"}


def varint(b, i):
    value = shift = 0
    while True:
        c = b[i]
        i += 1
        value |= (c & 127) << shift
        shift += 7
        if c < 128:
            return value, i


def fields(b):
    i = 0
    while i < len(b):
        key, i = varint(b, i)
        number, wire = key >> 3, key & 7
        if wire == 0:
            value, i = varint(b, i)
        elif wire == 2:
            length, i = varint(b, i)
            value, i = b[i:i + length], i + length
        elif wire == 1:
            value, i = b[i:i + 8], i + 8
        else:
            value, i = b[i:i + 4], i + 4
        yield number, value


def packed(b):
    out, i = [], 0
    while i < len(b):
        value, i = varint(b, i)
        out.append(value)
    return out


def read_index(path, prefix):
    """Definitions of every symbol, and the references under the prefix."""
    defs, refs, docs = defaultdict(set), {}, set()
    with open(path, "rb") as f:
        data = f.read()
    for number, document in fields(data):
        if number != 2:
            continue
        name, occurrences = None, []
        for n, v in fields(document):
            if n == 1:
                name = v.decode()
            elif n == 2:
                occurrences.append(v)
        docs.add(name)
        for occurrence in occurrences:
            span, symbol, roles = [], "", 0
            for n, v in fields(occurrence):
                if n == 1:
                    span = packed(v)
                elif n == 2:
                    symbol = v.decode()
                elif n == 3:
                    roles = v
            if not symbol or not span:
                continue
            if roles & 1:
                defs[(name, symbol) if symbol.startswith("local ") else symbol].add((name, span[0]))
            elif name.startswith(prefix):
                refs[(name, span[0], span[1])] = symbol
    return defs, refs, docs


class Server:
    def __init__(self, command, root, log):
        self.process = subprocess.Popen(command.split(), stdin=subprocess.PIPE,
                                        stdout=subprocess.PIPE, stderr=open(log, "wb"), cwd=root)
        self.next = 0
        self.lock = threading.Lock()
        self.waiting = {}
        threading.Thread(target=self.read, daemon=True).start()

    def read(self):
        out = self.process.stdout
        while True:
            length = None
            while True:
                header = out.readline()
                if not header:
                    return
                if header in (b"\r\n", b"\n"):
                    break
                if header.lower().startswith(b"content-length:"):
                    length = int(header.split(b":")[1])
            message = json.loads(out.read(length))
            if "id" in message and "method" in message:
                self.write({"jsonrpc": "2.0", "id": message["id"], "result": None})
            elif "id" in message:
                done, box = self.waiting.pop(message["id"])
                box.append(message)
                done.set()

    def write(self, message):
        body = json.dumps(message).encode()
        with self.lock:
            self.process.stdin.write(b"Content-Length: %d\r\n\r\n" % len(body) + body)
            self.process.stdin.flush()

    def send(self, method, params):
        self.next += 1
        done, box = threading.Event(), []
        self.waiting[self.next] = (done, box)
        self.write({"jsonrpc": "2.0", "id": self.next, "method": method, "params": params})
        return done, box

    def call(self, method, params):
        done, box = self.send(method, params)
        if not done.wait(600):
            raise TimeoutError(method)
        return box[0]

    def note(self, method, params):
        self.write({"jsonrpc": "2.0", "method": method, "params": params})

    def many(self, method, all_params, tick, window=32):
        out, pending = [None] * len(all_params), []

        def drain():
            for k, (done, box) in pending:
                done.wait(600)
                out[k] = box[0] if box else {}
            pending.clear()

        for k, params in enumerate(all_params):
            pending.append((k, self.send(method, params)))
            if len(pending) >= window:
                drain()
                tick(k + 1)
        drain()
        return out

    def memory_mb(self):
        r = subprocess.run(["ps", "-o", "rss=", "-p", str(self.process.pid)], capture_output=True, text=True)
        return int(r.stdout.strip() or 0) // 1024


def package_of(path):
    path = path.replace("%40", "@")
    folder = "node_modules/" if "node_modules/" in path else "site-packages/" if "site-packages/" in path else None
    if folder is None:
        return "?"
    tail = path.rsplit(folder, 1)[1].split("/")
    name = "/".join(tail[:2]) if tail[0].startswith("@") else tail[0]
    return "typescript" if name.startswith("@tsrs/") else name


def verdict_of(symbol, want, got, docs):
    inside = {t for t in got if t[0] in docs}
    if not want:
        if inside:
            return "index_none_server_inside"
        if not got:
            return "outside_server_nothing"
        parts = symbol.split(" ")
        if len(parts) <= 3 or parts[2] not in {package_of(t[0]) for t in got}:
            return "outside_other_package"
        same = any(os.path.basename(t[0]) in symbol for t in got)
        return "outside_same_file" if same else "outside_same_package"
    if want & got:
        return "agree"
    if not got:
        return "server_nothing"
    return "differ" if inside else "server_outside"


def module_named(path, name):
    """Whether this file is the module of that name, as `a/__init__.py` or `a.py`."""
    stem = os.path.splitext(os.path.basename(path))[0]
    return name == (os.path.basename(os.path.dirname(path)) if stem == "__init__" else stem)


def class_of(symbol, name, want_lines, got_line, got_name):
    """The named class of a site that still differs after asking again (docs/TYPESCRIPT.md, section 7).

    The lines are the source at the index's targets and at where the server ended. Whatever fits none stays
    unclassified, to be read: TS2 asks for 0 of those."""
    if any(word in text for word in ("<constructor>", "constructor", "__init__", "__new__")
           for text in [symbol, got_line, *want_lines]):
        return "constructor"
    if name in ("exports", "module"):
        return "CommonJS exports"
    # `models.TextChoices("Medal", "GOLD")` builds an enum: a server may answer with what runs.
    if got_name == "__call__":
        return "metaclass call"
    # `from .a import __all__ as a_all`.
    if got_name == "__all__":
        return "alias followed"
    # `var get = UTIF.decode._get`: the server goes on to the function, and the index stops at the variable.
    if got_name and got_name != name and any(
            re.search(r"(?<![\w$])" + re.escape(got_name) + r"(?![\w$])", w) for w in want_lines):
        return "alias followed"
    named = re.compile(r"(?<![\w$])" + re.escape(name) + r"(?![\w$])") if name else None
    if named and named.search(got_line):
        if any(named.search(w) for w in want_lines):
            return "same name, another declaration"
        return "server on the name, index elsewhere"
    return "unclassified"


def compare(args):
    root = os.path.realpath(args.project)
    defs, refs, docs = read_index(args.index, args.prefix)
    files = sorted(d for d in docs if d.startswith(args.prefix) and os.path.exists(os.path.join(root, d)))
    if args.limit:
        files = files[:args.limit]
    if args.reverse:
        files.reverse()
    chosen = set(files)
    text = {p: open(os.path.join(root, p), encoding="utf-8", errors="replace").read() for p in files}
    sites = sorted(k for k in refs if k[0] in chosen)

    tokens, calls = [], set()
    for p in files:
        comment = PYTHON_COMMENT if p.endswith((".py", ".pyi")) else COMMENT
        masked = comment.sub(lambda m: re.sub(r"[^\n]", " ", m.group()), text[p])
        for number, line in enumerate(masked.split("\n")):
            for m in TOKEN.finditer(line):
                key = (p, number, m.start())
                if key not in refs:
                    tokens.append(key)
                if m.group() not in KEYWORDS and line[m.end():].lstrip().startswith("("):
                    calls.add(key)

    def uri(p):
        return "file://" + os.path.join(root, p)

    def position(key):
        return {"textDocument": {"uri": uri(key[0])}, "position": {"line": key[1], "character": key[2]}}

    def places(answer):
        result = (answer or {}).get("result") or []
        out = []
        for location in result if isinstance(result, list) else [result]:
            path = (location.get("targetUri") or location["uri"])[7:]
            span = location.get("targetSelectionRange") or location["range"]
            out.append((os.path.relpath(path, root) if path.startswith(root + "/") else path,
                        span["start"]["line"], span["start"]["character"]))
        return out

    def targets(answer):
        return {(p[0], p[1]) for p in places(answer)}

    def open_file(p):
        server.note("textDocument/didOpen", {"textDocument": {
            "uri": uri(p), "languageId": LANGUAGES.get(os.path.splitext(p)[1], "typescript"),
            "version": 1, "text": text[p]}})

    def ask(keys):
        """The answers for places in the files, opening them as asked for."""
        if not args.one_by_one:
            return server.many("textDocument/definition", [position(k) for k in keys], tick)
        by_file, out = defaultdict(list), {}
        for k in keys:
            by_file[k[0]].append(k)
        for p in files:
            if by_file[p]:
                open_file(p)
                for k, a in zip(by_file[p], server.many("textDocument/definition",
                                                        [position(k) for k in by_file[p]], lambda _: None)):
                    out[k] = a
                server.note("textDocument/didClose", {"textDocument": {"uri": uri(p)}})
                tick(len(out))
        return [out[k] for k in keys]

    report = {"files": len(files), "sites": len(sites), "other_identifiers": len(tokens)}
    server = Server(args.server, root, args.out + ".err")
    server.call("initialize", {"processId": os.getpid(), "rootUri": "file://" + root,
                               "capabilities": {"textDocument": {"definition": {"linkSupport": True}}},
                               "workspaceFolders": [{"uri": "file://" + root, "name": "project"}]})
    server.note("initialized", {})
    start = time.time()
    for p in files[:1] if args.one_by_one else files:
        open_file(p)
    server.call("textDocument/definition", position((files[0], 0, 0)))
    report["first_answer_s"] = round(time.time() - start, 3)
    if args.one_by_one:
        server.note("textDocument/didClose", {"textDocument": {"uri": uri(files[0])}})
    peak = [server.memory_mb()]
    begun = time.time()

    def tick(k):
        peak[0] = max(peak[0], server.memory_mb())
        if k % 32000 == 0:
            print(f"{k} asked, {time.time() - begun:.0f} s, {peak[0]} MB", flush=True)

    answers = {}
    spots = {}
    for key, answer in zip(sites, ask(sites)):
        answers[key] = targets(answer)
        spots[key] = places(answer)
    report["sites_s"] = round(time.time() - begun, 2)
    report["per_second"] = round(len(sites) / max(report["sites_s"], 0.01))

    verdicts, examples = Counter(), defaultdict(list)
    for key in sites:
        symbol = refs[key]
        local = symbol.startswith("local ")
        kind = "local" if local else "function" if symbol.endswith(").") else "other"
        want = defs.get((key[0], symbol) if local else symbol, set())
        v = verdict_of(symbol, want, answers[key], docs)
        verdicts[f"{kind}/{v}"] += 1
        if v not in ("agree", "outside_same_file"):
            examples[v].append({"at": f"{key[0]}:{key[1] + 1}:{key[2] + 1}", "symbol": symbol[-70:],
                                "want": sorted(want)[:3], "got": sorted(answers[key])[:3]})
    report["verdicts"] = dict(sorted(verdicts.items()))

    # The rule of section 4: what still differs after asking again from where the answer landed.
    still, followed = Counter(), 0

    def source_line(path, line):
        whole = os.path.join(root, path) if not os.path.isabs(path) else path
        try:
            with open(whole, encoding="utf-8", errors="replace") as f:
                lines = f.read().split("\n")
        except OSError:
            return ""
        return lines[line] if line < len(lines) else ""

    for example in examples.get("differ", []):
        if example["symbol"].endswith("`/"):
            still["module position"] += 1
            continue
        p, line, column = example["at"].rsplit(":", 2)
        key = (p, int(line) - 1, int(column) - 1)
        want = defs.get((p, refs[key]) if refs[key].startswith("local ") else refs[key], set())
        here = spots[key][:1]
        for _ in range(8):
            if not here or here[0][0] not in chosen:
                break
            if not args.one_by_one:
                next_here = places(server.call("textDocument/definition", position(here[0])))
            else:
                open_file(here[0][0])
                next_here = places(server.call("textDocument/definition", position(here[0])))
                server.note("textDocument/didClose", {"textDocument": {"uri": uri(here[0][0])}})
            if not next_here or next_here[:1] == here:
                break
            here = next_here[:1]
            followed += 1
        hit = bool(here) and (here[0][0], here[0][1]) in want
        if hit:
            still["reached the index's target"] += 1
            continue
        site_line = text[p].split("\n")[key[1]]
        token = TOKEN.match(site_line, key[2])
        # Where it first answered: asking again from a field lands on `self`, which says nothing.
        first = spots[key][:1]
        if first and first[0][1] == 0 and token and module_named(first[0][0], token.group()):
            still["module position"] += 1
            continue
        got_line = source_line(first[0][0], first[0][1]) if first else ""
        want_lines = [source_line(w[0], w[1]) for w in sorted(want)[:3]]
        got_token = TOKEN.match(got_line, first[0][2]) if first else None
        named = class_of(refs[key], token.group() if token else "", want_lines, got_line,
                         got_token.group() if got_token else "")
        still[named] += 1
        example["after_asking_again"] = here
        example["class"] = named
    report["differ_after_asking_again"] = dict(still)
    report["steps_followed"] = followed

    begun = time.time()
    for key, answer in zip(tokens, ask(tokens)):
        answers[key] = targets(answer)
    report["other_identifiers_s"] = round(time.time() - begun, 2)
    answered = sum(1 for k in calls if answers.get(k))
    inside = sum(1 for k in calls if any(t[0] in docs for t in answers.get(k, ())))
    report["call_like"] = {"sites": len(calls), "answered": answered, "inside_project": inside,
                           "no_answer": len(calls) - answered}
    silent = [k for k in sorted(calls) if not answers.get(k)]
    report["no_answer_examples"] = [f"{p}:{l + 1}:{c + 1} {text[p].split(chr(10))[l].strip()[:70]}"
                                    for p, l, c in silent[:: max(1, len(silent) // 40)]]

    busiest = Counter(k[0] for k in sites).most_common(1)[0][0]
    begun = time.time()
    if args.one_by_one:
        open_file(busiest)
    server.note("textDocument/didChange", {"textDocument": {"uri": uri(busiest), "version": 2},
                                           "contentChanges": [{"text": text[busiest] + "\n// edited\n"}]})
    server.call("textDocument/definition", position((busiest, 0, 0)))
    report["edit_first_answer_s"] = round(time.time() - begun, 3)
    peak[0] = max(peak[0], server.memory_mb())
    report["memory_peak_sampled_mb"] = peak[0]
    server.process.kill()

    report["examples"] = examples
    with open(args.out, "w") as f:
        json.dump(report, f, indent=1, default=list)
    with open(args.out + ".answers", "w") as f:
        json.dump({f"{k[0]}:{k[1]}:{k[2]}": sorted(v) for k, v in answers.items()}, f, sort_keys=True)
    print(json.dumps({k: v for k, v in report.items() if "examples" not in k}, indent=1, default=list))


def same(a, b):
    with open(a) as f:
        first = json.load(f)
    with open(b) as f:
        second = json.load(f)
    differ = [k for k in first if k in second and first[k] != second[k]]
    print(f"{len(first)} and {len(second)} places, {len(differ)} differ")
    for k in differ[:20]:
        print(" ", k, first[k][:2], second[k][:2])


def main():
    if len(sys.argv) == 4 and sys.argv[1] == "--same":
        same(sys.argv[2], sys.argv[3])
        return
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("project")
    parser.add_argument("index")
    parser.add_argument("--server", required=True)
    parser.add_argument("--prefix", default="")
    parser.add_argument("--reverse", action="store_true")
    parser.add_argument("--one-by-one", action="store_true")
    parser.add_argument("--limit", type=int, default=0)
    parser.add_argument("--out", required=True)
    compare(parser.parse_args())


if __name__ == "__main__":
    main()
