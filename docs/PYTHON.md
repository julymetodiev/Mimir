# Python

The specification for Python in section 8 of [ARCHITECTURE.md](ARCHITECTURE.md). The rules, layers and
targets there hold here too, and so does what [TYPESCRIPT.md](TYPESCRIPT.md) says unless this file
differs. **Status: built.** Section 8 says what is open.

## 1. Scope

Files: `.py`, `.pyi`, `.pyw`. Whatever git ignores is left out, so a `.venv` that git ignores gives
targets and no symbols.

Left out: notebooks, code inside strings, and any other file that is not one of those.

## 2. The parts

```
  latt-parse-python ── symbols and references, through tree-sitter
  latt-resolve      ── the same rules and the same port
  latt-resolve-lsp  ── asking a language server about each site, and saving the answers;
                        `tsrs.rs` and `pyrefly.rs` say what is each server's own
  latt-lsp          ── starting a language server, its messages, file URIs
```

| Crate | Layer | Owns |
|---|---|---|
| `latt-parse-python` | domain | Python through `tree-sitter-python` |
| `latt-resolve-lsp` | adapter | the state, the saved answers, which files to ask about after a change, stopping when idle; the module `pyrefly.rs` has the program, its version, its project files, its language id |

- **Nothing is copied.** A server is a module of a few lines in `latt-resolve-lsp`, and its tests are a
  folder of its own there. Go is expected to use it too.
- **Saved answers** are per source: `ty-answers.txt` beside `tsrs-answers.txt`, in the same format.
- **`commands/daemon.rs`** stays the only place that names the sources and the parsers.
- **`Language::Python`** already exists, and is a family of its own: linking never crosses to another.

## 3. Symbols and references

Symbols: function and `async` function, also nested; class, also nested; method, `classmethod`,
`staticmethod`, property; lambda bound to a name; a field, as an attribute of a class or the first
`self.x = ...` in a method; enum member; `type` alias; module-level constant and variable; and
`<module>`, a symbol every file has. Overloads and their implementation are one symbol, at the
implementation; with none, at the first signature.

| In the source | Reference | Edge |
|---|---|---|
| `f()`, `a.f()`, `(x or y).f()`, `f()()` | `Call` | `Calls` |
| `X()` | `Call` | `Calls`: the class's `__init__` if it declares one, otherwise the class |
| `@dec`, `@a.dec(1)` | `Call` | `Calls` |
| `a.x` where `x` is a property | `FieldUse`, `FieldWrite` | `Calls`: the getter for a read, the setter for a write, both for `a.x += 1` |
| `f` named without a call | `FunctionValue` | `UsesAsValue` |
| `import a`, `from a import b` | `Import` | none: a file is no symbol, and calls show what is used |
| a type named in an annotation | `TypeUse` | `UsesType` |
| `a.x` on a field | `FieldUse`, `FieldWrite` | `UsesField` |
| a constant read | `FunctionValue`: what it names decides | `UsesConstant` |
| `class A(B)` | as `impl Tr for T` | `Implements`: a possible target |

A caller is the innermost named function around the site. An unnamed function, a comprehension and a
lambda belong to the one around them. Code outside any function belongs to the constant that holds it,
or else to `<module>`. A name a function binds, as a parameter or a local, is no `FunctionValue`.

A call the source does not spell gives no reference: `a + b` running `__add__`, `with x:` running
`__enter__`, `for x in y:` running `__iter__`, attribute access through `__getattr__`.

## 4. Resolution

**The port stays**, and the answers are entries of `LiveDocument`, as for TypeScript. Linking uses the
same `resolve_answers`, with what differs below given by the language.

**Asking.** Only the sites the parser found, with `textDocument/definition`, by a background task after
the graph answers from syntax rules. One file is open at a time. Batches of 32. `pyrefly` counts columns in
UTF-16 and offers nothing else, so a column is changed on the way out and back.

**Only `definition`.** Where `tsrs` needs `typeDefinition` and `implementation`, `pyrefly` gives every member
of a union from `definition` alone, also with no name on the receiver (section 7). So each site costs one
question.

- **Several targets inside the project** give one edge each, marked possible.
- **An overload** answers with its first signature, which is the symbol at the implementation.
- **A class called** answers with its `__init__`, its own or the one it inherits, or else the class. An edge
  goes to that.
- **A target on an import** is asked again, as for TypeScript.
- **A target outside the project** gives no edge. **No answer** gives no edge, and `sync` reports the
  number. Calls are never guessed.

**A field** may be marked with its object, as `self.x`, so a target's name is what follows the last dot.

**Property.** `pyrefly` answers with the getter for a read and for a write. For a write the edge goes to the
method beside it with `@x.setter`, by the decorators in the source. This is the `accessor` of `script.rs`,
with one form for each language.

**What may run instead.** `c.area()` on a `Circle` is a `Calls` edge to `Circle.area`. A subclass that
overrides it is a possible target through `Implements`, as with a trait.

**Saved answers.** As for TypeScript. What changes all answers: `pyproject.toml`, `pyrefly.toml`, `setup.py`,
`setup.cfg`, `requirements.txt`, `requirements-dev.txt`, `uv.lock`, `poetry.lock`, `pdm.lock`, `Pipfile.lock`,
`.python-version`.

**An edit.** As for TypeScript: first the edited file, then each file that imports it, directly or not.
Finding importers needs Python's own rules for a path: `from . import a`, `from ..a import b`, `import a.b`,
a package's `__init__.py`, and a `src/` layout.

Told that a file changed on disk, `pyrefly` takes that in by itself and keeps answering from before until it
is done: 6 of 60 runs gave an importer the old target. So Lantern Lattice declares `window.workDoneProgress` and waits
for the `end` of `$/progress`: 0 of 200. It reports that for every change told, of any file, and only then.
That holds for a change nothing imports, which keeps the server. One with importers gets a new one: section 5.
`tsrs` needs no wait, 0 of 100 without one, and reports nothing.

**Syntax rules**, until answers come or without `pyrefly`: a call links only to a function in the same file
with nothing nearer of that name, or to a name `from a import b` takes from a project file with exactly
one public symbol of that name. No methods, no `import *`.

**`pyrefly` itself.**

- It is found on `PATH`, never in the project's environment.
- **It runs the project's Python.** A `.venv/bin/python` put in a project to leave a mark was run. It asks that
  program where the packages are, so a repository can choose what runs, as its build scripts can with
  `rust-analyzer`. `pyrefly check` has `--skip-interpreter-query`, and `pyrefly lsp` does not. See `SECURITY.md`.
- One exact version is the tested one, a constant in `latt-resolve-lsp/src/pyrefly.rs`, and
  `scripts/check-deps.py` compares it with the newest on PyPI (rule 10). Another version still runs, and the
  daemon's log says so.
- Without it only syntax rules apply, and `sync` reports how many files that covers.
- It stops after 10 idle minutes, like the others. It is never linked.
- Install: `pipx install pyrefly`, or `pip install pyrefly`. Neither needs Node.

**The project's packages.** `pyrefly` finds `.venv` at the top of a project by itself. Lantern Lattice installs nothing.
Without the packages less is answered, and `sync` says so.

## 5. Targets

Added to section 2 of the architecture when they hold, each checked by `scripts/bench`.

| # | Target |
|---|---|
| PY1 | Every entry of the hand-written list in `crates/latt-resolve-lsp/tests/pyrefly/main.rs` holds, covering each row of the table in section 3 |
| PY2 | Against a SCIP index by `scip-python`: every disagreement falls in a named class of section 7; unclassified 0 |
| PY3 | After edits, the graph equals the graph from a clean start with no saved answers: 0 differences |
| PY4 | T1, T2, T3 and T4 hold on a Python project of 20 000 symbols |
| PY5 | All sites of a project of 20 000 symbols are answered in under 60 s, on a current laptop |

Where they stand, measured through `latt` on an M1 Pro:

| | State |
|---|---|
| PY1 | Holds |
| PY2 | Not met: 2 unclassified on flask, and the comparison on django did not end in 15 minutes |
| PY3 | Not met in full on django: 0 differences for a comment on a base file and for a changed type, 1 extra edge after an edit of a file nothing imports. See below |
| PY4 | Not measured as `scripts/bench` does. T4 below: 539 MB on django, of 67 865 symbols |
| PY5 | Holds on what was measured: django, 67 865 symbols, 65 s, for over three times the symbols of the target |

| | flask | requests | django |
|---|---|---|---|
| Symbols | 2 046 | 1 114 | 67 865 |
| Calls | 1 118 | 951 | 61 849 |
| Uses of fields | 392 | 649 | 16 379 |
| First `sync` | 2.1 s | 1.5 s | 65 s |
| Restart with nothing changed, without starting `pyrefly` | 0.1 s | 0.1 s | 12 s |
| Daemon after asking | 31 MB | 27 MB | 539 MB |
| Daemon after that restart | 28 MB | 26 MB | 489 MB |
| `pyrefly` while it ran | 233 MB | 178 MB | 614 MB |

A restart gives the same graph as a clean start, 0 differences on all three. An edit of three files most of
django imports took 51 s, as long as a clean start of 53 s: they make nearly every file stale.

PY3 on django, 1860f0a, against a clean start with the same edit, on an M1 Pro:

| Edit | Differences | `sync` |
|---|---|---|
| A comment on top of `django/db/models/base.py` | 0 | 49 s |
| `-> int` on `QuerySet.filter` in `django/db/models/query.py` | 0 | 47 s |
| A comment on top of `tests/basic/tests.py`, which nothing imports | 1 extra: `check_dependencies` uses `context_processors` | 1 s |

Which graph is right at that edge, and why it comes, is not known.

With one `pyrefly` kept for all edits, the first lost 1 218 of 60 802 calls and the second 55. On three files,
20 of 20 runs: once a file changes, `pyrefly` 1.4.0 can keep the old line for a member inherited through another
file, and a new `pyrefly` gives the right one. Which it does depends on the text: with the same messages,
`# moved` on top keeps the old line and `# one line more` does not. Telling it of the middle file, opening
that file, editing as an editor does and `--indexing-mode` did not help. So a change with importers gets a new
`pyrefly`. It starts in 16 ms and asks only the stale files. `sync --full` gets one too.

With `pyrefly` 1.4.1 every edge on flask, requests and django is the same as with 1.4.0. A comment on top of `base.py` and `tests/basic/tests.py` together differs from a new index in 2 edges, the one above and `test_refresh_unsaved` using `db`, in `UsesField`. Whether 1.4.0 gave that second one was not run. Whether 1.4.1 still keeps the old line was not tried on its own, and a change with importers still gets a new `pyrefly`.

The first `trace` after an edit on django, depth 1, as against one `pyrefly` for all edits:

| Edit | One `pyrefly` | Now |
|---|---|---|
| 1 file nothing imports | 590 ms | 600 ms, and 860 ms, partial, for the first edit after a start |
| 1 file with few importers | 540 ms | 830 ms |
| 10 test files | 820 ms, partial | 880 ms, partial, complete 1.15 s later |

## 6. Machine checks

- `scripts/check-architecture.sh`: the new crates in `allowed`; `latt-engine` depends on no parser of a
  language; `latt-parse-python` is in the pure domain list.
- `scripts/check-deps.py`: the `pyrefly` version, from PyPI.
- `cargo test`: the fixture of PY1, skipped with a message when `pyrefly` is not installed. CI installs it.
- `scripts/bench`: PY2 to PY5, with `scip_compare.py`, which takes the server as an argument.

## 7. What was measured

`ty` 0.0.96, `pyrefly` 1.4.0 and `pyright` 1.1.414, 2026-11-03, on an M1 Pro. Packages were installed from
wheels, so none of their code ran at install. A site is each call in the source, found with Python's `ast`; one
file open at a time, batches of 32. **The percentages are how many sites got an answer, not how many answers
are right.**

| With packages | flask | requests | django |
|---|---|---|---|
| Files | 83 | 37 | 2 943 |
| Sites | 3 904 | 2 684 | 190 543 |
| Imports left unresolved, by `pyright` | 12 | 6 | 50 |
| `pyrefly`, answered | 71.8% | 95.1% | 78.9% |
| `pyrefly`, code / tests | 89.9% / 62.2% | 95.5% / 94.9% | 83.1% / 77.8% |
| `pyrefly`, time and memory | 1.6 s, 206 MB | 2.1 s, 169 MB | 51 s, 619 MB |
| `ty`, answered | 73.7% | 93.5% | 78.5% |
| `ty`, code / tests | 95.6% / 62.1% | 95.9% / 92.1% | 81.7% / 77.8% |
| `ty`, time and memory | 1.5 s, 174 MB | 0.7 s, 154 MB | 34 s, 780 MB |
| `pyright`, answered | 74.5% | 93.4% | not run |
| `pyright`, time and memory | 25 s, 928 MB | 33 s, 845 MB | not run |

Without its packages, an earlier `pyright` run gave flask 61.2% and requests 86.5%. The imports left on
django are mostly optional drivers, as `psycopg2` and `MySQLdb`, that were not installed.

**Does the order of asking change an answer?** `pyrefly` on all of django, each call and attribute, files in
order and in reverse: 299 205 sites, 0 different targets inside the project. `ty` fails this: see section 8.

**`pyrefly` against `scip-python` 0.6.8,** with `scripts/bench/scip_compare.py`. A site is any place the index
reports a reference, so more than the calls above. On django it did not end in 15 minutes, and gave nothing.

| | flask | requests |
|---|---|---|
| Inside, agree | 7 294 of 8 214 | 5 739 of 6 559 |
| Sites that name a function, agree | 739 of 778 | 414 of 434 |
| Index has a target, `pyrefly` none | 19 | 3 |
| Index has none, `pyrefly` does | 1 461 | 2 100 |
| Same name, another declaration | 793 | 671 |
| Constructor | 55 | 122 |
| Other named classes | 6 | 2 |
| Unclassified | 2 | 0 |

In "same name, another declaration", by samples read, most are local variables assigned twice: `pyrefly` gives
the last assignment before the site, as `url = cast(..., url)`, and the index the first. Those give no edge. Why
about 5% of the sites that name a function differ is not worked out; in samples there are a setter against
its getter and an overload against its signature.

The index is no truth. On django `ty` disagreed with it at 5 476 places where it named another class, as
`models.CharField` at `class DateTimeField`, in every sample read.

**`pyrefly` against `ty`, by the edges of Lantern Lattice's graphs:**

| Calls | Both | Only `ty` | Only `pyrefly` |
|---|---|---|---|
| flask | 1 013 | 108 | 36 |
| requests | 906 | 44 | 40 |
| django | 54 981 | 8 072 | 6 501 |

Most of this is one call described two ways: `IntegerField()` with an inherited constructor is the class in
`ty`, and `Field.__init__` in `pyrefly`. Leaving out edges to a class or a constructor, on django 989 are only in
`ty` and 1 217 only in `pyrefly`. Which is right at those was not checked.

**What is left unanswered in django's code,** by samples read, not counted: values without a type, as
`value.strip()` on a parameter with no annotation.

**The answers of `pyrefly`,** on a list written by hand:

| In the source | `definition` gives |
|---|---|
| `u.area()` on `u: Circle \| Square` | both methods |
| `make().area()`, returning a union | both methods |
| `(c if flag else Square()).area()` | both methods |
| `[c][0].area()` | `Circle.area` |
| `s.area()` on a `Protocol` | the protocol's method |
| `Circle(1)` | `Circle.__init__` |
| `Sub(1)`, inheriting it | `Circle.__init__` |
| `Point(1)` on a dataclass | the class |
| `over(1)` | the first signature |
| `alias()` after `alias = plain` | the variable, so no call of `plain` |
| `cls(1)` in a `classmethod` | nothing that gives an edge |
| `c.size`, `c.size = 2` | the getter, in both |

**Running the project.** A `.venv/bin/python` put in a project was run, as section 4 says. `pyrefly` found a
project's `.venv` without `VIRTUAL_ENV`: flask gave 71.8% either way.

## 8. Open

1. **`pyrefly` runs the project's Python,** as section 4 says. The option that stops it is not on `pyrefly lsp`.
2. **Correctness is agreement with another checker, which errs too,** and on django not even that was measured
   for `pyrefly`: PY2. Differences with `ty` were only sampled.
3. **Values without a type are left out:** about a fifth of the calls in django's own code. `[possible]`
   targets by name, kept apart from `Calls`, are an idea and are not specified. It needs its own measurement
   of how often they are right.
4. **Calls that give no edge here:** `alias()` through a variable, `cls()`, and on flask names in `examples/`, which
   has a `pyproject.toml` of its own, as `render_template`. Why those are not found was not worked out.
5. **Structural types.** A class that fits a `Protocol` without naming it gives no `Implements`.
6. **Calls the source does not spell**, as in section 3, and anything through `getattr`, or a dynamic
   decorator that returns another function.
7. **A package installed or removed** without a change to a file of section 4 leaves saved answers stale.
8. **Other environments:** a virtual environment elsewhere, conda, and `VIRTUAL_ENV` of another project in
   the shell that started the daemon, are unmeasured. Also unmeasured: uv workspaces, namespace packages,
   and a module with both `.py` and `.pyi`, where a target may land in the stub.
9. **Strings as annotations**, as `-> "Circle"`, have not been tried as sites.
10. **What `sync` says of missing packages** looks only for `.venv` beside a list of packages. A `VIRTUAL_ENV`
    or an environment elsewhere is not seen, so the note words it as a condition. Nothing checks that the
    environment holds what is listed.
11. **PY4** is not measured as `scripts/bench` does. On django the daemon holds 539 MB, for three times the
    symbols of T4, and a new daemon takes 12 s with nothing changed.
12. **Why not `ty`, for now.** It was the first choice, and is faster: 34 s against 51 s for `pyrefly` on django's
    calls, 0.7 s against 2.1 s on requests. It also did not run a planted `.venv/bin/python`. But an answer of `ty`
    depends on what it was asked before, reported as
    [astral-sh/ty#4915](https://github.com/astral-sh/ty/issues/4915). `error.error_list` at `forms.py:296` in django
    gets its two targets when asked first, and none after one question about `base.py:76`. It repeats on a clean
    clone with nothing installed, and on 0.0.80, 0.0.90, 0.0.95 and 0.0.96. `pyrefly` and `pyright` answer the same
    in both orders. **When that is fixed `pyrefly` is to be replaced by `ty`, by the owner's word,** once `ty` passes the same
    check of order on all of django. A server is one module here, `pyrefly.rs`.
13. **Whether Homebrew can provide `pyrefly`** is not checked. Its releases have 22 files, under the MIT license.

## 9. Order

1. `latt-resolve-lsp`, from what was `latt-resolve-tsrs`, with no change in behaviour. Done.
2. `scip_compare.py`, and `scip-python` on flask and requests. Done. Django is open, PY2.
3. `latt-parse-python`, with syntax rules only. Done.
4. `pyrefly.rs`, with the fixture of PY1, after `ty.rs` failed the check of order. Done.
5. Saved answers and edits, with PY3, and what `sync` says of missing packages. Done.
6. PY5, and the README and the Homebrew text. Done. PY4 is open.
