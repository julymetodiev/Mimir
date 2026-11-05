# Lantern Lattice architecture

The specification of the system: code follows it exactly. Code cites it by section number. Why a
decision was made is in git and in project memory, not here.

## 1. The system

Lantern Lattice gives coding agents an understanding of code: who calls what, where a symbol is, and safe
edits by symbol. It is one binary, `latt`, with no daemon to manage and no language model inside.

```
  Claude Code ── plugin ── latt hook (no daemon)
  Claude Code ── plugin ── latt mcp ─┐
                                      ├── client ── socket ── daemon ── engine
  a person ─────────────── latt ... ─┤
  a browser ── latt view ────────────┘
```

- **One daemon per project**, started by the first command that needs it and stopped when idle.
  There is no other path: no command builds or reads the graph in its own process.
- **Why a daemon:** measured on 1311 files, `trace` takes 0.02 s through it and 2.2 s without it.
  After an edit, a process of its own pays for a new `rust-analyzer` on every query (25 s, 3.7 GB);
  the daemon keeps one running.
- **Calls are never guessed.** What cannot be known is left out and reported. Calls stay separate
  from possible targets (trait implementations, functions as values), which `trace` marks.

## 2. Targets

Each is a number `scripts/bench` can check, on a project of 20 000 symbols. All hold today.

| # | Target |
|---|---|
| T1 | A query with nothing changed: under 50 ms |
| T2 | A query after an edit: first answer under 1 s, marked partial until complete |
| T3 | From no daemon to a first answer, with the index written: under 5 s |
| T4 | Daemon idle, without the analyzer: under 400 MB |
| T5 | Call edges: nothing lost against the SCIP index; every entry of the hand-written list in `crates/latt-resolve-ra/tests/fixture/expected.rs` holds |
| T6 | After edits, the graph equals the graph from a new index: 0 differences |
| T7 | `latt view`: first frame of the whole graph under 1 s, then 60 frames a second with layout running, on a current laptop |
| T8 | With no view open, T1 to T4 hold as if the view did not exist |
| GO1 to GO5 | The targets for Go, in [GO.md](GO.md), section 5 |

## 3. Layers

Dependencies point inwards. IO lives at the edge.

```
  transports    lantern-lattice, latt-mcp, latt-daemon     map a request to one engine call, nothing else
  client        latt-client                           connect, start the daemon, one call per command
  wire          latt-protocol                         requests and responses, typed
  application   latt-engine                           the one writer: sync, watch, query, edit
  adapters      latt-resolve-scip, latt-resolve-ra,  a SCIP index, rust-analyzer, scip-go, gopls, tsrs, pyrefly, clangd-indexer, a language server, files on disk
                latt-resolve-go, latt-resolve-clang, latt-resolve-lsp, latt-lsp,
                latt-edit
  ports + rules latt-resolve                          linking rules and the port for a semantic source
  domain        latt-graph, latt-graph-fb, latt-parse-core,         pure: no IO, no async, no clock, no environment
                latt-parse-rust, latt-parse-ts, latt-parse-python, latt-parse-go, latt-parse-c,
                latt-types
```

| Crate | Layer | Owns |
|---|---|---|
| `latt-types` | domain | ids, symbols, references, locations, languages |
| `latt-parse-core` | domain | the language adapter trait and what it extracts |
| `latt-parse-rust` | domain | Rust through tree-sitter |
| `latt-parse-ts` | domain | TypeScript and JavaScript through tree-sitter |
| `latt-parse-python` | domain | Python through tree-sitter |
| `latt-parse-go` | domain | Go through tree-sitter |
| `latt-parse-c` | domain | C and C++ through tree-sitter |
| `latt-graph` | domain | symbols, edges and queries over them, and the graph as bytes for a file |
| `latt-graph-fb` | domain | the code `flatc` generates for the file, the only `unsafe` it takes to read it, behind a check of every offset |
| `latt-resolve` | rules + port | turning references into edges; syntax rules; the port `SemanticSource` |
| `latt-resolve-scip` | adapter | reading a SCIP index, keeping it by a hash of the sources, the choice between a kept index, a running server and a new index |
| `latt-resolve-ra` | adapter | what is Rust's own: Cargo roots, failed macros, the running `rust-analyzer` |
| `latt-resolve-go` | adapter | what is Go's own: `go.mod` roots, `scip-go`, `gopls`, the versions tested |
| `latt-resolve-clang` | adapter | what is C's own: the compilation database, `clangd-indexer`, its output as a SCIP file, the versions tested |
| `latt-resolve-lsp` | adapter | a language server as a semantic source: asking about each site, saving the answers; one module for each server, as `tsrs` and `pyrefly` |
| `latt-lsp` | adapter | a running language server: starting it, its messages, file URIs |
| `latt-edit` | adapter | applying edits to files, with conflict detection |
| `latt-hook` | adapter | what to answer a coding agent's hook, and a mark on disk for each session; no graph and no daemon, as an agent waits for it before each search, read or edit |
| `latt-engine` | application | the actor; scanning, watching, editing, reading source, what git says changed, saving the graph for measurements |
| `latt-protocol` | wire | every request and response over the socket |
| `latt-daemon` | transport | the socket server over the engine, its lock and idle timer |
| `latt-client` | client | connecting, starting and restarting the daemon, the socket path |
| `latt-mcp` | transport | the MCP server: tools over the client |
| `latt-view` | transport | `latt view`: the built page and a local web server over the client |
| `latt-view-wire` | wire | the binary form of a snapshot and of events, as the browser gets them; no IO |
| `latt-view-page` | client, in the browser | drawing and laying out the graph, on the GPU; compiled to WebAssembly |
| `lantern-lattice` | transport + composition | the `latt` binary: arguments, output, which adapters run |

Allowed dependencies between crates are listed in `scripts/check-architecture.sh` (`allowed`). The CLI chooses the languages and the semantic
sources, so only `commands/daemon.rs` names them.

The socket is in a private directory under the system's temporary directory, by a hash of the
project's path, because a socket's path is limited to about 100 characters. A daemon holds
`.latt/daemon.lock`, so a project has one. A client that finds a daemon of another version replaces
it. A daemon starts from `.latt/graph/graph.fb` and `references.fb` and reads only the files that changed:
[GRAPH-STORE.md](GRAPH-STORE.md). `.latt/graph/graph.json` is written by `latt sync --json`, for
measurements, and nothing reads it back.

## 4. Rules

1. **The domain is pure.** No IO, `async`, locks over shared mutable state, clock or environment.
2. **One operation, one engine method.** Validation lives in that method and nowhere else.
3. **Transports contain no logic.** A handler parses its wire, calls the client or the engine, and
   maps the result. The CLI and MCP are at parity by construction: both call the same client.
4. **One writer.** Only the engine changes the graph and the saved files. Edits go through it.
5. **Errors are typed.** Each library crate has one `thiserror` enum. `anyhow` only in `lantern-lattice`.
   No `unwrap` or `expect` outside tests, no `#[allow]`, no swallowed `Result`, no silent default
   for invalid input, no `err.to_string().contains(..)`, no `{:?}` in what goes to disk or the wire.
6. **Files stay small.** No source file over 500 lines, tests included; aim for 400. Tests go to a
   sibling `tests.rs` when they do not fit. Cut by responsibility.
7. **Modules have an API.** No `#[path]`, `pub(super)`, `pub(in ...)`, or `super::` paths outside
   `use super::*;` in a test module. An item is `pub` inside a private module, re-exported by its
   parent where callers need it. `pub(crate)` only where nothing else is possible, listed with the
   reason in `scripts/approved-crate-items.txt`.
8. **Every `pub` item has a caller.** Dead code is deleted, never kept for later.
9. **No hidden copies.** Search before adding a function.
10. **The newest packages.** Every dependency is on its latest stable version, checked by
    `scripts/check-deps.py`. Toolchain: `stable`, with `rust-version` as in `Cargo.toml`.

## 4a. What a change touches

`impact` compares each file git says changed with its version in a commit, by parsing both. Each
symbol is compared by its text without the symbols inside it, so lines moved by an edit above change
nothing, and a method added to a type does not change the type. Symbols of one name and kind pair in
written order. One gone from one place and added in another, with the same name, kind and owner, has
moved: with the same text it is not reported, and otherwise it is changed. Dependents are the callers of `trace`, each symbol once across the answer.

A removed symbol has no edges left, and its callers cannot be known from the graph. Reported for it are
the places that still name it and resolve to nothing else, as `still named at`: a claim about a name,
never about a call, so no call target is guessed.

## 5. Resolution

`rust-analyzer scip` writes an index, cached by a hash of the manifests and sources. After an edit
the index is kept, and a running `rust-analyzer` answers for the edited files only; more than 50
edited files, or a changed manifest, writes a new index. A query waits half a second for this and
then answers from syntax rules, marked partial. The analyzer stops after 10 idle minutes.

A proc macro can fail at random while the index is written (`#[napi]` when an `impl` is expanded
before its struct). When the index log names the macro, files with it on an `impl` go to the
running analyzer too. Without `rust-analyzer` only syntax rules apply, and `sync` reports how many
files that covers.

Edge kinds: `Calls` is exact. `UsesAsValue` (function values, constants holding functions) and
`Implements` are possible targets. `UsesConstant` is a read of any other constant. `UsesType` names a
type, trait, alias or variant, and `UsesField` reads, writes or destructures a field; an index does
not tell those apart. All three show when tracing what is used, and not the user. Syntax rules link
a call only when its form allows one target, a type only when it is in the same module, and a field
only as `self.field` in a method of such a type.

## 6. Checks

Work is not done until `scripts/check.sh` passes: `scripts/check-architecture.sh` (and `--new HEAD`),
`scripts/check-file-size.sh`, `scripts/check-version.sh`, `scripts/check-page.sh`, `scripts/check-deps.py`, `cargo deny check`,
`cargo doc` with warnings as errors, `cargo fmt --check`,
`cargo clippy --all-targets -- -D warnings`, `cargo test`. Budgets only go down, to 0.

## 7. The view

`latt view` opens a browser on the project's graph, which changes as the code and the agents do. It
reads and never writes: rule 4 stands, and the page has no way to ask for an edit.

```
  page (WebAssembly, GPU) ── WebSocket ── latt view ── client ── socket ── daemon ── engine
```

- **The engine announces, and never waits.** It publishes events on a bounded broadcast channel: a
  send does not block and costs nothing with no subscriber (T8). A subscriber that falls behind is told
  so and takes a new snapshot; it never slows the actor.
- **Events:** `GraphChanged` (a new generation of the graph, when a link ends, saying whether it is
  provisional) and `Activity` (which symbols a query or an edit touched, without its text).
- **The wire gains two commands.** `Snapshot` answers with every symbol and edge, compactly, and its
  generation. `Watch` turns a connection into a stream of events. A client that watches counts as use,
  so a view keeps the daemon from stopping as idle. The source of a symbol comes through the existing
  `Source`, when the page asks for it.
- **`latt-view` holds no logic** (rule 3): it serves the page, passes snapshots and events on, and
  opens the browser. It does not build or read the graph itself.
- **It listens on `127.0.0.1` only**, on a free port, and answers only with the random token in the
  address it opened, and only to its own origin. Any site open in the browser can reach a local port,
  and the graph shows the names and structure of the code.
- **The browser gets raw arrays**, in the form of `latt-view-wire`, which go into GPU buffers as they
  are. A snapshot goes again whole when the graph changes; a symbol keeps its place by file and name.
- **The built page is in the repository**, in `crates/latt-view/page`, so any way of installing gets
  it without a WebAssembly toolchain. `scripts/build-page.sh` builds it and records a hash of its
  sources; `scripts/check-page.sh` fails if the sources moved on. A release builds it anew from source.
- **`latt view --export DIR` writes the page as files**, with the graph and the source of each
  symbol, for any static host. Such a page has no server: it shows the graph as it was, and nothing
  lights up. It holds the project's source, so it is for what may be public.
- **The page is built into `latt`.** It loads nothing from the network. Layout runs in a compute
  shader; where the browser has no WebGPU the page says so. Each symbol
  starts at a place given by its crate and file and is held there by a weak spring, so the picture
  shows the project's structure and stays where it was between runs.
- **Domain types do not move to the browser** unless they build for it without IO: the page knows the
  wire's snapshot, not `latt-graph`.

## 8. Other languages

Built for JavaScript, TypeScript, Python and Go, and T1 to T8 are measured for Rust, and T1 to T4 for Go. Each is parsed
through tree-sitter, behind the same port as Rust. JavaScript, TypeScript and Python are resolved by a running language
server: `tsrs`, in [TYPESCRIPT.md](TYPESCRIPT.md), and `pyrefly`, in [PYTHON.md](PYTHON.md). Go works as Rust does, with an
index and a server for edited files, in [GO.md](GO.md). A language server is installed apart, and is never linked.
