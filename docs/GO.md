# Go

The specification for Go in section 8 of [ARCHITECTURE.md](ARCHITECTURE.md). The rules, layers and targets
there hold here too. **Status: specified, not built.** Section 8 says what is open.

Go works as Rust does, and not as TypeScript and Python do: an index of the whole project, and a running server
only for the files edited since. Section 7 has the numbers that decided this, by the owner's word.

## 1. Scope

Files: `.go`, test files included. Whatever git ignores is left out.

Also left out, because the Go tool does not build them: `vendor/`, `testdata/`, and a directory whose name starts
with `_` or `.`. A target in `vendor/` is outside the project.

Left out: C and assembly beside Go, code inside strings, and templates. Generated Go in the repository is Go like any other.

## 2. The parts

```
  mimir-parse-go      ── symbols and references, through tree-sitter
  mimir-resolve       ── the same rules and the same port, with implementations and several indexes
  mimir-resolve-scip  ── reading a SCIP index, keeping it by a hash of the sources, which files to ask
                         about after an edit: what is now in mimir-resolve-ra
  mimir-resolve-ra    ── what is Rust's own: Cargo roots, failed macros, `rust-analyzer`
  mimir-resolve-go    ── what is Go's own: `go.mod` roots, `scip-go`, `gopls`
  mimir-lsp           ── starting a language server, its messages, file URIs
```

| Crate | Layer | Owns |
|---|---|---|
| `mimir-parse-go` | domain | Go through `tree-sitter-go` |
| `mimir-resolve-scip` | adapter | `decode.rs`, the snapshot of what an index was made from, the choice between kept index, running server and new index |
| `mimir-resolve-go` | adapter | finding roots, running `scip-go`, asking `gopls`, the versions tested |

- **Nothing is copied.** `decode.rs` and the flow of `load_root` move from `mimir-resolve-ra` to `mimir-resolve-scip`
  with no change in what Rust does, and each language gives what is its own.
- **`commands/daemon.rs`** stays the only place that names the sources and the parsers.
- **`Language::Go`** already exists, and is a family of its own: linking never crosses to another.

## 3. Symbols and references

Symbols: function, also `init` and `main`, of which a package may have several in its files; method, by its
receiver; method of an interface; struct, interface, any other named type, and alias; field; constant and
variable at package level; a function bound to such a variable. A qualified name is the package's directory,
then the type if any, then the name: `inner::Base::Hello`. Generic functions and types are one symbol, whatever
their arguments.

| In the source | Reference | Edge |
|---|---|---|
| `f()`, `pkg.F()`, `x.M()`, `go f()`, `defer f()`, `f[int]()` | `Call` | `Calls` |
| `x.M()` on an interface | `Call` | `Calls` the interface's method; what implements it through `Implements` |
| `x.M()` through an embedded field | `Call` | `Calls` the method of the embedded type |
| `T(x)` | `Call` | `UsesType`: what the target is decides, a conversion is no call |
| `f`, `x.M`, `T.M` named without a call | `FunctionValue` | `UsesAsValue` |
| `import "a"` | `Import` | none |
| a type named anywhere, `T{}` | `TypeUse` | `UsesType` |
| `x.f` on a field | `FieldUse`, `FieldWrite` | `UsesField` |
| a constant or variable at package level read | `FunctionValue`: what it names decides | `UsesConstant` |
| a method that fits an interface | none in the source | `Implements`: a possible target, from the index or from `implementation` |

A caller is the innermost named function around the site. An unnamed function belongs to the one around it. Code
at package level belongs to the variable it initialises. A name a function binds is no `FunctionValue`, and
calling such a name gives no `Calls`: `f := helper; f()` is `helper` as a value, and no call.

A call the source does not spell gives no reference: what an operator runs, `String()` through `fmt`, and whatever
goes through `reflect`.

## 4. Resolution

**The index.** Two steps in a root write it, in this order:

1. `go list -e -export -test ./...`, with the network. It fetches the modules missing, compiles, and says what did not
   compile, which `scip-go` does not. A package with `Incomplete` is reported by `sync` and `status`, with the reason from its
   `Error` or `DepsErrors`. What resolved in it stays. Without `-export` an undefined name is not seen, and without `-test`
   nothing in a test file is.
2. `scip-go --output FILE`, with no network: `HTTPS_PROXY` and `HTTP_PROXY` name a local port that Mimir took and let go, so
   nothing listens there, and `NO_PROXY` and `no_proxy` are unset.

The second is a way around a fault in `scip-go`, and goes when that is fixed. For each package of another module of a `go.work`,
it asks that module's host over HTTP for the repository, twice and with no memory of the answer (`hasSameRepoRoot` in
`internal/loader/loader.go`), only to choose the version in a symbol, which Mimir drops. So it waits on the network, and it sends
the names of private modules out. Section 7 has the cost. Go itself has no network in this step either, which is why the first
comes before it.

An index is kept as in section 5 of the architecture: by a hash of the sources, and written again when a manifest changes, when
more than 50 files are edited, or by `sync --full`. Manifests: `go.mod`, `go.sum`, `go.work`, `go.work.sum`.

- **A root is a directory with a `go.mod`,** in a directory section 1 keeps. Every one has an index of its own. An index at a
  `go.work` beside no `go.mod` is empty, so `go.work` gives no root. Nested modules are roots too, and their number has no limit.
- **One root at a time, the fewest files first.** Each takes all cores, and later ones reuse what earlier ones compiled. An index
  enters the graph when it ends, and a root without one has syntax rules. Each root has 600 s for the two steps. One that takes
  longer keeps syntax rules, `sync` says so, and the others go on.
- **Occurrences** in `scip-go` have their place in `Occurrence.single_line_range = 8`, which the decoder learns to
  read, beside `range = 1`.
- **Columns** are bytes, as `Document.position_encoding = 6` says, the same as for Rust.
- **A document outside the root,** as what `cgo` generates in the build cache, has a path with `..`. It is no project
  file, and so it is ignored as it is today.
- **Implementations.** `Document.symbols = 3` holds `SymbolInformation.symbol = 1` and `relationships = 4`, with
  `Relationship.symbol = 1` and `is_implementation = 3`. The port gains these pairs. For two methods they give an edge
  `Implements`. A pair of types gives none, since it would add nothing to the methods.
- **What is a function.** An interface's method ends in `.`, as `Area#Area.`, the same as a field, where
  `is_function` looks for `().`. So what such a symbol is comes from the symbol of the graph at its definition. Rust does
  not change.
- **Across modules.** The same symbol in another module's index has another version: `example.com/a 1b5c3cabf341` where it
  is defined, `example.com/a .` where it is used. Definitions are looked up across all indexes of the family, by the symbol
  without its version. Today they are per index, in `semantic::resolve`. Rust does not change, checked by T5 and T6.
- **An interface's method across modules** ends otherwise at a use than at its definition: `Object#GetName().` in the other
  module, `Object#GetName.` where it is defined. So a symbol with no definition is looked up in its other form too. This cannot
  find a wrong target, since a type in Go has no field and method of one name.

**After an edit.** The index is kept, and `gopls` answers for the edited files, as `rust-analyzer` does. Positions in the
index for an edited file are old, and its definitions are found by name and order, as today.

- One `gopls` per root, started in it with `zeroConfig: false`. Otherwise opening a file for another system makes
  a view for that system and more memory is held.
- Only the sites the parser found, with `textDocument/definition`, in batches of 32. Columns go out and back in
  UTF-16, the only encoding it offers.
- For each method and each interface method in an edited file, `textDocument/implementation`. On a concrete method it gives
  the interfaces, on an interface's method what implements it. An edge is `Implements`, as in the index.
- **It may answer `no package metadata`.** Opening a file it has not read counts as a new file, and it drops what it knows
  of that package and every package that imports it until it has loaded them again (`cache/snapshot.go`, `oldFiles[uri]`
  may be nil). A file asked about meanwhile gets this error. So files with this error are asked once more after the others,
  5 s later. **An error is not an answer:** a file that still has it is outside the build, and keeps its syntax edges.
- It stops after 10 idle minutes.

**Outside the build.** A file behind a build tag or for another system, as `//go:build withdeploy` or `a_windows.go`, is in no
index and gets no answer. It keeps syntax edges, and `sync` reports how many such files there are, as for `cfg` in Rust.

**Syntax rules,** until answers come or without these tools: a call links to a function of the same package with exactly
one definition of that name, when nothing nearer has that name; `pkg.F()` likewise, when `pkg` is imported from a directory
of the project. No methods. Two definitions, as `f` in `a_linux.go` and `a_windows.go`, give no edge.

**The tools.**

- They are found on `PATH`, never in the project. `go` itself must be there too, as both run it.
- One exact version of each is the tested one, constants in `mimir-resolve-go`, and `scripts/check-deps.py` compares
  them with the newest, from `proxy.golang.org` (rule 10). Another version still runs, and the daemon's log says so.
- Without `scip-go`, or when it fails: `gopls` is not asked either, only syntax rules apply, and `sync` says why. Without
  `gopls`: the index stays, and edited files have syntax rules until the next index.
- Install, with Go: `go install github.com/scip-code/scip-go/cmd/scip-go@latest` and
  `go install golang.org/x/tools/gopls@latest`. They are never linked.

**What runs.** An index compiles the project and what it needs, through `go list -export`: the compiler, and for `cgo` the C
compiler, with what the source asks for as far as Go's list of allowed flags lets it. A `//go:generate` in a project was not run
(section 7). No test and no code of the project is run. Mimir does not set `GOTOOLCHAIN`, so as for any `go` command a
`go.mod` may make Go fetch the toolchain it names. See `SECURITY.md`.

**What goes out.** The first step of an index talks to whatever fetches modules for this machine, as any `go` command does. The
second sends nothing. `gopls` is started with the environment it would have anyway, and what it sends was not looked at (section 8).

**The first index** is a build of everything and takes minutes, once: `sync` shows that it is running, and the graph answers from
syntax rules until it ends, as with Rust. It fills Go's own build cache, as a build does.

## 5. Targets

Added to section 2 of the architecture when they hold, each checked by `scripts/bench`.

| # | Target |
|---|---|
| GO1 | Every entry of the hand-written list in `crates/mimir-resolve-go/tests/`, covering each row of the table in section 3, holds |
| GO2 | Against `gopls` over all sites of a project: every disagreement falls in a named class of section 7; unclassified 0 |
| GO3 | After edits, the graph equals the graph from a new index: 0 differences |
| GO4 | T1, T2, T3 and T4 hold on a Go project of 20 000 symbols, T3 with the build cache filled |
| GO5 | Across two modules of one `go.work`, a call has its edge |

Nothing is measured through `mimir` yet: section 7 is of the tools alone. GO2 holds on hugo by those numbers.

## 6. Machine checks

- `scripts/check-architecture.sh`: the new crates in `allowed`; `mimir-engine` depends on no parser of a language;
  `mimir-parse-go` is in the pure domain list.
- `scripts/check-deps.py`: the versions of `scip-go` and `gopls`, and `tree-sitter-go` as any crate.
- `cargo test`: the fixture of GO1, skipped with a message when the tools are not installed. CI installs them.
- `cargo test`: two modules of a `go.work`, indexed with a local server named as the proxy instead of the closed port. It must
  get no request, so a new `scip-go` that goes around the proxy fails here.
- `scripts/bench`: GO2 to GO5.

## 7. What was measured

`gopls` 0.24.0, `scip-go` 0.2.9, Go 1.27.1, 2026-11-04, on an M1 Pro with 32 GB, without `mimir`. hugo at `fa5a3d4`, 912 files;
cobra at `adbc881`, 36 files. A site is a name before `(`, or after a `.`, found by a regular expression outside comments and strings.

**Why an index.** All of hugo:

| | Time | Peak memory |
|---|---|---|
| `gopls`, each site, one file open at a time | 82 to 113 s | 5.6 GB |
| The same with `diagnosticsTrigger: Save` | 139 s, another run beside it | 4.3 GB |
| The same with `GOGC=50` | 147 s, another run beside it | 4.4 GB |
| `gopls` with no file opened, on 160 files | about 200 s for 17 512 sites | 1.3 GB |
| `scip-go`, build cache filled | 2.4 to 2.8 s | 1.1 GB |
| `scip-go`, build cache empty | about 310 s | 1.0 GB |

The index is 29 MB. An empty cache grew to 16 GB, in the run through the type checker that takes the same time. On kubernetes,
200 files at `gopls` took 42 s and 3.9 GB, so about 48 minutes for each order by that rate.

**A new index after an edit:**

| Edit | Time |
|---|---|
| None | 2.4 to 2.8 s |
| A comment at the end of a file in `tpl/strings` | 7.6 s |
| Lines moved in `common/hstrings`, which much imports, with the edits below | 293 s |

Why the last is as long as an empty cache was not worked out.

**The index against `gopls`** on all of hugo, 103 967 sites:

| | Sites |
|---|---|
| Same place | 50 369 |
| Target outside the project, same package | 36 911 |
| The definition itself | 8 829 |
| A built-in name or `unsafe`, which the index does not report | 6 528 |
| `gopls` has an error, the file is outside the build | 893 |
| File not in the index, outside the build (`deploy/`) | 391 |
| A package's name: the `import` line against the package | 40 |
| Not in the index | 4 |
| `gopls` empty, outside the build | 2 |

A target outside the project was compared by package and not by line, since an index does not hold where it is. The 4 are fields of
`godartsass.SassError` through a type switch, outside the project, so they give no edge. Why they are missing was not worked out.

**After edits,** with `gopls` running from before them: lines moved in two files, a new function, a call of it from another file, and
a new file. 275 sites in those 4 files, 0 differences with a new index, 0 errors, answered in about 0.5 s, 1.1 GB. This is of those
files alone: it is not GO3.

**Does the order of asking change an answer?** All of hugo through `gopls`, each file in order and in reverse, with the second try of
section 4: 103 967 sites, 0 differences. Without the second try 848 sites in 3 files had `no package metadata` in one order only;
two answers never differed. On cobra, 6 977 sites, 0 differences.

**The answers,** on a list written by hand:

| In the source | `scip-go` | `gopls` `definition`, then `implementation` |
|---|---|---|
| `a.Area()` on an interface | `Area#Area.` | the interface's method, then both methods |
| `c.Area()`, `s.Area()` | the method | not asked |
| `t.Area()` through an embedded field | `Circle#Area().` | `Circle.Area`, then the interface's method |
| `t.Hello()` from an embedded type in another package | `inner/Base#Hello().` | that method |
| `st.Push(1)` on a generic type | `Stack#Push().` | that method |
| `v.Area()` in a type switch | the method of that `case` | that method, then the interface's method |
| `n.Name()` on an interface embedding another | `Named#Name.` | the interface's method, then `Tag.Name` |
| `f()` after `f := helper` | a local | the variable |
| `g := c.Area`, `h := Circle.Area` | the method, as a value | the variable, at the later call |
| `Fn(helper)`, `MyInt(1)` | the type | the type |
| `float64(1)`, `len(x)` | nothing | `builtin.go` |
| `alias.Plain()` | `inner/Plain().` | not asked |
| `go helper()`, `defer helper()`, in `func() {...}()` | `helper().` | not asked |
| a file `win_windows.go` | not in the index | not asked |

Relations in the index: `Circle#Area().` and `Square#Area().` to `Area#Area.`, `Tag#Name().` to `Named#Name.`, and for the types
`Circle#`, `Square#`, `Tag#` to `Area#`, `Tag#` to `Named#`. On hugo 5 007 in all, not checked one by one.

**Layouts.** Index at a `go.work` with no `go.mod`: exit 0, no documents, and the same with two modules and neither. In a module of a
`go.work`: its own files, with the other module's symbols named.

**Running the project.** A `//go:generate touch` in a package was not run. A `go.mod` asking for Go 1.99 with `GOTOOLCHAIN=local`:
exit 1, with the reason. A package with an import that does not exist and an undefined name: exit 0 and no word of it. Its file is
in the index with what resolved, and the two names have no occurrence, so no wrong target. `#cgo pkg-config:` of a package that does
not exist: exit 0 as well.

**What `go list -e` sees** of a package that does not compile, all with exit 0:

| | `scip-go` | `go list -e` | With `-export` | With `-export -test` |
|---|---|---|---|---|
| An undefined name | nothing | nothing | `Incomplete`, file, line, message | the same |
| An import that does not exist | nothing | `Incomplete`, in `DepsErrors` | the same | the same |
| An undefined name in a test file | not tried | not tried | nothing | `Incomplete`, file, line, message |

On hugo with the cache filled: 192 packages, 0 incomplete, 0.96 s, 64 MB, without `-test`.

**kubernetes** at `a65bce7f`, 13 688 files in 41 `go.mod` and a `go.work`, of which `_mock` is left out by section 1. One root at a
time, the fewest files first, the root last. Go's cache was not empty at the start, and held little of kubernetes:

| | Time | Peak memory | Index |
|---|---|---|---|
| 28 modules, 3 to 101 files, `scip-go` with the network | 1 740 s, up to 215 s for one | 506 MB | |
| 39 modules, no network, cache filled by the run above | 104 s, up to 20 s for one | 772 MB | 254 MB |
| Root, 5 681 files of its own: `go list -e -export -test` | 170 s, 3 289 packages, 0 incomplete | 812 MB | |
| Root: `scip-go`, no network | 27 s | 4.1 GB | 315 MB |

The cache grew from 35 to 48 GB. In `cloud-provider`, 215 s held 27 s of processor, and `scip-go` was seen connected to `k8s.io`.

**With and without the network** on `metrics`, `cloud-provider` and `kube-aggregator`: the same occurrences, symbols and relations, in
another order within a document. `metrics` took 41 s and 0.5 s, the second with the cache filled. The other modules were not compared.
`GOPROXY=off` as well left 117 packages of `code-generator/examples` incomplete, for want of a module; with `go list` first and
only the proxy variables closed, 0. A module with no git, with git and no remote, and with a remote on an unknown host: exit 0, and
the same occurrences apart from the version.

**Across modules,** over the 39 without the root: 1 033 758 uses of a symbol of another of them, of which 7 247 have no definition by
the symbol without its version. 6 418 of those have one in the other form of section 4. 829 have none.

## 8. Open

1. **A build from an empty cache** of kubernetes was not measured, so the 170 s is less than a first index costs, and 600 s for a
   root is not known to be enough, there or on a slower machine.
2. **Reading 575 MB of indexes in Mimir** is unmeasured: the time, and T4, under 400 MB, on such a project. It can only be measured
   once the reader exists, and it may change the design.
3. **The 829 uses of section 7** have no definition, so no edge. Some look like a field through a defined type, as
   `type CounterOpts KubeOpts`, with the field looked for under `CounterOpts`. They were not checked one by one, and uses from the
   root were not counted at all.
4. **The closed proxy** was measured with port 9, not with a port taken and let go, and with `NO_PROXY` as it was. `scip-go` was not
   told of its fault, which the owner will decide. The two steps in this order were run on `code-generator/examples` and the root only.
5. **`gopls` and the network.** Whether it asks a module's host as `scip-go` does was not looked at.
6. **Why a change in `common/hstrings` took 293 s on hugo** is still not known. hugo has no `go.work`, so the fault above need not be it.
7. **An edit changes more than its file.** A method added to an interface changes what implements it in files not edited, as a changed
   signature does what a call means. They stay as the index says until the next one, as with Rust. Not measured for Go.
8. **`implementation` at a declaration** was tried at sites only. It is in the design from those answers, and GO1 is to hold it.
9. **A type with no method of its own** that fits an interface through embedding, as `Tag` through `Circle`, has a pair of types and
   no pair of methods. The method it runs is the embedded one, which has its edge. Not checked for a method overridden on the way.
10. **Calls the source does not spell,** as in section 3, and `f()` on a variable holding a function: never guessed, and the function
    named is `UsesAsValue`.
11. **Files outside the build** have syntax edges alone, however many a project has, and a project's tags cannot be set. An edited one
    also costs the 5 s of the second try, once in a run, shared with any others.
12. **`cgo`:** `C.f()` is not linked. What a project's flags can do with its C compiler was not tried beyond the case above.
13. **A build in the daemon.** A first index takes all cores for minutes and fills Go's cache. Its effect on T8 is unmeasured.
14. **Only `gopls` and `scip-go` were looked at.** There is no other maintained server for Go, by what is known and not by a search.
15. **Whether Homebrew can provide them** is not checked, and with Go required the formula's caveats name `go install`.

## 9. Order

1. `mimir-resolve-scip`, from `mimir-resolve-ra`, with no change in behaviour: T5 and T6 on Rust.
2. The decoder's new fields, implementations in the port, and definitions across indexes, with no change for Rust.
3. `mimir-parse-go`, with syntax rules only.
4. `mimir-resolve-go` with `scip-go`, and the fixture of GO1.
5. `gopls` for edited files, the second try, and GO3.
6. GO5, kubernetes, and what they decide of section 8, items 1 and 2.
7. GO2 and GO4 in `scripts/bench`, and the README, `SECURITY.md`, the Homebrew text and CI.
