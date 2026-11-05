#!/usr/bin/env python3
"""Every language, every kind of edit: the graph after the edit against a clean start of the same files.

Usage: edit_matrix.py [MIMIR]
Each case is a small project in a temporary git repository, so this takes under a minute and needs only
the language servers. Exit 1 if any case differs.
"""
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time

ENV = dict(os.environ, GIT_CONFIG_GLOBAL="/dev/null")
MIMIR = sys.argv[1] if len(sys.argv) > 1 else "mimir"


def script(ext):
    return {
        "main": f"types.{ext}",
        "comment": "//",
        "head": "",
        "a": "export class A {\n  name() { return 1; }\n}\n",
        "b": "export class B {\n  name() { return 2; }\n}\n",
        "c": "export class C {\n  name() { return 0; }\n}\n",
        "tail": "export class Base {\n  go() { return 3; }\n}\n",
        "rest": {
            f"mid.{ext}": 'import { Base } from "./types";\nexport class Mid extends Base {}\n',
            f"use.{ext}": 'import { A, B } from "./types";\nimport { Mid } from "./mid";\n'
            "export function useA() { return new A().name(); }\n"
            "export function useB() { return new B().name(); }\n"
            "export function useMid() { return new Mid().go(); }\n",
            "tsconfig.json": '{"compilerOptions":{"strict":true,"allowJs":true,"checkJs":true,"jsx":"preserve"}}',
        },
    }


LANGUAGES = {
    "rust": {
        "main": "src/types.rs",
        "comment": "//",
        "head": "pub struct A;\npub struct B;\npub struct C;\n\n",
        "a": "impl A { pub fn name(&self) -> u8 { 1 } }\n",
        "b": "impl B { pub fn name(&self) -> u8 { 2 } }\n",
        "c": "impl C { pub fn name(&self) -> u8 { 0 } }\n",
        "tail": "pub trait Base { fn go(&self) -> u8 { 3 } }\n",
        "rest": {
            "Cargo.toml": '[package]\nname = "matrix"\nversion = "0.1.0"\nedition = "2021"\n',
            "src/mid.rs": "use crate::types::Base;\npub struct Mid;\nimpl Base for Mid {}\n",
            "src/main.rs": "mod mid;\nmod types;\nuse mid::Mid;\nuse types::{Base, A, B};\n\n"
            "fn use_a() -> u8 { A.name() }\nfn use_b() -> u8 { B.name() }\nfn use_mid() -> u8 { Mid.go() }\n\n"
            "fn main() { let _ = use_a() + use_b() + use_mid(); }\n",
        },
    },
    "go": {
        "main": "types.go",
        "comment": "//",
        "head": "package main\n\ntype A struct{}\ntype B struct{}\ntype C struct{}\ntype Base struct{}\n\n",
        "a": 'func (a *A) Name() string { return "a" }\n',
        "b": 'func (B) Name() string { return "b" }\n',
        "c": 'func (C) Name() string { return "c" }\n',
        "tail": 'func (Base) Go() string { return "d" }\n',
        "rest": {
            "go.mod": "module matrix\n\ngo 1.22\n",
            "mid.go": "package main\n\ntype Mid struct{ Base }\n",
            "use.go": "package main\n\nfunc useA() string { return (&A{}).Name() }\n"
            "func useB() string { return B{}.Name() }\nfunc useMid() string { return Mid{}.Go() }\n\n"
            "func main() { _ = useA() + useB() + useMid() }\n",
        },
    },
    "python": {
        "main": "types_.py",
        "comment": "#",
        "head": "",
        "a": "class A:\n    def name(self):\n        return 1\n",
        "b": "class B:\n    def name(self):\n        return 2\n",
        "c": "class C:\n    def name(self):\n        return 0\n",
        "tail": "class Base:\n    def go(self):\n        return 3\n",
        "rest": {
            "mid.py": "from types_ import Base\n\n\nclass Mid(Base):\n    pass\n",
            "use.py": "from mid import Mid\nfrom types_ import A, B\n\n\ndef use_a():\n    return A().name()\n\n\n"
            "def use_b():\n    return B().name()\n\n\ndef use_mid():\n    return Mid().go()\n",
        },
    },
    "typescript": script("ts"),
    "tsx": script("tsx"),
    "javascript": script("js"),
}


def source(language, *members, top=""):
    return top + language["head"] + "\n".join(language[m] for m in members) + "\n" + language["tail"]


def edits(language):
    comment = language["comment"]
    return {
        "comment on top": source(language, "a", "b", top=f"{comment} moved\n"),
        "longer comment": source(language, "a", "b", top=f"{comment} one line more\n{comment} and another\n"),
        "members swapped": source(language, "b", "a"),
        "member added above": source(language, "c", "a", "b"),
    }


def run(*args, cwd):
    return subprocess.run(list(args), cwd=cwd, capture_output=True, text=True, env=ENV, timeout=180)


def graph(root):
    g = json.load(open(root + "/.mimir/graph/graph.json"))
    files = {v: k for k, v in g["file_ids"].items()}
    symbols = {s["id"]: s for s in g["symbols"]}

    def place(i):
        s = symbols[i]
        return f'{files[s["file_id"]]}:{"::".join(s["qualified_name"]["segments"])}@{s["range"]["start_line"]}'

    return {(str(e[2]), place(e[0]), place(e[1])) for e in g["edges"]}


bad = 0
for name, language in LANGUAGES.items():
    for edit, text in edits(language).items():
        with tempfile.TemporaryDirectory() as directory:
            root = os.path.realpath(directory)
            for path, content in {**language["rest"], language["main"]: source(language, "a", "b")}.items():
                os.makedirs(os.path.dirname(f"{root}/{path}"), exist_ok=True)
                open(f"{root}/{path}", "w").write(content)
            run("git", "init", "-q", cwd=root)
            started = time.time()
            run(MIMIR, "sync", "--json", cwd=root)
            open(f"{root}/{language['main']}", "w").write(text)
            run(MIMIR, "sync", "--json", cwd=root)
            edited = graph(root)
            run(MIMIR, "daemon", "stop", cwd=root)
            shutil.rmtree(root + "/.mimir")
            run(MIMIR, "sync", "--json", cwd=root)
            clean = graph(root)
            run(MIMIR, "daemon", "stop", cwd=root)
            extra, lost = sorted(edited - clean), sorted(clean - edited)
            same = not extra and not lost
            bad += not same
            print(f"{'ok ' if same else 'BAD'} {name:<11}{edit:<20}{len(clean):>3} edges, {time.time() - started:4.1f}s", flush=True)
            for kind, source_at, target in extra[:4]:
                print(f"      only after the edit: {kind} {source_at} -> {target}", flush=True)
            for kind, source_at, target in lost[:4]:
                print(f"      only from a clean start: {kind} {source_at} -> {target}", flush=True)
print(f"{bad} differ")
sys.exit(1 if bad else 0)
