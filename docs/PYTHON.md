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
  mimir-parse-python ── symbols and references, through tree-sitter
  mimir-resolve      ── the same rules and the same port
  mimir-resolve-lsp  ── asking a language server about each site, and saving the answers;
                        `tsrs.rs` and `ty.rs` say what is each server's own
  mimir-lsp          ── starting a language server, its messages, file URIs
```

| Crate | Layer | Owns |
|---|---|---|
| `mimir-parse-python` | domain | Python through `tree-sitter-python` |
| `mimir-resolve-lsp` | adapter | the state, the saved answers, which files to ask about after a change, stopping when idle; the module `ty.rs` has the program, its version, its project files, its language id |

- **Nothing is copied.** A server is a module of a few lines in `mimir-resolve-lsp`, and its tests are a
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

**`ty` marks a field with its object,** as `self.x`, so a target's name is what follows the last dot.

**The port stays**, and the answers are entries of `LiveDocument`, as for TypeScript. Linking uses the
same `resolve_answers`, with what differs below given by the language.

**Asking.** Only the sites the parser found, with `textDocument/definition`, by a background task after
the graph answers from syntax rules. One file is open at a time. Batches of 32. Byte columns: `ty` offers
`utf-8`.

**Only `definition`.** Where `tsrs` needs `typeDefinition` and `implementation`, `ty` gives every member
of a union from `definition` alone, also with no name on the receiver (section 7). So each site costs one
question.

- **Several targets inside the project** give one edge each, marked possible, unless they are the
  declarations of one symbol: overloads, as above, and a getter with its setter, as below.
- **A variable and what it holds.** `alias()` after `alias = plain` answers both. The function is the
  call, and a variable gives no edge.
- **A target on an import** is asked again, as for TypeScript. `ty` went through `__all__` and a
  re-export at once where this was tried, so this rule is expected to do nothing, and costs nothing.
- **A target outside the project** gives no edge. **No answer** gives no edge, and `sync` reports the
  number. Calls are never guessed.

**Property.** `ty` answers with both the getter and the setter. A read keeps the one without
`@x.setter`, and a write the one with it, by the decorators in the source. This is the `accessor` of
`script.rs`, with one form for each language.

**What may run instead.** `c.area()` on a `Circle` is a `Calls` edge to `Circle.area`. A subclass that
overrides it is a possible target through `Implements`, as with a trait.

**Saved answers.** As for TypeScript. What changes all answers: `pyproject.toml`, `ty.toml`, `setup.py`,
`setup.cfg`, `requirements*.txt`, `uv.lock`, `poetry.lock`, `pdm.lock`, `Pipfile.lock`, `.python-version`.

**An edit.** As for TypeScript: first the edited file, then each file that imports it, directly or not.
`ty` sees a file changed on disk through `workspace/didChangeWatchedFiles` (section 7). Finding importers
needs Python's own rules for a path: `from . import a`, `from ..a import b`, `import a.b`, a package's
`__init__.py`, and a `src/` layout. That is one more thing the language gives.

**Syntax rules**, until answers come or without `ty`: a call links only to a function in the same file
with nothing nearer of that name, or to a name `from a import b` takes from a project file with exactly
one public symbol of that name. No methods, no `import *`.

**`ty` itself.**

- It is found on `PATH`, never in the project's environment, because a repository must not choose what
  the daemon runs.
- One exact version is the tested one, a constant in `mimir-resolve-lsp/src/ty.rs`, and `scripts/check-deps.py`
  compares it with the newest on PyPI (rule 10). Another version still runs, and the daemon's log says so.
- Without it only syntax rules apply, and `sync` reports how many files that covers.
- It stops after 10 idle minutes, like the others. It is never linked.
- Install: `uv tool install ty`, `pip install ty`, or a binary from its releases. None needs Node.

**The project's packages.** `ty` finds `.venv` at the top of a project by itself. Mimir installs nothing. Without the packages less is answered, and `sync` says so.

## 5. Targets

Added to section 2 of the architecture when they hold, each checked by `scripts/bench`.

| # | Target |
|---|---|
| PY1 | Every entry of the hand-written list in `crates/mimir-resolve-lsp/tests/ty/main.rs` holds, covering each row of the table in section 3 |
| PY2 | Against a SCIP index by `scip-python`: every disagreement falls in a named class of section 7; unclassified 0 |
| PY3 | After edits, the graph equals the graph from a clean start with no saved answers: 0 differences |
| PY4 | T1, T2, T3 and T4 hold on a Python project of 20 000 symbols |
| PY5 | All sites of a project of 20 000 symbols are answered in under 60 s, on a current laptop |

Where they stand, measured through `mimir` on an M1 Pro:

| | State |
|---|---|
| PY1 | Holds |
| PY2 | Holds on flask, requests and django: unclassified 0 |
| PY3 | Not met on django: 1 difference in 116 546 edges, 3 files edited, the same each time, from `ty` itself (section 8). The fixture for a changed type holds |
| PY4 | Not measured as `scripts/bench` does. T4 below: 598 MB on django, of 67 865 symbols |
| PY5 | Holds: django, 67 865 symbols, 46 s |

| | flask | requests | django |
|---|---|---|---|
| Symbols | 2 046 | 1 114 | 67 865 |
| Calls | 1 184 | 955 | 63 420 |
| Uses of fields | 408 | 644 | 15 921 |
| First `sync` | 2.1 s | 0.9 s | 46 s |
| Restart with nothing changed, without starting `ty` | 0.1 s | 0.1 s | 12 s |
| Daemon after asking | 32 MB | 28 MB | 598 MB |
| Daemon after that restart | 30 MB | 25 MB | 494 MB |
| `ty` while it ran | 179 MB | 152 MB | 1.1 GB |
| Saved answers | 103 KB | 94 KB | 5.7 MB |

A restart gives the same graph as a clean start, 0 differences on all three. An edit of three files most of
django imports took 35 s, about as long as a clean start of 32 s: they make nearly every file stale.

## 6. Machine checks

- `scripts/check-architecture.sh`: the new crates in `allowed`; `mimir-engine` depends on no parser of a
  language; `mimir-parse-python` is in the pure domain list.
- `scripts/check-deps.py`: the `ty` version, from PyPI.
- `cargo test`: the fixture of PY1, skipped with a message when `ty` is not installed. CI installs it.
- `scripts/bench`: PY2 to PY5. `scip_compare.py` becomes `scip_compare.py`, with the server as an argument.

## 7. What was measured

`ty` 0.0.96, `pyrefly` 1.4.0 and `pyright` 1.1.414, 2026-11-03, on an M1 Pro. Packages were installed from
wheels, so none of their code ran. A site is each call in the source, found with Python's `ast`; one file
open at a time, batches of 32. **The percentages are how many sites got an answer, not how many answers
are right.** Against an index, below.

| With packages | flask | requests | django |
|---|---|---|---|
| Files | 83 | 37 | 2 943 |
| Sites | 3 904 | 2 684 | 190 543 |
| Imports left unresolved, by `pyright` | 12 | 6 | 50 |
| `ty`, answered | 73.7% | 93.5% | 78.5% |
| `ty`, code / tests | 95.6% / 62.1% | 95.9% / 92.1% | 81.7% / 77.8% |
| `ty`, time and memory | 1.5 s, 174 MB | 0.7 s, 154 MB | 34 s, 780 MB |
| `pyrefly`, answered | 71.8% | 95.1% | 78.9% |
| `pyrefly`, code / tests | 89.9% / 62.2% | 95.5% / 94.9% | 83.1% / 77.8% |
| `pyrefly`, time and memory | 1.6 s, 206 MB | 2.1 s, 169 MB | 51 s, 619 MB |
| `pyright`, answered | 74.5% | 93.4% | not run |
| `pyright`, time and memory | 25 s, 928 MB | 33 s, 845 MB | not run |

Without its packages, an earlier `pyright` run gave flask 61.2% and requests 86.5%. The imports left on
django are mostly optional drivers, as `psycopg2` and `MySQLdb`, that were not installed.

**`ty` against `scip-python` 0.6.8,** with `scripts/bench/scip_compare.py`. A site is any place the index
reports a reference, so more than the calls above.

| | flask | requests | django |
|---|---|---|---|
| Sites | 15 824 | 12 624 | 613 214 |
| Inside, agree | 8 162 of 8 214 | 6 514 of 6 559 | 459 650 of 477 676 |
| Index has a target, `ty` none | 3 | 6 | 3 509 |
| Index has none, `ty` does | 1 346 | 2 100 | 3 549 |
| The index: time, memory | 6 s, 530 MB | 7 s, 464 MB | 69 s, 4.5 GB |

Of the disagreements on django, by what `scip_compare.py` read at both targets:

| Class | Sites | Meaning |
|---|---|---|
| Module position | 7 745 | File start against some definition: the index's answer for a name that is a module |
| Server on the name, index elsewhere | 5 476 | In the samples read the index is wrong: `models.CharField` at `class DateTimeField` |
| Same name, another declaration | 468 | Either may be right: `self.client.get` at `Client.get`, the index at a subclass set in `client_class`; a class field against `self.x` |
| Constructor | 437 | The class against `__init__`, which section 4 makes one |
| Alias followed | 285 | `from .a import __all__ as a_all` and the like |
| Reached by asking again | 52 | Section 4 |
| Metaclass call | 5 | `models.TextChoices("Medal", "GOLD")`: `ty` gives `EnumMeta.__call__`, outside the project |
| Unclassified | 0 | |

Of the 3 509 where only the index answers, 1 841 are calls to a function and 1 668 fields or keyword names.
So against this index `ty` loses 3.1% of calls to a function on django: 1 841 of 58 950. `scip-python` is
built on `pyright`, which also works out types from a call site, and `ty` does not do that; by samples read,
these are as `cursor.execute(...)`. A class says what a disagreement looks like, not which side is right, and
this index is no truth: it was wrong in every sample of its second-largest class.

**`ty` against `pyrefly`, on django.** Both answered 148 351 sites, and 87 605 share a target by file and
line. Of the other 60 746: 28 633 are the class against its `__init__`, which section 4 makes one; 29 944
have both targets outside the project, which give no edge; 1 963 have one inside and one outside; and 206
differ inside the project, in the samples read mostly `pyrefly` stopping on an import line. So they differ
where it matters at about 1.5%. Agreement is not proof.

**What is left unanswered in django's code,** by samples read, not counted: values without a type, as
`value.strip()` on a parameter with no annotation.

**The answers of `ty`,** on a list written by hand:

| In the source | `definition` gives |
|---|---|
| `u.area()` on `u: Circle \| Square` | both methods |
| `make().area()`, returning a union | both methods |
| `(c if flag else Square()).area()` | both methods |
| `[c][0].area()` | `Circle.area` |
| `s.area()` on a `Protocol` | the protocol's method; `implementation` finds no class that fits it |
| `Circle(1)`, `Point(1)` on a dataclass | the class |
| `over(1)` | each overload and the implementation |
| `alias()` after `alias = plain` | `plain` and the variable |
| `again()`, re-exported through `__all__` | `plain`, at its definition |
| `lib.plain()`, `Circle.make()`, `Circle.helper()`, `decorated()` | the definition |
| `c.size`, `c.size = 2` | getter and setter, in both |
| `getattr(c, "area")()` | the name `getattr`, outside the project; the call it returns has no site |

**Edits.** With `didChange`, on the same site, the answer moved by the line added. After 3 lines were
added on disk to another file and `didChangeWatchedFiles` sent, the answer moved by 3, and back when they
were removed, in about 1 ms on the small list. On django's `query.py` a first answer after an edit took
41 ms, but at a place with no target, so it says little.

**Running the project.** `ty` did not run a `.venv/bin/python` put in a project to leave a mark, in one try.
It found a project's `.venv` without `VIRTUAL_ENV`: flask gave 73.7% either way.

## 8. Open

1. **Correctness is agreement with another checker, which errs too.** About 6 400 sites on django differ with
   no plain answer who is right, or `ty` has none, and they were only sampled.
2. **Values without a type are left out:** about a fifth of the calls in django's own code. `[possible]`
   targets by name, kept apart from `Calls`, are an idea and are not specified. It needs its own measurement
   of how often they are right.
3. **Structural types.** A class that fits a `Protocol` without naming it gives no `Implements`.
4. **Calls the source does not spell**, as in section 3, and anything through `getattr`, or a dynamic
   decorator that returns another function.
5. **A package installed or removed** without a change to a file of section 4 leaves saved answers stale.
6. **Other environments:** a virtual environment elsewhere, conda, and `VIRTUAL_ENV` of another project in
   the shell that started the daemon, are unmeasured. Also unmeasured: uv workspaces, namespace packages,
   and a module with both `.py` and `.pyi`, where a target may land in the stub.
7. **Strings as annotations**, as `-> "Circle"`, have not been tried as sites.
8. **What `sync` says of missing packages** looks only for `.venv` beside a list of packages. A `VIRTUAL_ENV`
   or an environment elsewhere is not seen, so the note words it as a condition. Nothing checks that the
   environment holds what is listed.
9. **PY4** is not measured as `scripts/bench` does. On django the daemon holds 598 MB, for three times the
   symbols of T4, and a new daemon takes 12 s with nothing changed.
10. **Results depend on a 0.0.x beta,** whose answers may change between versions. Only 0.0.96 was tried.
11. **An answer of `ty` can depend on what it was asked before.** `error.error_list` at `forms.py:296` in django
    gets its two targets when asked first, and none when asked after the files before it in a clean start. Three
    clean starts gave the same graph, and so did two runs of edits, so it holds for one order of asking, and this
    is PY3's one difference. How many answers it costs a clean start is not known.
12. **Whether Homebrew can provide `ty`** is not checked. Its releases have binaries, under the MIT license.

## 9. Order

1. `mimir-resolve-lsp`, from what was `mimir-resolve-tsrs`, with no change in behaviour: TS1 and TS3 still hold, and the graphs of hono, nest and three.js did not change. Done.
2. `scip_compare.py`, and `scip-python` on flask, requests and django: PY2. Done.
3. `mimir-parse-python`, with syntax rules only. Done.
4. `ty.rs`, with the fixture of PY1. Done.
5. Saved answers and edits, with PY3, and what `sync` says of missing packages. Done.
6. PY5, and the README and the Homebrew text. Done. PY4 is open.
