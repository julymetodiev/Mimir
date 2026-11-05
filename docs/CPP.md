# C and C++

The specification for C and C++ under section 8 of [ARCHITECTURE.md](ARCHITECTURE.md). The rules, layers and targets there
hold here too. **Status: not built; the owner approved it on 2026-11-04, and the work follows section 9.** Section 8 says what is open.

C and C++ work as Rust and Go do: an index of the whole project, and a running server for what the index leaves out. They are
one family, with one index and one server, as a C++ file calls C and a `.h` serves both. Section 7 has the numbers.

## 1. Scope

Files: `.c`, `.h`, `.cpp`, `.cc`, `.cxx`, `.hpp`, `.hh`, `.hxx`. Whatever git ignores is left out, and over 500 KB the scanner
leaves a file out, as for every language.

A file in no compilation database and included by none, as `env_windows.cc` on a Mac, has syntax rules only, and `clangd` never
opens it: section 7 shows what that does to other files' answers. Code under an `#if` that is not compiled has none either.

## 2. The parts

```
  mimir-parse-c        ── symbols and references of both languages, through tree-sitter
  mimir-resolve        ── the same rules and the same port
  mimir-resolve-scip   ── keeping an index by a hash, and the choice between kept index, server and new index, as it is
  mimir-resolve-clang  ── what is C's own: the database, `clangd-indexer`, `clangd`
  mimir-lsp            ── starting a language server, its messages, file URIs
```

| Crate | Layer | Owns |
|---|---|---|
| `mimir-parse-c` | domain | C through `tree-sitter-c`, C++ through `tree-sitter-cpp` |
| `mimir-resolve-clang` | adapter | finding the database, running `clangd-indexer`, turning its output into a SCIP index, asking `clangd`, the versions tested |

- **Nothing is copied.** `mimir-resolve-clang` implements `IndexLanguage` and calls `load_root`, as `mimir-resolve-go` does.
- **`clangd-indexer`'s output becomes a SCIP file** in `write_index`, so `decode`, `SemanticIndex` and the port stay as they are.
  A symbol is its id from the index. A definition has role 1. Pairs of an override and what it overrides become `implementations`.
- **`Language::family`** gives C for `Cpp` too.
- **`.h` becomes `Language::Cpp`,** from `Language::C` today, as a header may hold either, and the C++ grammar reads what headers
  hold. This is not measured: section 8.
- **`commands/daemon.rs`** stays the only place that names the sources and the parsers.

## 3. Symbols and references

Symbols: function, method, constructor and destructor; struct, class, union, enum and its values; typedef and alias; field;
namespace; a variable at file level. A qualified name is the scope and the name: `geo::Circle::area`. A template is one symbol,
whatever its arguments, and a specialization is another: `Box::put` and `Box<char>::put` are two. Overloads are two.

A declaration and its definition are one symbol, at the definition. A function only declared, as a pure virtual, is at its
declaration.

| In the source | Reference | Edge |
|---|---|---|
| `f()`, `ns::f()`, `x.m()`, `x->m()`, `T::m()` | `Call` | `Calls` |
| `x.m()` on a virtual | `Call` | `Calls` the method named; what overrides it through `Implements` |
| `T x(1)`, `new T(1)` | `Call` | `Calls` the constructor |
| `a + b` on a class | none | no edge: an operator is a call the source does not spell |
| `CALL(x)`, a macro that calls | `Call` by what the index holds | `Calls` what it runs, at the macro's place |
| `o.run = f`, `fp = f` | `FunctionValue` | `UsesAsValue` |
| `o.run(x)`, `fp(x)` | `Call` | none: the target is not known |
| `#include` | `Import` | none |
| a type named | `TypeUse` | `UsesType` |
| `x.f`, `x->f` | `FieldUse`, `FieldWrite` | `UsesField` |
| a variable at file level read | `FunctionValue`: what it names decides | `UsesConstant` |

**Macros are not symbols.** A call through one reaches the function it runs, as `CALL_TWICE(r)` reaches `twice`, and the index holds
no macro. So `trace DEBUGASSERT` finds nothing. By the owner's word.

**A macro in the way of the grammar.** tree-sitter does not know what a macro is: `class EXPORT Name {` reads as a function called `Name`, and
`int f() LOCKS(mu) {` as two declarations. So before parsing, a name of capitals in those two places becomes spaces, and every position stays.
It only goes before what starts a body or a definition, so `if (x) LOG(y);` stays a call. Nothing here says what a call refers to.

**A value is not a call.** `clangd-indexer` marks `o.run = twice` as a call, so what is a call comes from the parser, as for
other languages, and not from the index.

A caller is the innermost named function around the site. A lambda belongs to the one around it. A call through a pointer or a
`std::function` gives no `Calls`: what it runs is never guessed.

## 4. Resolution

**The database.** `compile_commands.json` in the project's root, or else in `build/`. Mimir never runs `cmake`, `make` or `bear`, as
each runs the project's code. With none, only syntax rules apply, `clangd` is not started, and `sync` and `status` say so, with what
to run:

- CMake: `cmake -B build -DCMAKE_EXPORT_COMPILE_COMMANDS=ON`
- Make: `bear -- make`

The database's flags are the project's. An `#include` it cannot find is a compiler's error: section 8 has what to do with them.

**The index.** `clangd-indexer --executor=all-TUs --format=yaml DATABASE`, in the database's directory. Its
output is read once, and kept as a SCIP file of the project's symbols. A new index is written when the database changes, when more
than 50 files are edited, or by `sync --full`, as in section 5 of the architecture. The manifest is the database. One root: the
project.

**What the index leaves out, and `clangd`.** One `clangd`, started with
`--background-index=false --enable-config=false --log=error --header-insertion=never --compile-commands-dir=DIR`. With no index of its
own it writes no `.cache/` into the project, and it opens only files of the database, and headers. It answers twice:

- **While an index is written, once, for local symbols:** those defined in a source file with their declaration in the same file. `clangd-indexer` records no use of such a
  symbol. Only the sites whose name is one in that file are asked, and what it says goes into the index, which so holds them from then on, at no cost for each load.
  Without `clangd` the index stays, with no such uses.
- **For edited files,** each site the parser found.

**One answer, for each site:**

1. `textDocument/symbolInfo` gives its id. None means no answer: this drops what `clangd` answers by a word's text, as `get` in
   `gtest.h` going to `format-inl.h`. A site with no id keeps its syntax edges.
2. An id with a definition in a file asked about, by `textDocument/documentSymbol`, goes to it, by its line now.
3. Otherwise to where the index has it, if in one place or in this file.

A target is exactly what it names at its own line. At the name in `T(1)` `clangd` says the class, and at the bracket the constructor this call runs: both are asked, so the edge is to each.

So `clangd` says which symbol a site is, and never where it is. An edited file's definitions go by the lines `clangd` gives for their ids, and not by their order among those of a name, which
two definitions under `#if` would get wrong.

**A symbol of several programs,** as `main` in curl's `docs/examples`, is one id. The index keeps its definition in each file, from the references that are definitions, and a name looks for the one in
its own file first, and otherwise for the only one there is. With several and none here there is no edge.

**Overrides** come from the index, by ids, so a pair holds after an edit. A method added since has none until the next index.

**Syntax rules,** until answers come or without these tools: a call links to a function in the same file with exactly one definition
of that name, not after `.`, `->` or `::`. Section 7 has its numbers. It is not always right, in C when one name is a function under one
`#if` and a macro under another, so it holds only until the index and `clangd` answer. Across files, no edge.

**The tools.**

- They are found on `PATH`, never in the project.
- `clangd-indexer` has one tested version, a constant in `mimir-resolve-clang`, which `scripts/check-deps.py` compares with the
  newest release of `clangd/clangd`. `clangd` is whichever is there: Apple's follows Xcode. The daemon's log says when either is
  not the tested one.
- Without `clangd-indexer`, or when it fails: `clangd` is not asked either, only syntax rules apply, and `sync` says why. Without
  `clangd`: the index stays, and edited files and local functions have syntax rules.
- Install: on a Mac `clangd` comes with `xcode-select --install`, on Linux with the system's package. `clangd-indexer` is in
  `clangd_indexing_tools` of a [release of clangd](https://github.com/clangd/clangd/releases). They are never linked.

**What runs.** Neither compiles or runs the project's code: each parses it, with the database's flags. `clangd-indexer` runs `xcrun` on a Mac to find
`clang`. `--enable-config=false` keeps a `.clangd` in a project from setting how `clangd` reads it. `clangd-indexer` 23.1.0 has no such flag, though newer sources do,
and a `.clangd` adding a `-D` changed nothing it found. Neither is told `--query-driver`, which is what asks a compiler the database names; whether either
runs one otherwise was not watched. What a project's flags, as `-fplugin` or `-load`, can do was not tried: section 8. See `SECURITY.md`.

**What goes out.** Whether either tool talks to the network was not looked at: section 8.

## 5. Targets

Added to section 2 of the architecture when they hold, each checked by `scripts/bench`.

| # | Target |
|---|---|
| CPP1 | Every entry of the hand-written list in `crates/mimir-resolve-clang/tests/`, covering each row of the table in section 3, holds |
| CPP2 | Against `clangd` with its own index over all sites of a project: every disagreement falls in a named class of section 7; unclassified 0 |
| CPP3 | After edits, the graph differs from the graph from a new index only in the named classes of section 7, each of an edit's own files |
| CPP4 | T1, T2, T3 and T4 hold on a project of 20 000 symbols, T3 with the index kept |
| CPP5 | Whatever the order of asking, the same answers: 0 differences on curl, fmt and yaml-cpp |

## 6. Machine checks

- `scripts/check-architecture.sh`: the new crates in `allowed`; `mimir-engine` depends on no parser of a language;
  `mimir-parse-c` is in the pure domain list.
- `scripts/check-deps.py`: the version of `clangd-indexer`, and `tree-sitter-c` and `tree-sitter-cpp` as any crate.
- `cargo test`: the fixture of CPP1 and CPP5, skipped with a message when the tools are not installed. CI installs them.
- `scripts/bench`: CPP2, CPP3 and CPP4.

## 7. What was measured

`clangd` Apple 21.0.0, `clangd-indexer` 23.1.0, `scip-clang` 0.4.1, `norn` 0.25.0, `cpp_ls` 0.1.4, on an arm64 Mac, 2026-11-04,
without `mimir`. curl at `116c5ae`, fmt at `4ed3535`, leveldb at `bb74ef7`, yaml-cpp at `c5e6536`, and a fixture of 7 files by
hand. A site is a name before `(`, or after `.`, `->` or `::`, by a regular expression outside comments and strings. Times marked
loaded ran beside other runs.

**Why an index.** Each from nothing:

| | `clangd`, each site | `clangd-indexer` | `clangd` for local functions |
|---|---|---|---|
| curl, 73 114 sites | 74 to 94 s, 673 MB, loaded | 3.6 s, 237 MB | 17 to 19 s, 381 MB |
| yaml-cpp, 20 081 sites | 42 s, 1 GB, loaded | 2.6 s, 440 MB | 4.4 to 4.7 s, 327 MB |
| fmt, 36 249 sites | 68 to 157 s, 1.4 GB, loaded | 15.6 s, 778 MB, loaded | not measured |
| leveldb, 7 046 sites | 54 to 63 s, 919 MB, loaded | 7.8 s, 435 MB, loaded | not measured |

Memory of `clangd-indexer` is its largest process. Of `clangd`'s time for local functions on curl, 15 to 17 s is opening 178 files and
0.5 s asking. By an estimate from these rates, each site on Linux would take about 2.5 hours in each direction, which the owner
refused to run.

**The index.** curl's YAML is 109 MB, of which 88 names the project, and read in 2 s by Python; binary 6.1 MB, a RIFF of format
version 20. yaml-cpp's YAML is 50 MB, of which 12 names the project. On leveldb it has 0 errors with no `-isysroot` and with
`SDKROOT`, and the same size within 0.1%.

**Does the order of asking change an answer?** `clangd` with its index ended, files in order and in reverse, only files of the database
and headers:

| | Sites | Differences | Those with an id from `symbolInfo` | Of those, with no common target |
|---|---|---|---|---|
| leveldb | 7 046 | 3 | 1 | 0 |
| fmt | 36 249 | 561 | 76 | 4 |
| curl | 73 114 | 1 878 | 134 | 133 |

The 133 and 4 are one symbol with several definitions: `main` in curl's `docs/examples` 131 times, `curlx_strdup` under two `#ifdef`, `format_as`.
With all files, leveldb has 207 of 15 643, and `Env::Default` goes to `env_posix.cc` or `env_windows.cc`. Without the index ended, 24 of
77 on the fixture.

Asked about local functions alone, with no index and the rule of section 4, step 2:

| | Sites | Differences without the rule | With it | Against each site, not read one by one |
|---|---|---|---|---|
| curl | 10 348 | 1 | 0 | 72 differ |
| yaml-cpp | 145 | 11 | 0 | 4 differ |

The 12 examples read of these are a definition against its declaration, and one `main`.

**After an edit,** `clangd` running from before it, against one started anew on the edited copy with its own index. 5 lines moved in 3
files of which one a header, a new local function, and a new function called from another file. Answered in 0.09 s and 0.24 s; each id
equal to a new start. This is the edited files alone, and so not CPP3.

| Sites | yaml-cpp, 311 | curl, 853 |
|---|---|---|
| Same target | 255 | 685 |
| The site is the definition or declaration itself | 41 | 34 |
| Target outside the project | 11 | not read |
| No symbol on either side | 2 | 33 |
| The index has no target, the target is in the project | 0 | 44 |
| Another target in the project | 2 | 43 |

The 2 are a namespace. Of curl's 87, the 24 read are macros, where the new start goes to the `#define` and the id to what it runs, as
`curlx_strdup` to `lib/easy.c:114`. The other 63 were not read.

**Ids of `clangd` 21 and `clangd-indexer` 23 at one position:** curl 43 217 of 43 380 equal, yaml-cpp 7 649 of 7 928. Part of curl's rest
was the reader of this measurement, which kept quotes on an id of digits alone. yaml-cpp's example is `resize` on a template.

**What `clangd-indexer` has no reference for,** against `scip-clang`:

| | curl | leveldb | fmt |
|---|---|---|---|
| Target in another file. The examples are declarations | 2 239 | 813 | 5 515 |
| Target only in this file | 602 | 338 | 448 |
| In this file, a header or with other definitions | 10 | 79 | 455 |
| In a header of the project | 0 | 0 | 364 |

Some count twice. On the fixture: `local_one`, `helper` and `plain`, each `static`, have no reference. Its source sets no option for them
and there is no flag. **The same run twice** gives leveldb 0 differences of 3 622 calls, also with one thread; curl 288 of about 37 500, and
510 with one thread, part of it `main`, and 113 in `lib/pop3.c`. It indexes each file once, which may be why.

**The rule for syntax,** against `scip-clang`:

| | Agree | Differ | The index has nothing |
|---|---|---|---|
| curl | 2 790 | 0 | 156 |
| leveldb | 106 | 0 | 1 |
| fmt | 399 | 18 | 93 |

**What `mimir-parse-c` finds,** through `mimir` at this draft with syntax rules alone, against the functions `clangd-indexer` has defined in the project.
Left out as what a parser cannot see: what a macro generates, as `TEST` and curl's `typecheck-gcc.h`, and a test library in the project:

| | Functions | Found | With no blanking of macros | Syntax `Calls` that agree with `scip-clang` | Differ | The index has nothing there |
|---|---|---|---|---|---|---|
| curl | 2 828 | 2 824, 99.9% | not run | 2 390 | 22 | 1 973 |
| leveldb | 970 | 949, 97.8% | 838, by an older count | 94 | 0 | 178 |
| yaml-cpp | 1 202 | 1 123, 93.4% | 978, by an older count | 114 | 3 | 1 241 |
| fmt | 1 861 | 1 370, 73.6% | not comparable | 373 | 47 | 123 |

A whole `mimir sync` of it took 0.4 to 1.6 s. Of curl's 22, 20 are a name that is a function under one `#if` and a macro under another. Of fmt's 47, those read are
`open_buffered_file`, defined in two files, which is the case of section 8, item 4. Why fmt is low was not worked out to the end: `FMT_ENABLE_IF` in a template's
parameters, as `isfinite` in `format.h`, is part of it, but plain functions in the same file, as `equal2`, are missing too. leveldb's rest is `#if` inside a
function's parts and classes inside functions, by the examples read.

**Through `mimir`, with the index and no `clangd` yet,** on an idle machine. `Calls` from the index against `scip-clang`, at the same place and target:

| | A first `mimir sync` | Kept index | Same | Another target | `scip-clang` has no definition | Files in the index, and with syntax rules |
|---|---|---|---|---|---|---|
| curl | 6.7 s | 12 MB | 8 027 | 3 | 0 | 535, 586 |
| yaml-cpp | 4.6 s | 0.6 MB | 1 611 | 23 | 1 | 125, 181 |
| leveldb | 3.2 s | 0.5 MB | 1 581 | 3 | 0 | 86, 47 |
| fmt | 8.0 s | 4.4 MB | 6 037 | 223 | 17 | 59, 19 |

Files with syntax rules are those in no database, as curl's other platforms and leveldb's tests that were not configured; `scip-clang` has nothing at their places: 1 661,
1 119, 204 and 812. Of the differences, those read are `scip-clang` dropping `extern "C"`, as `NewLRUCache` in leveldb's `db/c.cc`, what a macro runs, as `FMT_STRING`,
and a class against its constructor. They were not read one by one. Calls to a local function in a file of the index have no edge until `clangd` answers: `scip-clang`
has about 8 000 on curl, 300 on yaml-cpp, 500 on leveldb and 3 900 on fmt, some counted twice.

**With `clangd` for local symbols,** by what calls what, each pair once, against `scip-clang`, in a function and to a symbol of the graph. A first `mimir sync` on an idle machine:

| | A first `mimir sync` | Kept index | Local: has an edge | No edge | Elsewhere: has an edge | No edge |
|---|---|---|---|---|---|---|
| curl | 23 s | 12.6 MB | 2 533 | 3 | 5 935 | 10 |
| yaml-cpp | 7.6 s | 0.6 MB | 565 | 1 | 1 080 | 0 |
| leveldb | 12 s | 0.6 MB | 489 | 24 | 1 363 | 4 |
| fmt | 19 s | 4.8 MB | 1 684 | 631 | 4 923 | 217 |

Of leveldb's 24, those read are `new` of a class known only in its file, where `clangd` at the name gives the class and the index would give the constructor, and a class inside a function, which is no symbol. Of fmt's 631,
those read are `TEST`, a macro. Calls to what the parser has no symbol for are left out above: on curl 42 576 sites of what `CURLWARNING` generates, and on fmt 12 623 of
`gmock-gtest-all.cc`, over the scanner's 500 KB. An index is written only when the database changes, over 50 files are edited, or by `sync --full`, so the time for `clangd` is paid then.

**After edits, the whole graph,** `mimir sync` against `sync --full` of the same files, with `clangd` from before them. In each project 5 lines moved in 3 files of which one a header, a new local function, and a
new function called from another file. A first answer in 0.5 to 0.8 s:

| Edges only after the edit, only in the new index | yaml-cpp | curl | leveldb | fmt |
|---|---|---|---|---|
| `Calls` | 0, 14 | 0, 5 | 0, 1 | 8, 29 |
| `Implements` | 0, 0 | 0, 0 | 0, 0 | 0, 0 |
| `UsesAsValue` | 0, 2 | 0, 0 | 0, 13 | 0, 2 |
| `UsesConstant` | 0, 0 | 0, 20 | 0, 1 | 0, 0 |
| `UsesField` | 0, 0 | 0, 5 | 0, 0 | 1, 18 |
| `UsesType` | 1, 3 | 62, 15 | 0, 2 | 0, 31 |
| All edges of the new index | 8 242 | 69 198 | 6 536 | 21 260 |

The classes, by what was read, not one by one:

1. **A constructor no bracket shows:** `new T` and `T x;`, a copy at `return s`, the copy of what is thrown, as `ParserException` on yaml-cpp, and a conversion. Also
   arguments that depend on a template, where `clangd` gives the class or nothing, and an alias of an instantiated template, as `node_iterator`, whose id is not in the index.
   Where a bracket follows and none of these holds, `clangd` names the constructor the index has: 202 of 205 sites on leveldb, 295 on yaml-cpp and 417 on fmt.
2. **What a macro runs:** an index records it at the macro's place, and `clangd` gives the macro, as `curlx_free` to `Curl_cfree` on curl.
3. **A `typedef` and a struct of one name:** `curl_mimepart`, where `clangd` gives both and the index the `typedef`: curl's 62 are this.
4. **A file with nothing in the index** counts as outside the build when it changes, until the next index: fmt's `test/util.cc`.

**Two new indexes of the same unchanged files** differ on fmt and not on the others: `Calls` 27 and 7, `UsesField` 18 and 0, `UsesType` 21 and 0, on yaml-cpp, leveldb and curl 0 in every kind. So most of
fmt's row above is not from the edit. Leaving out the pairs of files that differ between the two, fmt has `Calls` 1, 2, `UsesAsValue` 0, 2, `UsesField` 1, 0 and `UsesType` 0, 10.

One edge has no explanation: on leveldb `Repairer::WriteDescriptor` to `log::Writer::Writer`, at `log::Writer log(file)`. The parser has the site there and `clangd` names that constructor at its bracket, and the edge is
still only in the new index.

**LLVM,** only `clangd-indexer`, not Mimir as a whole, since a whole run could take over an hour of the machine and the owner cannot spare that. The `llvm` folder, AArch64 only, 3 291 units, 8 cores: 665 s, 2.26 GB, 2.1 GB of YAML, 370 973 symbols, and 7 errors for files the build makes. Before it, 245 s to make the generated files, and 111 s to clone and 29 s for `cmake`. The parser alone reads the tree in 32 s: 17 719 files and 334 220 symbols.

**The database.** With none, `clangd` gave 13 of 77 fixture answers at a declaration. `clangd` found one in `build/` itself, and needed no
`-isysroot`, 0 of 77 changed.

**What was tried and is not used:**

| | Why |
|---|---|
| `clangd` at each site | The time above, which grows with the code. It stays for what the index leaves out |
| `scip-clang` | Exit 0 and `0 errored TUs` with 37 of 39 units of leveldb stopped by standard headers, without `-isysroot` or `SDKROOT`, seen only with `--show-compiler-diagnostics`. Drops all of `extern "C"`, its issue 544. `static` functions of one name share a symbol. No file for an Intel Mac or ARM Linux, not in Homebrew. It was the reference above |
| `norn` | Own parsing and no compiler, so targets marked `certain` are wrong: `iter->Valid()` to `SkipList::Iterator::Valid`, `pick(1)` to both overloads. `certain direct` against the index: curl 10 051 agree and 56 differ, leveldb 2 658 and 301, fmt 22 577 and 9 652, not read one by one. Fastest, 0.38 s on curl, and needs no database |
| `cpp_ls` | Own semantics: `pick(1)` to both overloads, `Box<char>::put` to the template. 562 s on fmt against 52 s. Writes `.cppls/`. Stopped on the owner's word before curl |
| `ccls` | Links Homebrew's `llvm`, 1.8 GB. On the fixture 3 of 77 changed by order, 2 of them empty |

`clice` was not run. By a search it is written in C++ and warns against use in production.

## 8. Open

1. **`.h` as C++ grammar and no edge through a pointer** are by the owner's word, with the costs of items 7 and 20 known.
2. **A large project is not proven.** Nothing over 73 114 sites went through Mimir as a whole. On LLVM the index takes 665 s, within the limit of 1 hour in `crates/mimir-resolve-clang/src/index.rs`. An indexer that hangs holds its index that long. Reading 2.1 GB of output, `clangd` for local symbols and T1 to T4 there are not known. T4 over 20 000 symbols fails for Go, and may for this.
3. **CPP3 allows the classes of section 7, where other languages have 0 differences:** the owner had the constructor checked first and left this to be decided after it. It stays, since what is left is what `clangd` does not say at a site, and fmt's new indexes differ from each other, so 0 could not be measured there. They go at the next index. An edit to a header also changes what its includers mean, and they stay as the index says until then.
4. **A method added since an index** that overrides another has no edge until the next index.
5. **The ids that differ:** 3.5% of yaml-cpp's positions and 0.4% of curl's, between `clangd` 21 and `clangd-indexer` 23. Why is not known,
   and step 4 of section 4 depends on them. A differing id gets no target, so an edge is lost and is not wrong, but this was not checked.
6. **Disagreements with the index not read:** 2 567 on curl, 272 on leveldb, 3 786 on fmt, against a whole run of `clangd`. Examples are a macro
   against the call in it, a class against its constructor, and templates. CPP2 has to name each class.
7. **What the parser misses.** Section 7 has the numbers. On code as full of macros as fmt, 27% of its functions are no symbols, so they have no edges, and the
   cause is not all known. On curl, `.h` as C++ grammar lost nothing measured; a C name as `class` or `new` in a header was not looked for.
   `tree-sitter-cpp`'s last release is 0.23.4 of 2024, and it works with `tree-sitter` 0.27.1.
8. **Files of `.ipp`, `.inl`, `.tpp` and the like** are not in the list of section 1. `cpp_ls` reads them. Not decided.
9. **The kept index** on a project far over curl's size: section 7 has what it is up to there. Reading a new daemon's index was not timed alone. `clangd-indexer`'s YAML is what
   is read, though its binary form is 18 times smaller: a binary format has a version, and was not read here.
10. **The differences between runs of `clangd-indexer`** on curl, partly not explained, go against T6. A cause and a fix are not known.
11. **Errors in a database's compile.** `clangd-indexer` puts them in its log and exits 0, as `scip-clang` does, on fmt's `fmt.pcm`. On leveldb, `scip-clang` stopped at
    `snappy.h`, which its CMake finds with no `-I` passed; the runs of `clangd-indexer` had that `-I` added by hand. Whether to read them, and
    how `sync` should report them, is not decided.
12. **Linux:** nothing was measured there, and `clangd` and `clangd-indexer` of different versions were only run on a Mac. The Linux archive
    is 160 MB, and its architectures were not checked. ARM Linux may have no `clangd-indexer`.
13. **Installing:** 109 MB for a Mac, by hand. Whether the tap can bring it was not looked at. `clangd` comes with Apple's tools, whose
    version rule 10 cannot move: tested with one, by what the owner decides.
14. **`--enable-config=false` on `clangd`** was not run, and a `.clangd` not tried on it. A newer `clangd-indexer` reads `.clangd` by default, so a new tested version has to be looked at for this.
15. **What a project's flags can do,** as `-fplugin` or `-load` in a database, was not tried. A database is the project's own.
16. **The network:** neither tool was watched.
17. **Several databases,** as a project with `cmake-build-debug/` and `cmake-build-release/`, or sub-projects, are not found by section 4.
18. **Without a database** there is nothing but syntax rules in one file. `norn` shows more can be had, at the cost of wrong targets.
19. **`curl`'s 6 units of 515** that `scip-clang` did not index: not looked at, and whether `clangd-indexer` has them.
20. **Calls through a pointer** give no edge, by section 1 of the architecture. Much C goes through them, as curl's `cf->cft->do_connect`.
21. **New indexes of fmt differ from each other,** by section 7, around `fmt-c.h` and `test/os-test.cc`. Why is not known. A header read as C in one unit and C++ in another is a guess, not checked.
22. **`T{1}` is `UsesAsValue`,** where `T(1)` is `Calls`, in the index and after an edit alike, since the text after the name decides. It runs a constructor all the same.
23. **The one edge of section 7 with no explanation,** on leveldb, and the differences not read: curl's `UsesConstant`, `UsesField` and `UsesType` other than the 62, and fmt's 10 `UsesType`.

## 9. Order

1. `mimir-parse-c`, with syntax rules only, and what it finds against the index. Done, and section 8, items 7 and 8 stay open.
2. `mimir-resolve-clang` with `clangd-indexer`, its output as a SCIP file, and the fixture of CPP1 without local functions. Done.
3. `clangd` for local symbols and edited files, the rule of section 4, and CPP5. Done on the fixture; item 5 stays open.
4. CPP3, and item 3. Done on four projects.
5. A project larger than curl: only the index of LLVM was measured, by the owner's word. Item 2 stays open.
6. CPP2 and CPP4 in `scripts/bench`, items 6 and 10, and the README, `SECURITY.md`, the Homebrew text and CI.
