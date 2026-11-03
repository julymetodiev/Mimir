#!/usr/bin/env python3
"""Dump function -> function call edges from rust-analyzer's LSP call hierarchy.

Usage: ra_truth.py ROOT OUT.jsonl [--limit N]
Evaluates every .rs file under ROOT/crates. Prints progress to stdout.
"""
import json, os, subprocess, sys, threading, time, queue, urllib.parse, pathlib

root = pathlib.Path(sys.argv[1]).resolve()
out_path = sys.argv[2]
limit = int(sys.argv[sys.argv.index("--limit") + 1]) if "--limit" in sys.argv else None
FN_KINDS = {6, 9, 12}  # Method, Constructor, Function

CONFIG = {
    "checkOnSave": False,
    "cargo": {"allTargets": True, "buildScripts": {"enable": True}},
    "procMacro": {"enable": True},
    "cachePriming": {"enable": False},
    "diagnostics": {"enable": False},
}

proc = subprocess.Popen(
    ["rust-analyzer"], cwd=root, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
    stderr=open(out_path + ".ra-stderr.log", "w"),
)
pending, lock, next_id = {}, threading.Lock(), [0]
status = {"quiescent": False, "health": None, "message": None}
stats = {"retries": 0, "timeouts": 0, "errors": 0}


def send(msg):
    body = json.dumps(msg).encode()
    with lock:
        proc.stdin.write(b"Content-Length: %d\r\n\r\n" % len(body) + body)
        proc.stdin.flush()


def reader():
    while True:
        length = None
        while True:
            line = proc.stdout.readline()
            if not line:
                return
            if line in (b"\r\n", b"\n"):
                break
            if line.lower().startswith(b"content-length:"):
                length = int(line.split(b":")[1])
        msg = json.loads(proc.stdout.read(length))
        if "method" in msg and "id" in msg:  # request from the server
            result = None
            if msg["method"] == "workspace/configuration":
                result = [CONFIG for _ in msg["params"]["items"]]
            send({"jsonrpc": "2.0", "id": msg["id"], "result": result})
        elif "method" in msg:
            if msg["method"] == "experimental/serverStatus":
                status.update(msg["params"])
        elif msg.get("id") in pending:
            pending.pop(msg["id"]).put(msg)


threading.Thread(target=reader, daemon=True).start()


def request(method, params, timeout=120):
    for attempt in range(6):
        next_id[0] += 1
        rid = next_id[0]
        box = queue.Queue(1)
        pending[rid] = box
        send({"jsonrpc": "2.0", "id": rid, "method": method, "params": params})
        try:
            msg = box.get(timeout=timeout)
        except queue.Empty:
            stats["timeouts"] += 1
            pending.pop(rid, None)
            return None
        if "error" in msg:
            if msg["error"].get("code") in (-32801, -32802):  # ContentModified, cancelled
                stats["retries"] += 1
                time.sleep(0.5 * (attempt + 1))
                continue
            stats["errors"] += 1
            return None
        return msg.get("result")
    return None


t0 = time.time()
request("initialize", {
    "processId": os.getpid(),
    "rootUri": root.as_uri(),
    "workspaceFolders": [{"uri": root.as_uri(), "name": "bench"}],
    "initializationOptions": CONFIG,
    "capabilities": {
        "window": {"workDoneProgress": True},
        "workspace": {"configuration": True},
        "textDocument": {
            "documentSymbol": {"hierarchicalDocumentSymbolSupport": True},
            "callHierarchy": {},
        },
        "experimental": {"serverStatusNotification": True},
    },
}, timeout=120)
send({"jsonrpc": "2.0", "method": "initialized", "params": {}})

while not status["quiescent"]:
    time.sleep(1)
    if int(time.time() - t0) % 15 == 0:
        print(f"[load {time.time()-t0:5.0f}s] health={status['health']} {status['message']}", flush=True)
print(f"[load done {time.time()-t0:.1f}s] health={status['health']} message={status['message']}", flush=True)

files = sorted((root / "crates").rglob("*.rs"))
total_files = len(files)
if limit:
    files = files[:: max(1, total_files // limit)][:limit]


def rel(uri):
    p = urllib.parse.unquote(urllib.parse.urlparse(uri).path)
    try:
        return str(pathlib.Path(p).resolve().relative_to(root))
    except ValueError:
        return None  # outside the workspace (std, dependency)


def walk(symbols):
    for s in symbols or []:
        if s.get("kind") in FN_KINDS:
            yield s
        yield from walk(s.get("children"))


edges = fns = external = no_item = 0
t1 = time.time()
with open(out_path, "w") as out:
    for i, path in enumerate(files, 1):
        uri = path.as_uri()
        text = path.read_text()
        send({"jsonrpc": "2.0", "method": "textDocument/didOpen", "params": {"textDocument": {
            "uri": uri, "languageId": "rust", "version": 1, "text": text}}})
        syms = request("textDocument/documentSymbol", {"textDocument": {"uri": uri}})
        for s in walk(syms):
            fns += 1
            pos = s["selectionRange"]["start"]
            items = request("textDocument/prepareCallHierarchy",
                            {"textDocument": {"uri": uri}, "position": pos})
            caller = {"file": rel(uri), "line": pos["line"], "name": s["name"], "kind": s["kind"]}
            out.write(json.dumps({"fn": caller}) + "\n")
            if not items:
                no_item += 1
                continue
            for item in items:
                calls = request("callHierarchy/outgoingCalls", {"item": item}) or []
                for c in calls:
                    to = c["to"]
                    f = rel(to["uri"])
                    if f is None or not f.startswith("crates/"):
                        external += 1
                        continue
                    edges += 1
                    out.write(json.dumps({
                        "caller": caller,
                        "callee": {"file": f, "line": to["selectionRange"]["start"]["line"],
                                   "name": to["name"], "kind": to["kind"],
                                   "detail": to.get("detail")},
                        "ranges": c["fromRanges"],
                    }) + "\n")
        send({"jsonrpc": "2.0", "method": "textDocument/didClose",
              "params": {"textDocument": {"uri": uri}}})
        el = time.time() - t1
        if i % 5 == 0 or i == len(files):
            eta = el / i * (len(files) - i)
            print(f"[{i}/{len(files)}] fns={fns} edges={edges} external={external} "
                  f"elapsed={el:.0f}s eta={eta:.0f}s {stats}", flush=True)

el = time.time() - t1
print(f"DONE files={len(files)}/{total_files} fns={fns} edges={edges} external={external} "
      f"no_item={no_item} elapsed={el:.1f}s per_file={el/max(1,len(files)):.2f}s "
      f"projected_all={el/max(1,len(files))*total_files:.0f}s {stats}", flush=True)
proc.terminate()
