<p align="center">
  <img src="design/icon.svg" width="128" height="128" alt="Mimir">
</p>

<h1 align="center">Mimir</h1>

<p align="center"><strong>The call graph of your project for coding agents</strong></p>

<p align="center">
  <a href="https://github.com/julymetodiev/Mimir/actions/workflows/ci.yml"><img src="https://github.com/julymetodiev/Mimir/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://github.com/julymetodiev/Mimir/releases/latest"><img src="https://img.shields.io/github/v/release/julymetodiev/Mimir" alt="Latest release"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue" alt="MIT license"></a>
</p>

Who calls a function, what it calls, who uses a type or a field, and edits by symbol. `x.get()` links
to the `get` of `x`'s type, because `rust-analyzer` resolves it, where a text search finds every `get`.
It also has the tools an agent needs instead of Grep, Glob, Read, Edit and Write, so the graph stays up
to date as the agent works.

Rust only for now, on macOS and Linux.

## Install

Download an archive for your platform from the
[latest release](https://github.com/julymetodiev/Mimir/releases/latest) and put `mimir` on your `PATH`.
Or build it, with Rust 1.95 or newer:

```bash
cargo install --git https://github.com/julymetodiev/Mimir mimir-cli
```

For exact answers, Mimir needs `rust-analyzer`:

```bash
rustup component add rust-analyzer rust-src
```

## Use it

In a git repository, nothing else is needed. The first command starts a daemon, which stops after 30
idle minutes or when the project is gone.

```bash
mimir trace -s process_data              # who calls it, what it calls
mimir trace -s process_data --callers    # only who calls it
mimir search --pattern "^process"        # symbols by regular expression
mimir grep "TODO" --path "*.rs"          # text in any file, with the symbol it is in
mimir read -s process_data               # the source of a symbol
mimir files "crates/**/*.rs"             # files by glob, skipping what git ignores
mimir cat src/lib.rs --first 10 --last 40  # a file, numbered
mimir write src/new.rs --content "fn new() {}"  # a new file; --overwrite to replace one
mimir edit --symbol process_data --mode symbol --content "fn process_data() {}"
mimir edit --file src/lib.rs --mode text --old "a = 1" --content "a = 2"  # exact text, found once
mimir sync                               # up to date, wait until nothing is provisional
```

In Claude Code, with the plugin in [integrations/claude-code](integrations/claude-code/README.md),
the same operations are tools the model can use: `trace`, `search`, `read`, `edit_symbols`,
`insert_after`, `grep`, `files`, `read_file`, `edit_lines`, `replace_text`, `write_file`, `sync`, `status`. For any other MCP client, run `mimir mcp`.

## What an answer means

- A call in `trace` is exact. A call Mimir cannot be sure of is left out, never guessed.
- `[via trait, possible]` and `[implementation, possible]`: which implementation runs is decided at
  runtime.
- `[as value, possible]`: a function named without being called, as in `.map(helper)`, or a constant
  holding one. It may run later, or never.
- `[reads it]`: on a constant or static, a symbol that reads it.
- `Partial:` analysis is still running, so calls may be missing. Ask again shortly.

Without `rust-analyzer`, or with `MIMIR_NO_SCIP=1`, only syntax rules apply. They find about half the
calls, and `mimir sync` says how many files that covers.

## How fast

On a copy of a project with 1311 Rust files and 20 260 symbols, on a laptop:

| | |
|---|---|
| A query with nothing changed | 0.01 to 0.02 s |
| A first answer after an edit | about 1 s (0.8 to 1.1 s), marked partial until complete |
| A complete answer after the first edit | 8 to 12 s, then under 1 s |
| From no daemon to a first answer | 3.2 to 3.8 s, with the index already written |
| A first index | 1 to 2 minutes, at a peak of about 6 GB |
| Memory | daemon about 270 MB idle; `rust-analyzer` about 2.2 GB while in use, stopped after 10 idle minutes |

## Accuracy

On Mimir's own code, 1326 of 1326 calls between its functions match what `rust-analyzer` reports. The
measurement says 1325, and the one difference is the measurement's: SCIP gives two functions in
different test binaries one symbol. Every entry of a hand-written list of traps in
[crates/mimir-resolve-ra/tests/fixture/expected.rs](crates/mimir-resolve-ra/tests/fixture/expected.rs) holds too: same
names across types and crates, calls inside macros, trait methods. Scripts behind these numbers are in
[scripts/bench](scripts/bench/README.md).

Who uses what, against `rust-analyzer`'s find-references on the project above:

| | Compared | Both agree | Only Mimir | Only the server |
|---|---|---|---|---|
| Constants and statics | all 944 | 1814 | 46 | 13 |
| Types and traits | a sample of 40 | 420 | 0 | 0 |
| Variants | a sample of 40 | 295 | 0 | 0 |
| Fields | a sample of 40 | 157 | 0 | 0 |

"Only Mimir" is mostly code a server on this machine does not compile, as behind `#[cfg(...)]` or a
feature. A graph kept up to date through edits had 0 differences from a new index, on edits of comment
lines in 3 files. These are measurements of one project, not a guarantee for yours.

Known limits: only Rust. A proc macro that fails silently while the index is written loses its calls;
one that says so is handled. A read and a write of a field are not told apart. A constant inside a format string has a read only in macros known to take one, and only if it is an item of the same module or imported by a project path: an attribute such as `#[error("{NAME}")]`, a glob import or a renamed import leaves it out. A function named in an attribute, as in `#[serde(skip_serializing_if = "name")]`, has no call. Replacing a file by renaming breaks a hard link to it.

Left out on purpose for now: telling a read of a field from a write; languages other than Rust; keeping hard links when a file is written.

## Develop

Read [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md), the specification, and
[CONTRIBUTING.md](CONTRIBUTING.md). `scripts/check.sh` runs every check.

Security: [SECURITY.md](SECURITY.md). Changes: [CHANGELOG.md](CHANGELOG.md). Releases:
[RELEASING.md](RELEASING.md).

## License

[MIT](LICENSE)
