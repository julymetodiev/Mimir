# Changelog

All notable changes to Mimir are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.6.2] - 2026-11-05

### Changed

- The site at https://lanternlattice.com/ is published to Cloudflare after a release, instead of to GitHub Pages
  by a workflow of this repository.

### Fixed

- **A search right after an edit sees the edit.** The file watcher read a file when it heard of a write and
  handed the text on later, so an edit taken in meanwhile could be replaced by the text before it: a symbol
  just added was missing, or one just removed came back, until the next change of that file. The watcher now
  hands on only the path, and the graph reads the file when it comes to it. Under load, 10 of 120 runs of the
  end-to-end test failed before, and none of 240 after.
- The README installs with `brew install julymetodiev/tap/lantern-lattice`, the formula's name since 0.6.0:
  `julymetodiev/tap/latt` is no formula.

## [0.6.1] - 2026-11-05

### Changed

- **The Claude Code plugin refuses the first edit with `Edit` or `Write` of a project's file in a session
  once**, with Lantern Lattice's tools to use instead, which keep the graph up to date. The first read of the
  project through the shell is refused once too, instead of a reminder an agent could pass by: before, a
  session that never used Lantern Lattice was never refused a read through the shell. Repeating the call
  goes through, as before.

- **The site is at https://lanternlattice.com/**, and https://latt.369.is/ leads there.

### Fixed

- **`latt view` starts again.** In 0.6.0 the page stopped at `WebAssembly.Table.set(): Argument 1 is invalid
  for table`: the release built it with binaryen 108, which named the table of functions as the table of
  JavaScript values. A page is now built only with the binaryen that `scripts/install-binaryen.sh` names, 133,
  and `scripts/build-page.sh` refuses another.
- The Claude Code plugin no longer takes a command after `cd $DIR` or `cd ~/...` for a read of the project: where
  those lead only the shell can say.

## [0.6.0] - 2026-11-05

### Changed

- **Mimir is now Lantern Lattice**, and its command `latt`: the old name is that of many other projects. The
  repository is `lantern-lattice`, the Homebrew formula too, the plugin and its tools are `latt`, and
  `MIMIR_NO_SCIP` is `LATT_NO_SCIP`. A project is indexed anew in `.latt`, and its `.mimir` can be removed.
  `brew upgrade` follows the rename of the formula, and the site is at https://latt.369.is/. The
  plugin has a new name, so remove `mimir` and install `latt` from the marketplace at its new address.

## [0.5.0] - 2026-11-05

### Added

- **C and C++**, resolved by an index from `clangd-indexer` over the project's `compile_commands.json`, and by
  a running `clangd` for edited files, with syntax rules otherwise. A sync says how many compile commands
  failed. Measured up to 5 million lines of LLVM: a new index takes 25 minutes on 10 cores and 8.4 GB, a first
  query after a stop 1.3 s and a later edit 9 s.

### Changed

- **The graph is kept in FlatBuffers files and read from disk.** After a stop, a first query on a project of
  100 000 symbols takes 0.16 s instead of 6.1 s and holds 37 MB instead of 356 MB. A later edit is about as
  fast as before or faster. Nothing has to be indexed again: a graph from before this is built anew once.
- A bare name in `trace` and `read` prefers a symbol outside the tests, then the one most called, then the
  first by path.
- **The Claude Code plugin refuses reading the project through the shell** every time once a session has used
  Mimir, which shows that its tools work; before that it reminds once. Filters after a pipe, paths outside the
  project, builds and tests go through, and the Read tool stays for what Mimir does not show.
- `graph.json` is written only by `mimir sync --json`.
- `tsrs` 0.22.0 and `pyrefly` 1.4.1.

### Fixed

- A stopped daemon went on until its index was written, and the next command started a second one writing
  the same files. A stop now ends every tool and server of the daemon, for all languages.
- A sync longer than 30 minutes, or an index still being written after a query, no longer counts as idle.
- A daemon whose graph actor failed stays no more: it stops, and the next command starts a new one.
- A command cut off by a daemon's stop says so and what to do, instead of that the graph engine has stopped.
- On Linux, opening or reading a file no longer counts as a change, nor does a full scan.
- `mimir trace | head` and the like end quietly, with exit code 141, instead of a panic.
- An exported view no longer calls the code in it Mimir's own.

### Known issues

- `clangd-indexer` differs a little from run to run when it works in parallel, so two new indexes of a project
  can differ by a few edges (`docs/GRAPH-STORE.md`, section 6).

## [0.4.1] - 2026-11-04

### Added

- **`impact`**: what the uncommitted changes touch, or all of a branch with `--base`: the symbols added,
  changed and removed, and what depends on them. A removed symbol shows where its name is still written.
- **The Claude Code plugin steers an agent to Mimir**: the first search or read of a project in a session
  is refused once, with what to use instead, and repeating the call goes through. A search through the
  shell is reminded of it once. A `mimir` too old for this changes nothing.
- **Go**, resolved by an index from `scip-go` for each module, and by a running `gopls` for edited files.
- **Python**, resolved by a running `pyrefly`, with its answers saved beside the graph.

### Changed

- Answers saved by 0.3.0 are asked for once again, because their format has a new name.
- `trace`, `read` and `edit_symbols` find a method by its type, as `Known::add` for `impl Known<'a>::add`.
- `trace` over MCP marks what reads or uses a symbol with `not_a_call`, where it read as an exact call.
- `trace` follows reads and uses only from what is traced, and calls further along a chain. On django
  `save` to depth 3 goes from 10 121 symbols to 277.
- `sync --full` starts a new language server for Python and TypeScript.
- Python: an edit whose lines other files depend on starts a new `pyrefly`, because the running one keeps
  old lines through a file in between. The first `trace` after an edit on django takes 0.6 to 1.1 s, and
  beyond 500 ms it answers as provisional until the server has finished.

### Fixed

- `trace` gives each symbol its shortest depth and the same answer every time. It could drop callers,
  by an order of edges that changed with each edit.
- Python: an edit that moves lines no longer loses calls to members inherited through another file.
- Rust and Go: after an edit that moved or added a method with a name others in its file have, a call from
  another file could go to the wrong one until the next index. A method is now told apart by what it
  belongs to, and one that cannot be matched gets no edge, never a guess.

### Known issues

- Go: until the next index, the implementations of an edited file come from `gopls` and the rest from
  `scip-go`, which do not list the same pairs. On hugo, 3 files edited: 63 extra and 21 missing of about
  1 650 `Implements` edges, and 0 differences in calls.

## [0.3.0] - 2026-11-03

### Added

- **`mimir view --export DIR`**: the view as files for a static host, with the graph and its source.
- **TypeScript and JavaScript**, resolved by a running `tsrs`, with its answers saved beside the graph.

## [0.2.0] - 2026-11-03

### Added

- **Homebrew**: `brew install julymetodiev/tap/mimir`.
- **`mimir view`**: a browser on the whole graph, which changes as the code and the agents do. Symbols
  are placed by crate and file and drawn on the GPU. Zoom, search, and choose a symbol to see what it
  calls, who calls it and its source. What a query or an edit touches lights up as it happens. It reads
  and never writes, listens only on `127.0.0.1`, and needs a browser with WebGPU.

### Fixed

- A daemon someone is watching no longer stops as idle.

## [0.1.0] - 2026-11-03

First public release: Rust projects, on macOS and Linux.

### Added

- **`trace`**: who calls a function and what it calls across a project, from `rust-analyzer`. A call
  Mimir cannot be sure of is left out, never guessed. Possible targets, such as trait implementations
  and functions named as values, are marked as possible.
- **Who uses what**: `trace` on a type, trait, alias, variant, field, constant or static shows the
  symbols that use it.
- **Answers that stay fast after edits**: a daemon keeps the graph, answers at once from syntax rules
  while analysis runs, and marks those answers as provisional. A running analyzer resolves edited
  files, and the result matches a new index.
- **Tools that replace Grep, Glob, Read, Edit and Write for an agent**: `grep`, `files`, `read_file`,
  `read`, `search`, `edit_symbols`, `insert_after`, `edit_lines`, `replace_text`, `write_file`.
- **Safe edits**: a file is written whole or not at all, a batch writes nothing until every edit
  applies, and an edit by symbol fails rather than change the wrong lines if the file moved on.
- **MCP server** (`mimir mcp`) and a **Claude Code plugin** with a skill.
- **A daemon you do not manage**: the first command starts it. It stops after 30 idle minutes, or when
  its project is gone, and a rebuild replaces it.
- **Measurements** in `scripts/bench`, which compare the graph with `rust-analyzer`.
