#!/usr/bin/env python3
"""Edges in one graph.json and not the other, by kind. Usage: graph_diff.py A.json B.json"""
import json,sys,collections
def load(p):
    g=json.load(open(p)); F={v:k for k,v in g["file_ids"].items()}; S={s["id"]:s for s in g["symbols"]}
    out=collections.defaultdict(set)
    for e in g["edges"]:
        a,b=S[e[0]],S[e[1]]
        out[e[2]].add((F[a["file_id"]],a["name"],a["range"]["start_line"],F[b["file_id"]],b["name"],b["range"]["start_line"]))
    return out
A,B=load(sys.argv[1]),load(sys.argv[2])
for k in sorted(set(A)|set(B)):
    lost=A[k]-B[k]; new=B[k]-A[k]
    print(k,"live",len(A[k]),"full",len(B[k]),"only_live",len(lost),"only_full",len(new))
    for x in sorted(lost)[:14]: print("   live:",x)
    for x in sorted(new)[:14]: print("   full:",x)
