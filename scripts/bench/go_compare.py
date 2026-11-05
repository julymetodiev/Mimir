#!/usr/bin/env python3
"""GO2: against `gopls` over all sites of a Go project, does every disagreement with the index fall in a
named class of section 7 of docs/GO.md?

A site is a name before `(`, or after a `.`, found by a regular expression outside comments and strings.
`gopls` is asked for its definition, with one file open at a time, and files with `no package metadata`
once more at the end, as section 4 says. Whatever fits no class is printed, and the exit code is 1.

Usage: go_compare.py ROOT INDEX.scip [--limit N] [--out OUT.json]

ROOT is a directory with a go.mod, on a throwaway copy that is a git repository. INDEX is its index, the
largest `.scip` in `.latt/graph` for a project of one module.
"""
import argparse, json, os, re, subprocess, time
from collections import Counter, defaultdict
from urllib.parse import unquote

from scip_compare import Server, fields, packed

MASK = re.compile(r'//[^\n]*|/\*.*?\*/|"(?:\\.|[^"\\\n])*"|`[^`]*`|\'(?:\\.|[^\'\\\n])+\'', re.S)
SITE = re.compile(r"[A-Za-z_]\w*(?=\s*\()|(?<=\.)[A-Za-z_]\w*")
KEYWORDS = set("break case chan const continue default defer else fallthrough for func go goto if import interface "
               "map package range return select struct switch type var".split())
NO_METADATA = "no package metadata"


def ranged(occurrence):
    """Line and column of an occurrence, a typed range going first, and its symbol and roles."""
    span, typed, symbol, roles = [], None, "", 0
    for n, v in fields(occurrence):
        if n == 1:
            span = packed(v)
        elif n == 2:
            symbol = v.decode()
        elif n == 3:
            roles = v
        elif n in (8, 9):
            numbers = dict(fields(v))
            typed = (numbers.get(1, 0), numbers.get(2, 0))
    return (typed or (span[0], span[1])), symbol, roles


def read_index(path):
    defined, sites, documents = defaultdict(set), {}, set()
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
        documents.add(name)
        for occurrence in occurrences:
            (line, column), symbol, roles = ranged(occurrence)
            if not symbol:
                continue
            is_definition = bool(roles & 1)
            if is_definition:
                defined[(name, symbol) if symbol.startswith("local ") else symbol].add((name, line))
            sites[(name, line, column)] = (symbol, is_definition)
    return defined, sites, documents


def sites_of(text):
    # A space for each byte, so a column stays what it is in the source.
    masked = MASK.sub(lambda m: re.sub(r"[^\n]", lambda c: " " * len(c.group().encode()), m.group()), text)
    found = []
    for number, line in enumerate(masked.split("\n")):
        for m in SITE.finditer(line):
            if m.group() not in KEYWORDS:
                found.append((number, len(line[:m.start()].encode()), m.group()))
    return found


def utf16(line, column):
    return len(line.encode()[:column].decode(errors="replace").encode("utf-16-le")) // 2


def go_files(root):
    listed = subprocess.run(["git", "ls-files", "--cached", "--others", "--exclude-standard", "*.go"],
                            cwd=root, capture_output=True, text=True, check=True).stdout.split("\n")

    def kept(path):
        parts = path.split("/")[:-1]
        return path and not any(p in ("vendor", "testdata") or p.startswith(("_", ".")) for p in parts)

    return sorted(p for p in listed if kept(p))


def plain_path(path):
    """A path in the module cache as its package reads: no version, and `!a` as `A`."""
    path = re.sub(r"@[^/]+", "", path)
    return re.sub(r"!([a-z])", lambda m: m.group(1).upper(), path)


def classify(key, name, answer, known, root, lines_of):
    """The class of section 7, or `unclassified: ...`. `answer` is a list of (path, line), or None on an error."""
    defined, sites, documents = known
    path = key[0]
    if path not in documents:
        if answer is None:
            return "gopls has an error, the file is outside the build"
        if not answer:
            return "gopls empty, outside the build"
        return "file not in the index, outside the build"
    if answer is None:
        return "unclassified: gopls has an error on a file in the index"
    found = sites.get(key)
    outside = [t for t in answer if t[0].startswith("/")]
    if found is None:
        # `func _()` has no symbol in an index.
        if answer == [(path, key[1])]:
            return "the definition itself"
        if any(os.path.basename(t[0]) in ("builtin.go", "unsafe.go") for t in outside):
            return "a built-in name or unsafe, which the index does not report"
        if outside:
            return "not in the index, target outside the project"
        return "unclassified: not in the index" if answer else "unclassified: not in the index, gopls empty"
    symbol, is_definition = found
    if is_definition:
        return "the definition itself"
    want = defined.get((path, symbol) if symbol.startswith("local ") else symbol, set())
    if want & set(answer):
        return "same place"
    if symbol.endswith("/`/") or symbol.endswith("/"):
        if any(t[0] == path and "\"" in lines_of(path)[t[1]] for t in answer):
            return "a package's name: the import line against the package"
    if outside:
        # `scip-go gomod example.com/a v1 `example.com/a/b`/F().`: the package is between the backticks,
        # or the word before the first `/` for one with no slash in its path, as `fmt`/ is not quoted.
        package = symbol.split("`")[1] if "`" in symbol else symbol.split(" ")[-1].split("/")[0]
        if any(f"/{package}/" in plain_path(t[0]) for t in outside):
            return "target outside the project, same package"
        if package == "builtin" or any(os.path.basename(t[0]) == "builtin.go" for t in outside):
            return "a built-in name or unsafe, which the index does not report"
    if not answer:
        return "unclassified: gopls empty"
    return "unclassified: another place"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root")
    parser.add_argument("index")
    parser.add_argument("--limit", type=int, default=0)
    parser.add_argument("--out", default="")
    args = parser.parse_args()
    root = os.path.realpath(args.root)
    known = read_index(args.index)
    files = go_files(root)
    if args.limit:
        files = files[:args.limit]
    text = {p: open(os.path.join(root, p), encoding="utf-8", errors="replace").read() for p in files}
    split = {p: t.split("\n") for p, t in text.items()}

    def lines_of(path):
        if path not in split:
            with open(os.path.join(root, path), encoding="utf-8", errors="replace") as f:
                split[path] = f.read().split("\n")
        return split[path]

    server = Server("gopls", root, (args.out or "/dev/null") + (".err" if args.out else ""))
    server.call("initialize", {"processId": os.getpid(), "rootUri": "file://" + root,
                               "initializationOptions": {"zeroConfig": False},
                               "capabilities": {"textDocument": {"definition": {"linkSupport": False}}},
                               "workspaceFolders": [{"uri": "file://" + root, "name": "project"}]})
    server.note("initialized", {})

    def ask(path):
        """The answers for the sites of a file; `None` for the whole file if it has no metadata."""
        uri = "file://" + os.path.join(root, path)
        server.note("textDocument/didOpen", {"textDocument": {"uri": uri, "languageId": "go", "version": 1,
                                                              "text": text[path]}})
        sites = sites_of(text[path])
        params = [{"textDocument": {"uri": uri},
                   "position": {"line": s[0], "character": utf16(split[path][s[0]], s[1])}} for s in sites]
        answers = server.many("textDocument/definition", params, lambda _: None)
        server.note("textDocument/didClose", {"textDocument": {"uri": uri}})
        if any(NO_METADATA in (a.get("error") or {}).get("message", "") for a in answers):
            return sites, None
        return sites, answers

    def places(answer):
        if "error" in answer:
            return None
        result = answer.get("result") or []
        out = []
        for location in result if isinstance(result, list) else [result]:
            where = unquote(location["uri"][7:])
            relative = os.path.relpath(where, root) if where.startswith(root + "/") else where
            out.append((relative, location["range"]["start"]["line"]))
        return out

    begun = time.time()
    counts, examples, later = Counter(), defaultdict(list), []

    def count(path, sites, answers):
        for (line, column, name), answer in zip(sites, answers):
            verdict = classify((path, line, column), name, places(answer), known, root, lines_of)
            counts[verdict] += 1
            if verdict.startswith("unclassified") and len(examples[verdict]) < 15:
                examples[verdict].append(f"{path}:{line + 1}:{column + 1} {name} "
                                         f"{known[1].get((path, line, column), ('', False))[0][-60:]} "
                                         f"{str(places(answer))[:100]}")

    for number, path in enumerate(files, 1):
        sites, answers = ask(path)
        if answers is None:
            later.append(path)
        else:
            count(path, sites, answers)
        if number % 100 == 0:
            print(f"{number} of {len(files)} files, {time.time() - begun:.0f} s", flush=True)
    if later:
        time.sleep(5)
    second_try_failed = []
    for path in later:
        sites, answers = ask(path)
        if answers is None:
            second_try_failed.append(path)
            sites = sites_of(text[path])
            answers = [{"error": {"message": NO_METADATA}}] * len(sites)
        count(path, sites, answers)
    server.process.kill()

    total = sum(counts.values())
    report = {"files": len(files), "sites": total, "seconds": round(time.time() - begun),
              "second_try": len(later), "second_try_failed": len(second_try_failed),
              "classes": dict(counts.most_common()), "examples": examples}
    print(json.dumps(report, indent=1))
    if args.out:
        with open(args.out, "w") as f:
            json.dump(report, f, indent=1)
    unclassified = sum(n for name, n in counts.items() if name.startswith("unclassified"))
    print(f"unclassified: {unclassified}")
    raise SystemExit(1 if unclassified else 0)


if __name__ == "__main__":
    main()
