# Mimir

The call graph of your project for coding agents: who calls a function and what it calls, and edits
by symbol. `x.get()` links to the `get` of `x`'s type, because `rust-analyzer` resolves it. Rust only
for now.

## Use it

```bash
cargo install --path crates/mimir-cli
```

In a git repository, nothing else is needed. The first command starts a daemon, which stops after 30
idle minutes.

```bash
mimir trace -s process_data              # who calls it, what it calls
mimir trace -s process_data --callers    # only who calls it
mimir search --pattern "^process"        # symbols by regular expression
mimir grep "TODO" --path "*.rs"          # text in any file, with the symbol it is in
mimir read -s process_data               # the source of a symbol
mimir files "crates/**/*.rs"             # files by glob, skipping what git ignores
mimir cat src/lib.rs --first 10 --last 40  # a file, numbered
mimir edit --symbol process_data --mode symbol --content "fn process_data() {}"
mimir sync                               # up to date, wait until nothing is provisional
```

In Claude Code, with the plugin in [integrations/claude-code](integrations/claude-code/README.md),
the same operations are tools the model can use: `trace`, `search`, `read`, `edit_symbols`,
`insert_after`, `grep`, `files`, `read_file`, `edit_lines`, `sync`, `status`. For any other MCP client, run `mimir mcp`.

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

On a copy of a project with 1311 Rust files and 20 168 symbols:

| | |
|---|---|
| A query with nothing changed | 0.01 to 0.02 s |
| A first answer after an edit | under 1 s, marked partial until complete |
| A complete answer after the first edit | about 22 s, then under 1 s |
| From no daemon to a first answer | 2.5 s, with the index already written |
| A first index | 1 to 2 minutes |
| Memory | daemon about 200 to 340 MB; `rust-analyzer` 3.7 GB while in use, stopped after 10 idle minutes |

## Accuracy

On Mimir's own code, 1326 of 1326 calls between its functions match what `rust-analyzer` reports. The
measurement says 1325, and the one difference is the measurement's: SCIP gives two functions in
different test binaries one symbol. Every entry of a hand-written list of traps in
[crates/mimir-resolve-ra/tests/fixture.rs](crates/mimir-resolve-ra/tests/fixture.rs) holds too: same
names across types and crates, calls inside macros, trait methods. Scripts behind these numbers are in
[scripts/bench](scripts/bench/README.md).

Known limits: only Rust. A proc macro that fails silently while the index is written loses its calls;
one that says so is handled. Types and fields have no `trace`, only functions and constants; `grep` finds those. A function named in an attribute, as in `#[serde(skip_serializing_if = "name")]`, has no call.

## Develop

Read [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md), the specification. `scripts/check.sh` runs every check.

## License

MIT
