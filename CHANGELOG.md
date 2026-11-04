# Changelog

All notable changes to Mimir are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **`impact`**: what the uncommitted changes touch, or all of a branch with `--base`: the symbols added,
  changed and removed, and what depends on them. A removed symbol shows where its name is still written.
- **The Claude Code plugin steers an agent to Mimir**: the first search or read of a project in a session
  is refused once, with what to use instead, and repeating the call goes through. A search through the
  shell is reminded of it once. A `mimir` too old for this changes nothing.

## [0.4.0] - 2026-11-04

### Added

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
