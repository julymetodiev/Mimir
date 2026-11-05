<p align="center">
  <img src="design/icon.svg" width="128" height="128" alt="Lantern Lattice">
</p>

<h1 align="center">Lantern Lattice</h1>

<p align="center"><strong>The call graph from your compiler or type checker, for humans and coding agents</strong></p>

<p align="center">
  <a href="https://github.com/julymetodiev/lantern-lattice/actions/workflows/ci.yml"><img src="https://github.com/julymetodiev/lantern-lattice/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://github.com/julymetodiev/lantern-lattice/releases/latest"><img src="https://img.shields.io/github/v/release/julymetodiev/lantern-lattice" alt="Latest release"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue" alt="MIT license"></a>
</p>

Who calls a function, what it calls, who uses a type or a field, and edits by symbol. `x.get()` links
to the `get` of `x`'s type, because the language's own compiler resolves it, where a text search finds every `get`.
It also has the tools an agent needs instead of Grep, Glob, Read, Edit and Write, so the graph stays up
to date as the agent works.

On macOS and Linux:

| Language | Status | Answers come from |
|---|---|---|
| Rust | Supported | `rust-analyzer` |
| TypeScript | Supported | `tsrs` |
| JavaScript | Supported | `tsrs` |
| Python | Supported | `pyrefly` |
| Go | Supported | `scip-go` and `gopls` |
| C | Supported | `clangd-indexer` and `clangd` |
| C++ | Supported | `clangd-indexer` and `clangd` |
| Java | Next | |

<p align="center">
  <a href="https://lanternlattice.com/"><img src="docs/images/graph.jpg" alt="Lantern Lattice's own code as a graph: symbols grouped by crate and file, joined by calls and uses"></a>
  <br>
  <em>Lantern Lattice's own code: <a href="https://lanternlattice.com/">Live</a></em>
</p>

<p align="center">
  <a href="https://lanternlattice.com"><img src="docs/images/symbol.jpg" alt="A chosen symbol, with what calls it, what it calls and what it uses, and its source"></a>
  <br>
  <em>A chosen symbol: what calls it, what it calls, what it uses, and its source.</em>
</p>

## Install

With [Homebrew](https://brew.sh), on macOS and Linux:

```bash
brew install julymetodiev/tap/latt
```

Or download an archive for your platform from the
[latest release](https://github.com/julymetodiev/lantern-lattice/releases/latest) and put `latt` on your `PATH`.
The binaries are not signed, so on macOS one downloaded in a browser may need
`xattr -d com.apple.quarantine latt`.

Or build it, with Rust 1.95 or newer:

```bash
cargo install --git https://github.com/julymetodiev/lantern-lattice lantern-lattice
```

For exact answers, Lantern Lattice needs `rust-analyzer` for Rust, [`tsrs`](https://github.com/oxc-project/tsrs) for TypeScript and JavaScript, [`pyrefly`](https://github.com/facebook/pyrefly) for Python, `go` with [`scip-go`](https://github.com/scip-code/scip-go) and `gopls` for Go, and `clangd` with `clangd-indexer` for C and C++. In a Python project, install its packages in a `.venv` at the top: without them less is answered. A C or C++ project needs a `compile_commands.json`, at the top or in `build/`, and Lantern Lattice never runs what makes one, as that runs the project's code.

```bash
rustup component add rust-analyzer rust-src
npm install -g tsrs
pipx install pyrefly
go install github.com/scip-code/scip-go/cmd/scip-go@latest
go install golang.org/x/tools/gopls@latest
```

`clangd` comes with `xcode-select --install` on a Mac and with the system's package on Linux. `clangd-indexer` is in `clangd_indexing_tools` of a [release of clangd](https://github.com/clangd/clangd/releases).

## Use it

In a git repository, nothing else is needed. The first command starts a daemon, which stops after 30
idle minutes or when the project is gone.

```bash
latt trace -s process_data              # who calls it, what it calls
latt trace -s process_data --callers    # only who calls it
latt impact                             # what the uncommitted changes touch, and what depends on them
latt impact --base main                 # the same for a whole branch
latt search --pattern "^process"        # symbols by regular expression
latt grep "TODO" --path "*.rs"          # text in any file, with the symbol it is in
latt read -s process_data               # the source of a symbol
latt files "crates/**/*.rs"             # files by glob, skipping what git ignores
latt cat src/lib.rs --first 10 --last 40  # a file, numbered
latt write src/new.rs --content "fn new() {}"  # a new file; --overwrite to replace one
latt edit --symbol process_data --mode symbol --content "fn process_data() {}"
latt edit --file src/lib.rs --mode text --old "a = 1" --content "a = 2"  # exact text, found once
latt sync                               # up to date, wait until nothing is provisional
```

## Connect your agent

Through MCP, the same operations are tools the model can use: `trace`, `impact`, `search`, `read`,
`edit_symbols`, `insert_after`, `grep`, `files`, `read_file`, `edit_lines`, `replace_text`,
`write_file`, `sync`, `status`. The server is `latt mcp`, and it serves the git repository of the
directory it runs in. So for most agents, add it to the project's own settings. `latt` must be on the
`PATH` the agent sees.

### Claude Code

The plugin brings the server and a skill that tells the model when to use it instead of Grep, Glob,
Read, Edit and Write:

```
/plugin marketplace add julymetodiev/lantern-lattice
/plugin install latt@latt
```

Or only the server, without the skill:

```bash
claude mcp add --scope user latt -- latt mcp
```

### Other agents

There is no plugin for these yet: add the server to the file shown. Claude Code is the one agent
Lantern Lattice has been used with so far, so these come from each agent's configuration format and are untested.
If one does not work, please [open an issue](https://github.com/julymetodiev/lantern-lattice/issues).

| Agent | File in the project |
|---|---|
| Cursor | `.cursor/mcp.json` |
| Gemini CLI | `.gemini/settings.json` |
| Roo Code | `.roo/mcp.json` |

```json
{ "mcpServers": { "latt": { "command": "latt", "args": ["mcp"] } } }
```

Codex, in `.codex/config.toml`:

```toml
[mcp_servers.latt]
command = "latt"
args = ["mcp"]
```

VS Code, in `.vscode/mcp.json`:

```json
{ "servers": { "latt": { "type": "stdio", "command": "latt", "args": ["mcp"] } } }
```

Zed, in `.zed/settings.json`:

```json
{ "context_servers": { "latt": { "command": "latt", "args": ["mcp"] } } }
```

OpenCode, in `opencode.json`:

```json
{ "mcp": { "latt": { "type": "local", "command": ["latt", "mcp"], "enabled": true } } }
```

Any other MCP client: run `latt mcp` over stdio in the project's directory. To teach an agent when to
use the tools, put [the skill's text](integrations/claude-code/skills/latt-code/SKILL.md) in its
instructions, such as `AGENTS.md`.

## See it

[Look at Lantern Lattice's own code this way](https://lanternlattice.com/), in a browser with WebGPU. On
your project:

```bash
latt view
```

A browser opens on the whole graph, which changes as the code and the agents do. Symbols are placed by
crate and file. Zoom in, search, or choose a symbol to see what it calls, who calls it and its source.
What a query or an edit touches lights up as it happens, blue for a read and red for an edit, so you can
watch an agent work. It only reads, loads nothing from the network, and listens on `127.0.0.1` with a
private token in the address. It needs a browser with WebGPU; it has been tried in Chrome.

`latt view --export DIR` writes the same page as files for any static host, with the graph as it is and
the source of every symbol, so use it only on code that may be public.

## What an answer means

- A call in `trace` is exact. A call Lantern Lattice cannot be sure of is left out, never guessed.
- `[via trait, possible]` and `[implementation, possible]`: which implementation runs is decided at
  runtime.
- `[as value, possible]`: a function named without being called, as in `.map(helper)`, or a constant
  holding one. It may run later, or never.
- `[reads it]`: on a constant or static, a symbol that reads it.
- `[uses it]`: on a type or field, a symbol that names it. Neither of these is a call, and they come
  only for what you traced, not further along a chain.
- `Partial:` analysis is still running, so calls may be missing. Ask again shortly.

Without `rust-analyzer`, `tsrs`, `pyrefly` or `scip-go`, or with `LATT_NO_SCIP=1`, only syntax rules apply, and `latt
sync` says how many files that covers. In Rust they find about half the calls. In the others they link only
functions of the same file or of an import in the project, and no methods.

## How fast

On a copy of a project with 1311 Rust files and 20 260 symbols, on a laptop:

| | |
|---|---|
| A query with nothing changed | 0.01 to 0.02 s |
| A first answer after an edit | about 1 s (0.8 to 1.1 s), marked partial until complete |
| A complete answer after the first edit | 8 to 12 s, then under 1 s |
| From no daemon to a first answer | 3.2 to 3.8 s, with the index already written |
| A first index | 1 to 2 minutes, at a peak of about 6 GB |
| Memory | daemon about 370 MB idle; `rust-analyzer` about 2.2 GB while in use, stopped after 10 idle minutes |

On TypeScript, JavaScript and Python projects, on the same laptop, with the server stopped after 10 idle
minutes too:

| | hono | nest | three.js | flask | requests | django |
|---|---|---|---|---|---|---|
| Symbols | 12 497 | 27 854 | 27 387 | 2 046 | 1 114 | 67 865 |
| A first `latt sync` | 11 s | 48 s | 45 s | 2.1 s | 1.5 s | 65 s |
| A new daemon with nothing changed | 1.0 s | 2.9 s | 4.2 s | 0.1 s | 0.1 s | 12 s |
| Daemon after asking | 121 MB | 187 MB | 373 MB | 31 MB | 27 MB | 539 MB |
| The server while in use | 963 MB | 1.6 GB | 603 MB | 233 MB | 178 MB | 614 MB |

An edit of a file most of a project imports asks about those files again: about 30 s on nest and django,
with answers marked partial until then.

On Go projects, which work as Rust does, with an index and `gopls` for the files edited since:

| | hugo | kubernetes |
|---|---|---|
| Symbols | 19 065 | 221 808 |
| A first `latt sync`, compiling what the project needs | 331 s | 513 s, and 14 GB in Go's build cache |
| A new daemon with nothing changed | 1.3 to 1.9 s | 32 s |
| A first answer after an edit | under 0.9 s | not measured |
| Daemon after asking | 96 MB | 1.7 GB, and a peak of 4.7 GB while it reads the indexes |

## Accuracy

On Lantern Lattice's own code, 1326 of 1326 calls between its functions match what `rust-analyzer` reports. The
measurement says 1325, and the one difference is the measurement's: SCIP gives two functions in
different test binaries one symbol. Every entry of a hand-written list of traps in
[crates/latt-resolve-ra/tests/fixture/expected.rs](crates/latt-resolve-ra/tests/fixture/expected.rs) holds too: same
names across types and crates, calls inside macros, trait methods. Scripts behind these numbers are in
[scripts/bench](scripts/bench/README.md).

Who uses what, against `rust-analyzer`'s find-references on the project above:

| | Compared | Both agree | Only Lantern Lattice | Only the server |
|---|---|---|---|---|
| Constants and statics | all 944 | 1814 | 46 | 13 |
| Types and traits | a sample of 40 | 420 | 0 | 0 |
| Variants | a sample of 40 | 295 | 0 | 0 |
| Fields | a sample of 40 | 157 | 0 | 0 |

"Only Lantern Lattice" is mostly code a server on this machine does not compile, as behind `#[cfg(...)]` or a
feature. A graph kept up to date through edits had 0 differences from a new index, on edits of comment
lines in 3 files. These are measurements of one project, not a guarantee for yours.

In TypeScript and JavaScript, against `scip-typescript`, the targets inside a project agree at 70 998 of 72 557 places on hono, 57 401 of 58 894 on nest and 91 559 of 108 715 on three.js. Nearly all the rest are places only `tsrs` answers, or the start of a file against its first line; `scripts/bench/scip_compare.py` names each kind. After edits of comments in 2 to 3 files the graph had 0 differences from a clean start, on hono and nest. Agreement is not proof: these two tools can be wrong together.

In Python, against `scip-python`, the targets of places that name a function agree at 739 of 778 on flask and 414 of 434 on requests; on django that comparison did not finish. Most other differences read are local variables assigned twice, which give no edge. `pyrefly` gave the same targets at all 299 205 calls and attributes on django with files asked in reverse order, and after edits of 3 files on django the graph had 0 differences from a clean start. Agreement is not proof.

Known limits in Python: **`pyrefly` runs a project's `.venv/bin/python`** (see [SECURITY.md](SECURITY.md)). What has no type has no target, about a fifth of the calls in django's own code, and Lantern Lattice leaves those out and does not guess. A call through a variable, as `alias()` after `alias = plain`, or `cls()`, gives no edge. A call the source does not spell gives none either, as `a + b` running `__add__`. A class that fits a `Protocol` without naming it is not shown as implementing it. A package installed without a change to a lock file leaves saved answers stale. Environments other than a `.venv` at the top of a project are not measured.

Known limits in TypeScript and JavaScript: what has no type has no target, so plain JavaScript gets about two thirds of its calls (62% on express, 66% on three.js, against 91% and 94% on TypeScript projects). Lantern Lattice leaves the rest out and does not guess. `list[0].area()` on a union of types shows one member. pnpm and yarn workspaces are not measured, and `.vue` and `.svelte` files are left out. `tsrs` is a young beta.

In Go, against `gopls`, all 103 967 places on hugo that name something agree or fall in a known class: 50 369 at the same place, 36 908 outside the project in the same package, and the rest definitions, built-in names and files outside the build.

Known limits in Go: a first index compiles the project, as a build does, and a `go.mod` may make Go fetch the toolchain it names (see [SECURITY.md](SECURITY.md)). A file behind a build tag or for another system has syntax rules only, and `latt sync` says how many there are. A file over 500 KB is left out, in any language. `C.f()` is not linked. A call the source does not spell, as through `reflect`, has no edge, and neither has `f()` on a variable holding a function.

Known limits in Rust: a proc macro that fails silently while the index is written loses its calls;
one that says so is handled. A read and a write of a field are not told apart. A constant inside a format string has a read only in macros known to take one, and only if it is an item of the same module or imported by a project path: an attribute such as `#[error("{NAME}")]`, a glob import or a renamed import leaves it out. A function named in an attribute, as in `#[serde(skip_serializing_if = "name")]`, has no call. Replacing a file by renaming breaks a hard link to it.

Left out on purpose for now: telling a read of a field from a write; keeping hard links when a file is written.

## Develop

Read [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md), the specification, and
[CONTRIBUTING.md](CONTRIBUTING.md). `scripts/check.sh` runs every check.

Security: [SECURITY.md](SECURITY.md). Changes: [CHANGELOG.md](CHANGELOG.md). Releases:
[RELEASING.md](RELEASING.md).

## License

[MIT](LICENSE)
