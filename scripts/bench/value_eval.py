#!/usr/bin/env python3
"""Compare function value edges from the semantic index and from syntax rules, with their sites.
Usage: value_eval.py SEMANTIC_GRAPH.json SYNTAX_GRAPH.json REPO OUT.json"""
import collections, json, pathlib, random, sys
sem_path, syn_path, repo, out = sys.argv[1:5]
repo = pathlib.Path(repo)
def load(path):
    g = json.load(open(path))
    F = {v: k for k, v in g["file_ids"].items()}
    S = {s["id"]: s for s in g["symbols"]}
    kinds = collections.Counter(e[2] for e in g["edges"])
    edges = {}
    for e in g["edges"]:
        if e[2] != "UsesAsValue": continue
        a, b = S[e[0]], S[e[1]]
        key = (F[a["file_id"]], a["name"], a["range"]["start_line"], F[b["file_id"]], b["name"], b["range"]["start_line"])
        src = (repo / F[a["file_id"]]).read_bytes()
        line = src[:e[3]].count(b"\n") + 1
        text = src[src.rfind(b"\n", 0, e[3]) + 1: src.find(b"\n", e[3])].decode(errors="ignore").strip()[:130]
        edges[key] = {"line": line, "text": text, "holder": a["kind"], "site_text": src[e[3]:e[4]].decode(errors="ignore")}
    return edges, kinds
sem, ks = load(sem_path); syn, ky = load(syn_path)
tp = set(sem) & set(syn)
fmt = lambda k, d: f'{k[0]}:{d["line"]} [{k[1]}/{d["holder"]}] -> {k[3]}:{k[5]} {k[4]} :: {d["text"]}'
random.seed(11)
rep = {"semantic_kinds": ks, "syntax_kinds": ky, "semantic": len(sem), "syntax": len(syn), "both": len(tp),
       "syntax_precision": round(len(tp)/max(1,len(syn)),4), "syntax_recall": round(len(tp)/max(1,len(sem)),4),
       "holder_kinds": collections.Counter(d["holder"] for d in sem.values()),
       "site_text_ne_name": [fmt(k, d) + f' site={d["site_text"]!r}' for k, d in sem.items() if d["site_text"] != k[4]],
       "only_syntax": [fmt(k, syn[k]) for k in sorted(set(syn)-set(sem))],
       "only_semantic_sample": [fmt(k, sem[k]) for k in random.sample(sorted(set(sem)-set(syn)), min(25, len(set(sem)-set(syn))))],
       "semantic_sample": [fmt(k, sem[k]) for k in random.sample(sorted(sem), min(30, len(sem)))]}
json.dump(rep, open(out, "w"), indent=1, default=dict)
print(json.dumps(rep, indent=1, default=dict))
