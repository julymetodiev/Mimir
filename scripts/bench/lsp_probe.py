#!/usr/bin/env python3
"""Can a resident rust-analyzer answer for one edited file? Load time, memory, latency, agreement with SCIP.
Usage: lsp_probe.py ROOT INDEX.scip FILE [FILE...]  (nothing is written to disk)"""
import json, os, pathlib, queue, re, subprocess, sys, threading, time
root = pathlib.Path(sys.argv[1]).resolve(); scip = pathlib.Path(sys.argv[2]); targets = sys.argv[3:]
def varint(b, i):
    r = s = 0
    while True:
        c = b[i]; i += 1
        r |= (c & 127) << s
        if c < 128: return r, i
        s += 7
def fields(b):
    i = 0
    while i < len(b):
        k, i = varint(b, i); f, w = k >> 3, k & 7
        if w == 0: v, i = varint(b, i)
        elif w == 2:
            n, i = varint(b, i); v, i = b[i:i+n], i+n
        elif w == 1: v, i = b[i:i+8], i+8
        elif w == 5: v, i = b[i:i+4], i+4
        else: raise ValueError(w)
        yield f, v
def packed(b):
    o, i = [], 0
    while i < len(b):
        v, i = varint(b, i); o.append(v)
    return o
docs = {}
for f, v in fields(scip.read_bytes()):
    if f != 2: continue
    path, occ = None, []
    for g, w in fields(v):
        if g == 1: path = w.decode()
        elif g == 2:
            o = {"r": [], "s": "", "roles": 0}
            for h, x in fields(w):
                if h == 1: o["r"] = packed(x) if isinstance(x, bytes) else o["r"] + [x]
                elif h == 2: o["s"] = x.decode()
                elif h == 3: o["roles"] = x
            occ.append(o)
    docs[path] = occ
defs = {}
for path, occ in docs.items():
    for o in occ:
        if o["roles"] & 1 and o["s"].endswith("()."):
            defs[o["s"]] = (path, o["r"][0])
CONFIG = {"checkOnSave": False, "cargo": {"allTargets": True, "buildScripts": {"enable": True}},
          "procMacro": {"enable": True}, "cachePriming": {"enable": False}, "diagnostics": {"enable": False}}
proc = subprocess.Popen(["rust-analyzer"], cwd=root, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
pending, lock, nid = {}, threading.Lock(), [0]
status = {"quiescent": False}
def send(m):
    b = json.dumps(m).encode()
    with lock:
        proc.stdin.write(b"Content-Length: %d\r\n\r\n" % len(b) + b); proc.stdin.flush()
def reader():
    while True:
        n = None
        while True:
            l = proc.stdout.readline()
            if not l: return
            if l in (b"\r\n", b"\n"): break
            if l.lower().startswith(b"content-length:"): n = int(l.split(b":")[1])
        m = json.loads(proc.stdout.read(n))
        if "method" in m and "id" in m:
            r = [CONFIG for _ in m["params"]["items"]] if m["method"] == "workspace/configuration" else None
            send({"jsonrpc": "2.0", "id": m["id"], "result": r})
        elif "method" in m:
            if m["method"] == "experimental/serverStatus": status.update(m["params"])
        elif m.get("id") in pending: pending.pop(m["id"]).put(m)
threading.Thread(target=reader, daemon=True).start()
def req(method, params, timeout=300):
    for a in range(6):
        nid[0] += 1; i = nid[0]; q = queue.Queue(1); pending[i] = q
        send({"jsonrpc": "2.0", "id": i, "method": method, "params": params})
        m = q.get(timeout=timeout)
        if "error" in m:
            if m["error"].get("code") in (-32801, -32802): time.sleep(0.2); continue
            return None
        return m.get("result")
def rss():
    out = subprocess.run(["ps", "-o", "rss=", "-p", str(proc.pid)], capture_output=True, text=True).stdout.strip()
    return round(int(out) / 1048576, 2)
t0 = time.time()
req("initialize", {"processId": os.getpid(), "rootUri": root.as_uri(), "workspaceFolders": [{"uri": root.as_uri(), "name": "p"}],
    "initializationOptions": CONFIG, "capabilities": {"window": {"workDoneProgress": True}, "workspace": {"configuration": True},
    "textDocument": {"definition": {"linkSupport": False}}, "experimental": {"serverStatusNotification": True}}})
send({"jsonrpc": "2.0", "method": "initialized", "params": {}})
while not status["quiescent"]: time.sleep(0.5)
print(f"load: {time.time()-t0:.1f}s rss={rss()}GB", flush=True)
SITE = re.compile(r"\b([a-zA-Z_][a-zA-Z0-9_]*)(?=\(|::<)")
def sites(text):
    for ln, line in enumerate(text.split("\n")):
        if line.lstrip().startswith("//"): continue
        for m in SITE.finditer(line):
            if m.group(1) not in ("if", "for", "while", "match", "fn", "return", "Some", "Ok", "Err"):
                yield ln, m.start(), m.group(1)
def definition(uri, ln, col):
    r = req("textDocument/definition", {"textDocument": {"uri": uri}, "position": {"line": ln, "character": col}})
    if not r: return None
    r = r if isinstance(r, list) else [r]
    loc = r[0]; u = loc.get("uri") or loc.get("targetUri"); rng = loc.get("range") or loc.get("targetSelectionRange")
    p = u.replace("file://", "")
    try: rel = str(pathlib.Path(p).resolve().relative_to(root))
    except ValueError: return ("outside", 0)
    return (rel, rng["start"]["line"])
for path in targets:
    full = root / path; uri = full.as_uri(); text = full.read_text()
    send({"jsonrpc": "2.0", "method": "textDocument/didOpen", "params": {"textDocument": {"uri": uri, "languageId": "rust", "version": 1, "text": text}}})
    ss = list(sites(text))
    truth = {}
    for o in docs.get(path, []):
        if o["roles"] & 1 or not o["s"].endswith("().") or o["s"] not in defs: continue
        truth[(o["r"][0], o["r"][1])] = defs[o["s"]]
    for label in ("first", "after edit", "after edit 2"):
        if label != "first":
            send({"jsonrpc": "2.0", "method": "textDocument/didChange", "params": {"textDocument": {"uri": uri, "version": int(time.time())},
                 "contentChanges": [{"range": {"start": {"line": len(text.split(chr(10))) - 1, "character": 0}, "end": {"line": len(text.split(chr(10))) - 1, "character": 0}}, "text": "// edit\n"}]}})
        t = time.time(); first = None; agree = dis = none = 0; bad = []
        for ln, col, name in ss:
            d = definition(uri, ln, col)
            if first is None: first = time.time() - t
            exp = truth.get((ln, col))
            if exp is None: continue
            if d == exp: agree += 1
            elif d is None: none += 1
            else:
                dis += 1; bad.append((ln + 1, name, exp, d))
        print(f"{path} [{label}]: sites={len(ss)} total={time.time()-t:.2f}s first={first:.2f}s scip_sites={len(truth)} agree={agree} differ={dis} none={none} rss={rss()}GB", flush=True)
        if label == "first":
            for b in bad[:5]: print("   differ", b)
proc.terminate()
