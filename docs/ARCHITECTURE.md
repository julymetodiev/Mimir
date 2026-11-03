# Mimir architecture

The specification of the system (rule 2 of the law in `CLAUDE.md`: code follows it exactly). Code
cites it by section number. Why a decision was made is in git and in project memory, not here.

## 1. The system

Mimir gives coding agents an understanding of code: who calls what, where a symbol is, and safe
edits by symbol. It is one binary, `mimir`, with no daemon to manage and no language model inside.

```
  Claude Code ── plugin ── mimir mcp ─┐
                                      ├── client ── socket ── daemon ── engine
  a person ─────────────── mimir ... ─┘
```

- **One daemon per project**, started by the first command that needs it and stopped when idle.
  There is no other path: no command builds or reads the graph in its own process.
- **Why a daemon:** measured on 1311 files, `trace` takes 0.02 s through it and 2.2 s without it.
  After an edit, a process of its own pays for a new `rust-analyzer` on every query (25 s, 3.7 GB);
  the daemon keeps one running.
- **Calls are never guessed.** What cannot be known is left out and reported. Calls stay separate
  from possible targets (trait implementations, functions as values), which `trace` marks.

## 2. Targets

Each is a number `scripts/bench` can check, on a project of 20 000 symbols.

| # | Target |
|---|---|
| T1 | A query with nothing changed: under 50 ms |
| T2 | A query after an edit: first answer under 1 s, marked partial until complete |
| T3 | From no daemon to a first answer, with the graph saved: under 5 s |
| T4 | Daemon idle, without the analyzer: under 400 MB |
| T5 | Call edges: nothing lost against the SCIP index; every entry of the hand-written list in `crates/mimir-resolve-ra/tests/fixture.rs` holds |
| T6 | After edits, the graph equals the graph from a new index: 0 differences |

## 3. Layers

Dependencies point inwards. IO lives at the edge.

```
  transports    mimir-cli, mimir-mcp, mimir-daemon     map a request to one engine call, nothing else
  client        mimir-client                           connect, start the daemon, one call per command
  wire          mimir-protocol                         requests and responses, typed
  application   mimir-engine                           the one writer: sync, save, watch, query, edit
  adapters      mimir-resolve-ra, mimir-edit           rust-analyzer, files on disk
  ports + rules mimir-resolve                          linking rules and the port for a semantic source
  domain        mimir-graph, mimir-parse-core,         pure: no IO, no async, no clock, no environment
                mimir-parse-rust, mimir-types
```

| Crate | Layer | Owns |
|---|---|---|
| `mimir-types` | domain | ids, symbols, references, locations, languages |
| `mimir-parse-core` | domain | the language adapter trait and what it extracts |
| `mimir-parse-rust` | domain | Rust through tree-sitter |
| `mimir-graph` | domain | symbols, edges and queries over them |
| `mimir-resolve` | rules + port | turning references into edges; syntax rules; the port `SemanticSource` |
| `mimir-resolve-ra` | adapter | the SCIP index and the running `rust-analyzer` |
| `mimir-edit` | adapter | applying edits to files, with conflict detection |
| `mimir-engine` | application | the actor; scanning, watching, saving the graph, the idle timer |
| `mimir-protocol` | wire | every request and response over the socket |
| `mimir-daemon` | transport | the socket server over the engine |
| `mimir-client` | client | connecting, starting and restarting the daemon, the socket path |
| `mimir-mcp` | transport | the MCP server: tools over the client |
| `mimir-cli` | transport + composition | the `mimir` binary: arguments, output, which adapters run |

Allowed dependencies between crates are listed in `scripts/check-architecture.sh` (`allowed`).

## 4. Rules

1. **The domain is pure.** No IO, `async`, locks over shared mutable state, clock or environment.
2. **One operation, one engine method.** Validation lives in that method and nowhere else.
3. **Transports contain no logic.** A handler parses its wire, calls the client or the engine, and
   maps the result. The CLI and MCP are at parity by construction: both call the same client.
4. **One writer.** Only the engine changes the graph and the saved files. Edits go through it.
5. **Errors are typed.** Each library crate has one `thiserror` enum. `anyhow` only in `mimir-cli`.
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
`Implements` are possible targets. Syntax rules link a call only when its form allows one target.

## 6. Checks

Work is not done until these pass: `scripts/check-architecture.sh` (and `--new HEAD`),
`scripts/check-file-size.sh`, `scripts/check-deps.py`, `cargo fmt --check`,
`cargo clippy --all-targets -- -D warnings`, `cargo test`. Budgets only go down, to 0.
