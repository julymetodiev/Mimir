#!/usr/bin/env python3
"""Do the users `trace` shows for a type, a field or a constant match the language server's references?

Usage: uses_eval.py PROJECT GRAPH.json SAMPLE_PER_KIND [SEED]
The edges come from an index; these answers come from a running analyzer's find-references, another
path through it. Each answer is reduced to the innermost symbol holding it, leaving out imports and
`Self`, which the graph leaves out too. Prints precision and recall per kind, and what differs.
Nothing is written. PROJECT should be a throwaway copy, since rust-analyzer runs build scripts.
"""
import collections, json, pathlib, queue, random, re, subprocess, sys, threading, time

project = pathlib.Path(sys.argv[1]).resolve()
graph = json.load(open(sys.argv[2]))
per_kind = int(sys.argv[3])
random.seed(int(sys.argv[4]) if len(sys.argv) > 4 else 1)

paths = {v: k for k, v in graph["file_ids"].items()}
symbols = {s["id"]: s for s in graph["symbols"]}
in_file = collections.defaultdict(list)
for s in graph["symbols"]:
    in_file[s["file_id"]].append(s)
users = collections.defaultdict(set)
for e in graph["edges"]:
    if e[2] in ("UsesType", "UsesField", "UsesConstant", "UsesAsValue", "Calls"):
        users[e[1]].add(e[0])
sources = {}


def lines_of(rel):
    if rel not in sources:
        sources[rel] = (project / rel).read_text(errors="replace").split("\n")
    return sources[rel]


def innermost(file_id, line):
    """The smallest symbol holding a 1-based line and the column, as the graph reduces a use."""
    line1, col = line
    best = None
    for s in in_file[file_id]:
        r = s["range"]
        if (r["start_line"], r["start_col"]) <= (line1, col) <= (r["end_line"], r["end_col"]):
            size = s["span"]["end"] - s["span"]["start"]
            if best is None or size < best[0]:
                best = (size, s)
    return best[1] if best else None


def in_use_declaration(rel, line):
    """Whether a 0-based line is inside a `use` item, found by walking up to its start."""
    lines = lines_of(rel)
    for at in range(line, max(line - 40, -1), -1):
        text = lines[at].strip()
        if at != line and (not text or text.endswith((";", "}"))):
            return False
        if re.match(r"(pub(\([^)]*\))?\s+)?use\b", text):
            return True
        if at != line and text.endswith("{"):
            # Only a `use` list opens this way and goes on being a `use`.
            return False
    return False


process = subprocess.Popen(
    ["rust-analyzer"], cwd=project, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
    stderr=subprocess.DEVNULL,
)
answers, next_id = queue.Queue(), [0]
progress = {"done": False}


def reader():
    while True:
        length = 0
        while True:
            header = process.stdout.readline()
            if not header:
                return
            if header in (b"\r\n", b"\n"):
                break
            if header.lower().startswith(b"content-length:"):
                length = int(header.split(b":")[1])
        message = json.loads(process.stdout.read(length))
        if message.get("method") == "experimental/serverStatus":
            if message["params"].get("quiescent"):
                progress["done"] = True
            continue
        if "id" in message and "method" in message:
            send({"jsonrpc": "2.0", "id": message["id"], "result": None})
        elif "id" in message:
            answers.put(message)


def send(message):
    body = json.dumps(message).encode()
    process.stdin.write(b"Content-Length: %d\r\n\r\n%s" % (len(body), body))
    process.stdin.flush()


def request(method, params, timeout=300):
    next_id[0] += 1
    send({"jsonrpc": "2.0", "id": next_id[0], "method": method, "params": params})
    while True:
        message = answers.get(timeout=timeout)
        if message["id"] == next_id[0]:
            return message.get("result")


threading.Thread(target=reader, daemon=True).start()
request("initialize", {
    "processId": None, "rootUri": project.as_uri(),
    "capabilities": {"experimental": {"serverStatusNotification": True}},
    "initializationOptions": {"checkOnSave": False},
})
send({"jsonrpc": "2.0", "method": "initialized", "params": {}})
started = time.time()
while not progress["done"] and time.time() - started < 600:
    time.sleep(1)
print(f"analyzer ready in {time.time() - started:.0f} s", flush=True)

kinds = {"type": ("struct", "enum", "trait", "type_alias"), "variant": ("enum_variant",),
         "field": ("field",), "constant": ("constant",)}
for name, wanted in kinds.items():
    pool = [s for s in graph["symbols"] if s["kind"] in wanted and users[s["id"]]
            and (project / paths[s["file_id"]]).exists()]
    sample = random.sample(pool, min(per_kind, len(pool)))
    agreed = only_graph = only_server = 0
    shown = 0
    for s in sample:
        rel = paths[s["file_id"]]
        path = project / rel
        lines = lines_of(rel)
        at, column = None, None
        for number in range(s["range"]["start_line"] - 1, s["range"]["end_line"]):
            found = re.search(r"\b%s\b" % re.escape(s["name"]), lines[number])
            if found:
                at, column = number, found.start()
                break
        if at is None:
            continue
        send({"jsonrpc": "2.0", "method": "textDocument/didOpen", "params": {"textDocument": {
            "uri": path.as_uri(), "languageId": "rust", "version": 1, "text": path.read_text(errors="replace")}}})
        result = request("textDocument/references", {
            "textDocument": {"uri": path.as_uri()},
            "position": {"line": at, "character": column},
            "context": {"includeDeclaration": False},
        }) or []
        server = set()
        for location in result:
            try:
                other = pathlib.Path(location["uri"].removeprefix("file://")).resolve().relative_to(project)
            except ValueError:
                continue
            file_id = graph["file_ids"].get(str(other))
            if file_id is None:
                continue
            start = location["range"]["start"]
            text = lines_of(str(other))[start["line"]][start["character"]:location["range"]["end"]["character"]]
            if text == "Self" or in_use_declaration(str(other), start["line"]):
                continue
            holder = innermost(file_id, (start["line"] + 1, start["character"]))
            if holder and holder["id"] != s["id"]:
                server.add(holder["id"])
        ours = users[s["id"]]
        agreed += len(ours & server)
        only_graph += len(ours - server)
        only_server += len(server - ours)
        for label, ids in (("graph", ours - server), ("server", server - ours)):
            for i in sorted(ids)[:2]:
                if shown < 12:
                    u = symbols[i]
                    print(f"   only {label}: {s['name']} ({rel}:{s['range']['start_line']}) <- "
                          f"{u['name']} ({paths[u['file_id']]}:{u['range']['start_line']})", flush=True)
                    shown += 1
    total_graph, total_server = agreed + only_graph, agreed + only_server
    print(f"{name}: {len(sample)} sampled of {len(pool)}; agreed {agreed}, only graph {only_graph}, "
          f"only server {only_server}; precision {agreed / max(total_graph, 1):.1%}, "
          f"recall {agreed / max(total_server, 1):.1%}", flush=True)
process.kill()
