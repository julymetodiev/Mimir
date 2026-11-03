# JavaScript and TypeScript

The specification for section 8 of [ARCHITECTURE.md](ARCHITECTURE.md). The rules, layers and targets
there hold here too. **Status: built.** Section 8 says what is open.

## 1. Scope

Files: `.ts`, `.tsx`, `.mts`, `.cts`, `.js`, `.jsx`, `.mjs`, `.cjs`. A `.d.ts` in the project gives
symbols. Whatever git ignores is left out, so `node_modules` gives targets and no symbols.

Left out: `.vue`, `.svelte`, `.astro`, code inside HTML, and any other file that is not one of those.

## 2. The parts

```
  mimir-parse-ts ── symbols and references, through tree-sitter
  mimir-resolve  ── the same rules and the same port
  mimir-resolve-lsp ── the port, over a running language server such as `tsrs --lsp -stdio`
  mimir-lsp      ── starting a language server, its messages, file URIs
```

| Crate | Layer | Owns |
|---|---|---|
| `mimir-parse-ts` | domain | TypeScript and JavaScript through `tree-sitter-typescript` and `tree-sitter-javascript` |
| `mimir-resolve-lsp` | adapter | the running server, the saved answers, which sites to ask about; what is `tsrs`'s own is the module `tsrs.rs`; Python uses it too |
| `mimir-lsp` | adapter | what `mimir-resolve-ra/src/live/lsp.rs` holds that no one language needs; both adapters use it (rule 9) |

- **More than one source.** `SemanticSources`, in `mimir-resolve`, holds a list and is itself a
  `SemanticSource`: each one picks the files of its language, and they work at the same time.
  `commands/daemon.rs` stays the only place that names the sources.
- **More than one parser.** The engine gets its list of adapters from `commands/daemon.rs`, so it names
  no language.
- **The source is `tsrs`.** `scip-typescript` is used only to measure (section 7): Mimir does not run
  it and does not need Node.

## 3. Symbols and references

Symbols: function, arrow function or function expression bound to a name, a function assigned to a
property (`app.use = function () {}`, `exports.run = ...`, `Foo.prototype.run = ...`), or held in the
object of a top-level constant, class, method, constructor, getter, setter, field, interface, type
alias, enum and its members, namespace, top-level constant and variable. Overloads and their
implementation are one symbol, at the implementation; with none, at the first signature.

| In the source | Reference | Edge |
|---|---|---|
| `f()`, `a.f()`, `` f`...` `` | `Call` | `Calls` |
| `new X()` | `Call` | `Calls`: the class's constructor if it declares one, otherwise the class |
| `a.x` where `x` is a getter or setter | `FieldUse`, `FieldWrite` | `Calls`: the getter for a read, the setter for a write, both for `a.x += 1` |
| `@dec` | `Call` | `Calls` |
| `f` named without a call, `<Foo />` | `FunctionValue` | `UsesAsValue` |
| `import`, `export ... from`, `require('...')` | `Import` | none: a file is no symbol, and calls show what is used |
| a type named | `TypeUse` | `UsesType` |
| `a.x` on a field | `FieldUse`, `FieldWrite` | `UsesField` |
| a constant read | `FunctionValue`: what it names decides | `UsesConstant` |
| `class A implements B`, `class A extends B` | as `impl Tr for T` | `Implements`: a possible target |

A call through an interface or abstract method is a `Calls` edge to that method; what implements it
is a possible target, as with a trait. A caller is the innermost named function around the site. An
unnamed function belongs to the one around it. Code outside any function belongs to the constant that
holds it, or else to `<module>`, a symbol every file has.

A name a function binds, as a parameter or a local, is no `FunctionValue`: it names no function of its
own. A call through it is still asked about.

## 4. Resolution

**The port stays.** An index holds no SCIP documents for these files. Each file is an entry of
`LiveDocument`, the form the running `rust-analyzer` already answers in, and linking needs nothing new
for it. `ScipIndex.live` no longer means edited only, so `ScipIndex` and `live` get names that say
so in the same change.

**Asking.** Only the sites the parser found are asked about, with `textDocument/definition`, by a
background task after the graph answers from syntax rules. A query meanwhile is marked partial, as now.
One file is open at a time, opened to ask about it and closed after, always, also after an edit: what is
open can change an answer (section 7).

- **A target on an import** is asked again from there, until it stops moving or after 8 steps, when
  the site is left out and counted. A shorthand property, as in `export default { memo }`, gets the
  import from `tsrs`, and the edge must go to the definition. Asking again reached it at once in all
  four cases tried. Of several targets, the one on an import is the one followed.
- **A target outside the project** gives no edge.
- **No answer** gives no edge, and `sync` reports the number. This is `obj[name]()`, and anything of
  type `any`. Calls are never guessed.
- **Several targets inside the project** give one edge each, marked possible unless they are the
  declarations of one symbol.

**Saved answers.** Beside the graph, by a hash of each file, of the `tsconfig` and `jsconfig` files and
of the lockfiles. A changed `tsconfig` or lockfile restarts `tsrs` and asks everything again.

**An edit.** A target is a place in another file, which an edit moves, and a changed type can change
what `x.y()` means in a file that does not import the edited one directly. So the edited file is asked
first, which is the first answer of T2. Then every file that imports it, directly or not, marked
partial until done. `tsrs` is told of a change with `didChange`, whoever made it.

**Syntax rules**, until answers come or without `tsrs`: a call links only to a function in the same
file with nothing nearer of that name, or to a name imported by a relative path from a file that
exports exactly one symbol of that name. Nothing else: no methods. Each family of languages is linked by
itself, so a name in TypeScript never lands on a name in Rust.

**`tsrs` itself.**

- It is found on `PATH`, never in the project's `node_modules`, because a repository must not choose
  what the daemon runs.
- One exact version is the tested one, a constant in `mimir-resolve-lsp/src/tsrs.rs`, and `scripts/check-deps.py`
  compares it with the newest on npm (rule 10). Another version still runs, and the daemon's log says so.
- Without it only syntax rules apply, and `sync` reports how many files that covers. A `package.json` without
  `node_modules` gets a note from `sync`, since less is answered.
- It stops after 10 idle minutes, like the analyzer.
- It is a separate process and is never linked: its crates are unpublished, with no stable Rust API.

## 5. Targets

Added to section 2 of the architecture when they hold, each checked by `scripts/bench`.

| # | Target |
|---|---|
| TS1 | Every entry of the hand-written list in `crates/mimir-resolve-lsp/tests/tsrs/main.rs` holds, covering each row of the table in section 3 |
| TS2 | Against `scip-typescript`: every disagreement falls in a named class of section 7; unclassified 0 |
| TS3 | After edits, the graph equals the graph from a clean start with no saved answers: 0 differences |
| TS4 | T1, T2, T3 and T4 hold on a TypeScript project of 20 000 symbols |
| TS5 | All sites of a project of 20 000 symbols are answered in under 60 s, on a current laptop |

Where they stand, measured through `mimir` on an M1 Pro:

| | State |
|---|---|
| TS1 | Holds |
| TS2 | Holds on hono, nest and three.js: unclassified 0. Not repeated on VS Code, which had 368 disagreements before these classes |
| TS3 | Holds on hono and nest, 0 differences in every kind of edge. Comment edits, and the fixture for a changed type |
| TS4 | Not measured as `scripts/bench` does. T4 below |
| TS5 | Holds on three projects: hono 12 497 symbols 11 s, nest 27 854 symbols 48 s, three.js 27 387 symbols 45 s |

| Daemon | hono | nest | three.js |
|---|---|---|---|
| After asking | 121 MB | 187 MB | 373 MB |
| After a restart with nothing changed | 106 MB | 206 MB | 339 MB |
| That restart takes, without starting `tsrs` | 1.0 s | 2.9 s | 4.2 s |
| Saved answers | 1.0 MB | 2.3 MB | 4.9 MB |

Those are with `tsrs` 0.21.0, and every edge of the three graphs is the same as with 0.20.1. Between runs the daemon moved by up to 45 MB, up to 426 MB on three.js. `tsrs` itself used 0.6 to 2.5 GB while it ran. An edit near the bottom of the imports re-asks most of a project: 30 s on nest for `packages/common/index.ts`.

## 6. Machine checks

- `scripts/check-architecture.sh`: the new crates in `allowed`; `mimir-engine` depends on no parser of
  a language; `mimir-parse-ts` is in the pure domain list.
- `scripts/check-deps.py`: the `tsrs` version, as in section 4.
- `cargo test`: the fixture of TS1, skipped with a message when `tsrs` is not installed, as the
  `rust-analyzer` tests are.
- `scripts/bench`: TS2 to TS5, with `scip_compare.py` for TS2.

## 7. What was measured

`tsrs` 0.20.1 against `scip-typescript` 0.6.1, 2026-11-03, on an M1 Pro, with `scripts/bench/scip_compare.py`.
A site is a place the index reports a reference; its target is compared by file and line, or by package
and file outside the project. Packages were installed with `--ignore-scripts`, except for VS Code and
three.js, so targets in their libraries are not covered. The index for express comes from a copy without packages, because the
indexer found no files in the copy with them.

| | express, `lib/` | three.js, `src/` | hono, with tests | nest, with packages | VS Code, `vs/base/common` |
|---|---|---|---|---|---|
| Kind | JS, no config | JS, no config | TS | TS, monorepo | TS |
| Files asked about | 6 | 758 | 315 | 1 203 | 158 |
| Sites | 1 373 | 115 282 | 108 351 | 72 776 | 50 919 |
| Inside, agree | 907 of 1 087 | 91 559 of 108 715 | 70 998 of 72 557 | 57 401 of 58 894 | 44 471 of 44 885 |
| Inside, `tsrs` has no answer | 3 | 64 | 0 | 0 | 0 |
| Outside, same package and file | 214 of 286 | 6 385 of 6 567 | 35 723 of 35 794 | 12 342 of 13 882 | not compared |
| Outside, other package | 0 | 21 | 66 | 35 | not compared |
| Call-like sites answered | 62.0% of 469 | 66.2% of 29 126 | 90.8% of 37 982 | 93.7% of 19 217 | not compared |
| `scip-typescript`, its scope | 1.3 s, 306 MB | 37 s, 2.2 GB | 7 s, 945 MB | 7 s, 838 MB | 244 s, 7.8 GB, 16 GB heap needed |

A call-like site is a name followed by `(`, which also catches text in strings and shader code, so the
percentages are a lower bound. Where `tsrs` has no answer the index mostly has none either: the no-answer
row is small beside them.

**Opening.** One file open at a time is better than all files open, in all but one measure:

| | nest | three.js | hono, with tests |
|---|---|---|---|
| First answer, all open | 17.5 s | 12.7 s | 0.49 s |
| First answer, one at a time | 0.13 s | 0.03 s | 0.06 s |
| Definitions a second, all open | 3 323 | 3 490 | 3 352 |
| Definitions a second, one at a time | 4 775 | 6 899 | 7 018 |
| Memory, all open | 1.5 GB | 1.1 GB | 665 MB |
| Memory, one at a time | 1.05 GB | 483 MB | 1.07 GB |

**Same answers?** Places compared include every identifier. One at a time on nest, 176 968 places: run
twice, or in reverse order, no target inside the project changes. All open against one at a time changes
1 place on nest, 5 on three.js and 6 on hono, so the answer can depend on what is open, and section 4
fixes one way of asking. All open in reverse order: no target inside the project changes on hono and nest
with packages; without packages, 149 places changed on nest.

Classes of disagreement, by what was read:

| Class | hono with tests | Meaning |
|---|---|---|
| Index has no definition, `tsrs` does | 1 229 | Imported names the index gives as locals. `tsrs` is more complete |
| Module position | 242 | File start against the first line. No effect: `Imports` goes to the file |
| Reached by asking again | 74 of 84 | Shorthand properties and the like, section 4 |
| Index points at an augmentation | 2 | `tsrs` gives the declaration |
| Union type | 9 | `tsrs` gives one member, from the standard library. In 4 of them the project's own method is lost |
| Global declared twice | 26 | `URL` in `esbuild` against `lib.dom`. Both outside |
| Locals of a package | 40 | The index has no package to compare. Not a fault of either |

What still differs after asking again, as `scip_compare.py` sorts it by the source at both targets:

| Class | hono | nest | three.js | Meaning |
|---|---|---|---|---|
| Module position | 242 | 1 101 | 1 148 | As above |
| Reached by asking again | 74 | 0 | 66 | As above |
| Same name, another declaration | 10 | 114 | 105 | Members of a union, a `var` declared twice, a field and its setter |
| Constructor | 0 | 24 | 5 | `new X` at the class, where the index gives the constructor |
| CommonJS exports | 0 | 0 | 125 | `exports.x`: the index gives what is assigned |
| Alias followed | 0 | 0 | 28 | `var get = a.b.get`: `tsrs` goes on to the function, the index stops at the variable |
| The server on the name, index elsewhere | 0 | 0 | 6 | `Foo.prototype = {`: the index gives the function |
| Unclassified | 0 | 0 | 0 | |

A class says what a disagreement looks like, not which side is right. "Same name, another declaration"
is the widest, and only samples of it were read: one was `camera.isArrayCamera ? _a : _b`, a union, which
section 4 asks about in another way.

`tsrs` also resolves `#private` members, which the index does not report. On nest, 1 505 outside targets
differ only by file within a package: `tsrs` follows declaration maps to `rxjs/src/*.ts`, and the index
stops at `.d.ts`. Outside targets give no edge, so this changes nothing.

## 8. Open

1. **Union types behind a call.** `tsrs` gives one member for `x.area()` on `Circle | Square`, and
   TypeScript 7.0.2 does the same. Mimir asks `typeDefinition` on the receiver and, with several types,
   `implementation`, so each member is a possible target. At `make().area()` the type is asked for at
   `make`. `list[0].area()` and `(a ?? b).area()` have no name to ask about, so there one member stays,
   as a certain call: a known limit.
2. **Plain JavaScript gets about two thirds of its calls.** The rest is left out: `this.type()` on a
   prototype, and values of unknown type. This is the rule of not guessing, and the README must say so.
3. **TS2 was not repeated on VS Code**, whose copy is gone, and its classes do not say who is right.
4. **pnpm and yarn workspaces** are unmeasured: nest uses npm, and the index covered 3 of its 153
   `tsconfig` files. `tsrs` was asked about all files.
5. **All sites of a very large project.** Estimated, not measured: about 15 minutes for all of VS Code if
   every reference were asked about. The parser's sites are fewer. TS5 decides.
6. **An edit near the bottom of the imports** re-asks most of a project: 30 s on nest. Until it ends those files give syntax answers, marked partial.
7. **Memory after asking** is 373 to 426 MB on three.js, of 27 387 symbols, against T4's 400 MB at 20 000.
8. **Accuracy on real projects** is known only as agreement with an index; TS1 is a small hand-written list.
9. **How `tsrs` is installed.** The npm package needs Node for its launcher; the native binary is inside
   a platform package. Whether Homebrew or a release can provide it is not checked.
10. **Results depend on a one-month-old beta**, which takes no pull requests.
11. **Only the union case was compared with TypeScript 7 itself.** `scip-typescript` runs TypeScript 6
    and `tsrs` follows 7.1 beta, so other disagreements may come from either the version or the port.
12. **Unexplained:** memory on hono is higher one file at a time, and `scip-typescript --infer-tsconfig`
    found no files in express with its packages installed.

## 9. Order

1. Move the comparison script to `scripts/bench`, and measure plain JavaScript, a monorepo and a project
   with many libraries. Done.
2. `mimir-lsp`, taken out of `mimir-resolve-ra`, with no change in behaviour: T5 and T6 still hold. Done.
3. `SemanticSources` and the adapter list, still only Rust. Done.
4. `mimir-parse-ts`, with syntax rules only. Done.
5. The source over `tsrs`, with the fixture of TS1. Done.
6. Saved answers and edits, with TS3. Done.
7. README and the MCP instructions. Done. The targets move to the architecture once TS4 is measured.
