#!/usr/bin/env python3
"""Build call-edge truth from a rust-analyzer SCIP index, check it, and score resolvers on it.

Usage: scip_eval.py INDEX.scip LATT_GRAPH.json RA.jsonl REPO OUT.json
Scoring unit: unique (caller fn, callee fn) under crates/. Function values passed as arguments
are not calls and are excluded from truth and from both resolvers.
"""
import collections, json, pathlib, random, sys
import tree_sitter, tree_sitter_rust

scip_path, graph_path, ra_path, repo, out_path = sys.argv[1:6]
repo = pathlib.Path(repo)


# Minimal protobuf wire reader (scip.proto: Index.documents=2; Document.relative_path=1,
# occurrences=2; Occurrence.range=1, symbol=2, symbol_roles=3)
def varint(b, i):
    r = s = 0
    while True:
        c = b[i]
        i += 1
        r |= (c & 127) << s
        if c < 128:
            return r, i
        s += 7


def fields(b):
    i = 0
    while i < len(b):
        k, i = varint(b, i)
        f, w = k >> 3, k & 7
        if w == 0:
            v, i = varint(b, i)
        elif w == 2:
            n, i = varint(b, i)
            v, i = b[i:i + n], i + n
        elif w == 1:
            v, i = b[i:i + 8], i + 8
        elif w == 5:
            v, i = b[i:i + 4], i + 4
        else:
            raise ValueError(w)
        yield f, v


def packed(b):
    out, i = [], 0
    while i < len(b):
        v, i = varint(b, i)
        out.append(v)
    return out


docs = {}
for f, v in fields(pathlib.Path(scip_path).read_bytes()):
    if f != 2:
        continue
    path, occ = None, []
    for g, w in fields(v):
        if g == 1:
            path = w.decode()
        elif g == 2:
            o = {"range": [], "symbol": "", "roles": 0}
            for h, x in fields(w):
                if h == 1:
                    o["range"] = packed(x) if isinstance(x, bytes) else o["range"] + [x]
                elif h == 2:
                    o["symbol"] = x.decode()
                elif h == 3:
                    o["roles"] = x
            occ.append(o)
    docs[path] = occ

# Tree-sitter index of every function under crates/
lang = tree_sitter.Language(tree_sitter_rust.language())
parser = tree_sitter.Parser(lang)
trees, fns, fn_at = {}, {}, {}


def impl_type(n):
    p = n.parent
    while p is not None:
        if p.type == "impl_item":
            t = p.child_by_field_name("type")
            tr = p.child_by_field_name("trait")
            name = lambda x: x.text.decode().split("<")[0].split("::")[-1].lstrip("&").strip() if x else None
            return name(t), name(tr)
        if p.type == "trait_item":
            return p.child_by_field_name("name").text.decode(), None
        if p.type == "function_item":
            return None, None
        p = p.parent
    return None, None


files = sorted(p for p in (repo / "crates").rglob("*.rs"))
for p in files:
    rel = str(p.relative_to(repo))
    src = p.read_bytes()
    tree = parser.parse(src)
    trees[rel] = (tree, src, src.decode(errors="replace").split("\n"))
    stack = [tree.root_node]
    while stack:
        n = stack.pop()
        stack.extend(n.children)
        if n.type in ("function_item", "function_signature_item"):
            nm = n.child_by_field_name("name")
            k = (rel, nm.start_point[0], nm.text.decode())
            ty, tr = impl_type(n)
            fns[k] = {"type": ty, "trait": tr, "has_body": n.type == "function_item"}
            fn_at[(rel, nm.start_point[0], nm.start_point[1])] = k


def enclosing_fn(rel, line, col):
    tree = trees[rel][0]
    n = tree.root_node.named_descendant_for_point_range((line, col), (line, col))
    in_macro = False
    res = None
    while n is not None:
        if n.type == "token_tree":
            in_macro = True
        if n.type == "function_item" and res is None:
            nm = n.child_by_field_name("name")
            res = (rel, nm.start_point[0], nm.text.decode())
        n = n.parent
    return res, in_macro


# SCIP definitions and references
ROLE_DEF, ROLE_IMPORT = 1, 2
defs = {}
for rel, occ in docs.items():
    if not rel.startswith("crates/"):
        continue
    for o in occ:
        if o["roles"] & ROLE_DEF and o["symbol"].endswith("()."):
            k = fn_at.get((rel, o["range"][0], o["range"][1]))
            if k:
                defs[o["symbol"]] = k

T, T_macro_only, sites = set(), {}, collections.defaultdict(list)
scip_refs = skipped_no_def = skipped_value = 0
for rel, occ in docs.items():
    if rel not in trees:
        continue
    lines = trees[rel][2]
    for o in occ:
        s = o["symbol"]
        if o["roles"] & (ROLE_DEF | ROLE_IMPORT) or not s.endswith("()."):
            continue
        r = o["range"]
        line, col = r[0], r[1]
        end = r[-1]
        rest = lines[line][end:] if line < len(lines) else ""
        if not (rest.startswith("(") or rest.startswith("::<")):
            skipped_value += 1
            continue
        scip_refs += 1
        callee = defs.get(s)
        if callee is None:
            skipped_no_def += 1  # std, dependency, or a derive
            continue
        caller, in_macro = enclosing_fn(rel, line, col)
        if caller is None:
            continue
        e = (caller, callee)
        T.add(e)
        T_macro_only[e] = T_macro_only.get(e, True) and in_macro
        sites[e].append((rel, line, col, lines[line].strip()[:110]))

# Lantern Lattice, mapped into the same keys
g = json.load(open(graph_path))
FILES = {v: k for k, v in g["file_ids"].items()}
syms = {s["id"]: s for s in g["symbols"]}
by_loc = {}
for k in fns:
    by_loc.setdefault((k[0], k[2]), []).append(k)


def to_key(s):
    c = [k for k in by_loc.get((FILES[s["file_id"]], s["name"]), [])
         if s["range"]["start_line"] <= k[1] + 1 <= s["range"]["end_line"]]
    return min(c, key=lambda k: k[1]) if c else None


M = set()
latt_unmapped = 0
for f, t, kind, *_ in g["edges"]:
    if kind != "Calls":
        continue
    a, b = syms.get(f), syms.get(t)
    if not a or not b or a["kind"] not in ("function", "method") or b["kind"] not in ("function", "method"):
        continue
    if not FILES[a["file_id"]].startswith("crates/"):
        continue
    ka, kb = to_key(a), to_key(b)
    if ka is None or kb is None:
        latt_unmapped += 1
        continue
    M.add((ka, kb))

# rust-analyzer call hierarchy, same keys
H = set()
for line in open(ra_path):
    d = json.loads(line)
    if "caller" not in d:
        continue
    a = (d["caller"]["file"], d["caller"]["line"], d["caller"]["name"])
    b = (d["callee"]["file"], d["callee"]["line"], d["callee"]["name"])
    if a in fns and b in fns:
        H.add((a, b))


# A resolver that only uses tree-sitter
by_name = collections.defaultdict(list)
by_type = collections.defaultdict(list)
for k, v in fns.items():
    by_name[k[2]].append(k)
    if v["type"]:
        by_type[(v["type"], k[2])].append(k)


def call_sites(root):
    """(kind, name, qualifier/receiver, line, col, caller). Reads macro token trees as tokens too."""
    out, stack = [], [(root, None)]
    while stack:
        n, cur = stack.pop()
        if n.type == "function_item":
            nm = n.child_by_field_name("name")
            cur = nm.start_point[0], nm.text.decode()
        if n.type == "call_expression" and cur:
            f = n.child_by_field_name("function")
            if f.type == "generic_function":
                f = f.child_by_field_name("function")
            if f.type == "identifier":
                out.append(("plain", f.text.decode(), None, cur))
            elif f.type == "field_expression":
                out.append(("method", f.child_by_field_name("field").text.decode(),
                            f.child_by_field_name("value").text.decode(), cur))
            elif f.type == "scoped_identifier":
                q = f.child_by_field_name("path")
                out.append(("scoped", f.child_by_field_name("name").text.decode(),
                            q.text.decode() if q else "", cur))
        if n.type == "token_tree" and cur:
            ch = n.children
            for i, c in enumerate(ch):
                if c.type == "token_tree" and i and c.children and c.children[0].type == "(" and ch[i - 1].type == "identifier":
                    name = ch[i - 1].text.decode()
                    prev = ch[i - 2].type if i >= 2 else ""
                    if prev == ".":
                        recv = ch[i - 3].text.decode() if i >= 3 else ""
                        out.append(("method", name, recv, cur))
                    elif prev == "::":
                        q = ch[i - 3].text.decode() if i >= 3 else ""
                        out.append(("scoped", name, q, cur))
                    else:
                        out.append(("plain", name, None, cur))
        stack.extend((c, cur) for c in n.children)
    return out


def resolve(rel, caller_key, kind, name, q, guess_methods):
    me = fns.get(caller_key, {})
    if kind == "plain":
        c = [k for k in by_name[name] if k[0] == rel and not fns[k]["type"]]
        if len(c) == 1:
            return c
        c = [k for k in by_name[name] if not fns[k]["type"]]
        return c if len(c) == 1 else []
    if kind == "scoped":
        t = q.split("<")[0].split("::")[-1]
        if t == "Self":
            t = me.get("type")
        c = by_type.get((t, name), [])
        if c:
            return c[:1] if len(c) == 1 else []
        c = [k for k in by_name[name] if not fns[k]["type"]]
        return c if len(c) == 1 else []
    if q == "self" and me.get("type"):
        c = by_type.get((me["type"], name), [])
        if len(c) == 1:
            return c
    if guess_methods and len(by_name[name]) == 1:
        return by_name[name]
    return []


def run_resolver(guess_methods, use_macro=True):
    R = set()
    for rel, (tree, _, _) in trees.items():
        for kind, name, q, cur in call_sites(tree.root_node):
            caller = (rel, cur[0], cur[1])
            for t in resolve(rel, caller, kind, name, q, guess_methods):
                R.add((caller, t))
    return R


def prf(Tr, Pr):
    tp = len(Tr & Pr)
    p, r = tp / len(Pr) if Pr else 0, tp / len(Tr) if Tr else 0
    return {"truth": len(Tr), "predicted": len(Pr), "tp": tp, "fp": len(Pr - Tr), "fn": len(Tr - Pr),
            "precision": round(p, 4), "recall": round(r, 4), "f1": round(2 * p * r / (p + r), 4) if p + r else 0}


crate = lambda k: k[0].split("/")[1]
slices = {
    "same_file": lambda e: e[0][0] == e[1][0],
    "cross_file_same_crate": lambda e: e[0][0] != e[1][0] and crate(e[0]) == crate(e[1]),
    "cross_crate": lambda e: crate(e[0]) != crate(e[1]),
    "macro_only_call": lambda e: T_macro_only.get(e, False),
}


def slice_all(Tr, Pr):
    return {n: prf({e for e in Tr if f(e)}, {e for e in Pr if f(e)}) for n, f in slices.items()}


R_strict = run_resolver(False)
R_guess = run_resolver(True)
report = {
    "counts": {
        "documents": len(docs), "fns_treesitter": len(fns), "scip_fn_defs_matched": len(defs),
        "call_refs": scip_refs, "call_refs_to_outside_or_derive": skipped_no_def,
        "fn_refs_not_followed_by_paren": skipped_value, "truth_edges": len(T),
        "truth_macro_only": sum(1 for e in T if T_macro_only[e]), "latt_edges_mapped": len(M),
        "latt_edges_unmapped": latt_unmapped,
    },
    "hierarchy_vs_scip": {"hierarchy": len(H), "both": len(H & T), "only_hierarchy": len(H - T), "only_scip": len(T - H),
                          "only_scip_macro_only": sum(1 for e in T - H if T_macro_only[e])},
    "latt": prf(T, M), "latt_slices": slice_all(T, M),
    "ts_resolver_no_method_guess": prf(T, R_strict), "ts_resolver_no_method_guess_slices": slice_all(T, R_strict),
    "ts_resolver_unique_method_guess": prf(T, R_guess), "ts_resolver_unique_method_guess_slices": slice_all(T, R_guess),
}

# Which truth edges has the tree-sitter resolver left, by call form?
form = collections.Counter()
for e in T - R_guess:
    txt = sites[e][0][3]
    nm = e[1][2]
    form["method_call" if ("." + nm) in txt else "scoped_call" if ("::" + nm) in txt else "plain_call"] += 1
report["ts_resolver_missed_by_form"] = dict(form)
amb = collections.Counter(e[1][2] for e in T - R_guess)
report["ts_resolver_top_missed_names"] = amb.most_common(10)

random.seed(11)
fmt = lambda e: f"{e[0][0]}:{e[0][1]+1} {e[0][2]} -> {e[1][0]}:{e[1][1]+1} {e[1][2]}"
sample = lambda s, n: random.sample(sorted(s), min(n, len(s)))
report["audit_truth_sample"] = [{"edge": fmt(e), "site": sites[e][0]} for e in sample(T, 15)]
report["audit_only_hierarchy"] = [fmt(e) for e in sample(H - T, 10)]
report["audit_only_scip"] = [{"edge": fmt(e), "site": sites[e][0]} for e in sample(T - H, 10)]
report["audit_latt_fp"] = [fmt(e) for e in sample(M - T, 12)]
report["audit_latt_fn"] = [fmt(e) for e in sample(T - M, 12)]
report["audit_ts_guess_fp"] = [fmt(e) for e in sample(R_guess - T, 12)]
report["check"] = {t: sorted(fmt(e) for e in T if e[1][2] == t) for t in ("apply_batch", "find_by_name", "handle_trace")}

json.dump(report, open(out_path, "w"), indent=1, default=list)
print(json.dumps({k: v for k, v in report.items() if not k.startswith("audit") and k != "check"}, indent=0))
for k in report:
    if k.startswith("audit") or k == "check":
        print("##", k)
        print(json.dumps(report[k], indent=0)[:3200])
