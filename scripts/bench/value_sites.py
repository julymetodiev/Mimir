#!/usr/bin/env python3
"""List non-call occurrences of project functions in a SCIP index, with their syntax context.
Usage: value_sites.py INDEX.scip REPO OUT.json"""
import collections, json, pathlib, sys
import tree_sitter, tree_sitter_rust
scip_path, repo, out = sys.argv[1:4]
repo = pathlib.Path(repo)
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
for f, v in fields(pathlib.Path(scip_path).read_bytes()):
    if f != 2: continue
    path, occ = None, []
    for g, w in fields(v):
        if g == 1: path = w.decode()
        elif g == 2:
            o = {"r": [], "s": "", "roles": 0}
            for h, x in fields(w):
                if h == 1: o["r"] = packed(x) if isinstance(x, bytes) else o["r"]+[x]
                elif h == 2: o["s"] = x.decode()
                elif h == 3: o["roles"] = x
            occ.append(o)
    docs[path] = occ
defs = {o["s"] for occ in docs.values() for o in occ if o["roles"] & 1 and o["s"].endswith("().")}
parser = tree_sitter.Parser(tree_sitter.Language(tree_sitter_rust.language()))
ctx = collections.Counter(); rows = []; roles = collections.Counter()
for path, occ in sorted(docs.items()):
    if not path.startswith("crates/") and not path.startswith("fixtures/"): continue
    src = (repo/path).read_bytes(); lines = src.decode().split("\n")
    tree = parser.parse(src)
    for o in occ:
        if o["roles"] & 1 or o["s"] not in defs: continue
        r = o["r"]; l, c = r[0], r[1]; el, ec = (r[0], r[2]) if len(r) == 3 else (r[2], r[3])
        after = lines[el].encode()[ec:].decode(errors="ignore")
        if after.startswith("(") or after.startswith("::<"): continue
        n = tree.root_node.named_descendant_for_point_range((l, c), (el, ec))
        chain = []; p = n
        while p is not None and len(chain) < 4:
            chain.append(p.type); p = p.parent
        anc = []; p = n
        while p is not None:
            anc.append(p.type); p = p.parent
        k = "use" if "use_declaration" in anc else "doc" if n.type in ("line_comment","block_comment","doc_comment") or "line_comment" in anc else "attr" if "attribute_item" in anc else ">".join(chain[:3])
        ctx[k] += 1; roles[(o["roles"])] += 1
        rows.append({"k": k, "where": f"{path}:{l+1}:{c}", "text": lines[l].strip()[:110], "sym": o["s"].split(" ")[-1], "roles": o["roles"], "occ_text": lines[l].encode()[c:ec].decode(errors="ignore") if l == el else None})
json.dump(rows, open(out, "w"), indent=0)
print("total", len(rows), "roles", dict(roles))
for k, v in ctx.most_common(): print(v, k)
seen = collections.Counter()
for r in rows:
    if seen[r["k"]] < 3:
        seen[r["k"]] += 1; print(f'  [{r["k"]}] {r["where"]} occ={r["occ_text"]!r} sym={r["sym"]} :: {r["text"]}')
