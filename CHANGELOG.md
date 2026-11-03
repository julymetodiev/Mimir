# Changelog

All notable changes to Mimir are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
