#!/usr/bin/env python3
"""For each cached SCIP index of a project: its documents, and which files it adds. Usage: roots.py PROJECT"""
import glob, os, pathlib, sys, collections
proj = pathlib.Path(sys.argv[1])
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
idx = []
for p in sorted(glob.glob(str(proj/".latt/graph/*.scip")), key=os.path.getmtime):
    docs = []; meta = ""
    for f, v in fields(pathlib.Path(p).read_bytes()):
        if f == 1:
            for g, w in fields(v):
                if g == 3: meta = w.decode()
        if f == 2:
            for g, w in fields(v):
                if g == 1: docs.append(w.decode()); break
    idx.append((meta.replace("file://", ""), docs, os.path.getsize(p)))
all_rs = {str(x.relative_to(proj)) for x in proj.rglob("*.rs") if "target" not in x.parts}
base = idx[0][0].rstrip("/")
seen = set()
for meta, docs, size in idx:
    rel = os.path.relpath(meta, base) if meta else "?"
    abs_docs = {os.path.normpath(os.path.join(rel, d)) for d in docs}
    new = abs_docs - seen
    print(f"root={rel!r} docs={len(docs)} new={len(new)} size={size//1024}KB")
    for d in sorted(new)[:6] if seen else []: print("   +", d)
    seen |= abs_docs
miss = sorted(all_rs - seen)
print("uncovered", len(miss))
for m in miss: print("  ", m)
