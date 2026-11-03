---
name: mimir-code
description: Use Mimir's call graph (MCP tools named trace, search, read, edit_symbols, edit_lines, sync, status) when working in Rust code - before changing, renaming or removing a function, when asking who calls something or what it calls, when tracing a bug through calls, when finding a symbol by name, or when replacing a whole function, struct or impl. Prefer it over grep for these. Not for searching text, comments or non-code files, which it does not index.
---

# Mimir

Mimir holds the project's call graph, resolved by `rust-analyzer`, so `x.get()` links to the `get`
of `x`'s type. Grep cannot tell same-named methods apart and misses calls inside macros.

## When to use what

| You want | Tool |
|---|---|
| Who calls this, what it calls, before changing it | `trace` |
| A symbol whose name you partly know (`total` counts all matches, not only those returned) | `search` |
| A symbol's source, without knowing its file | `read` |
| Replace whole functions, structs or impls | `edit_symbols` |
| Replace lines in a file | `edit_lines` |

Text, comments and non-Rust files: use your usual tools.

## Reading a trace

- An entry without `possible` is an exact call.
- `possible: "via trait"` and `"implementation"`: which implementation runs is decided at runtime.
  `"as value"`: a function named without being called, so it may run later or never. Check these
  before relying on them.
- `provisional: true` means analysis is still running and calls may be missing. Ask again shortly;
  never conclude that something has no callers from a provisional answer.
- `others_with_this_name` means several symbols share the name and you got the likeliest. Check
  `at`, and pass `file` for another.
- No caller does not prove a function is dead. It can be called from outside the project, from a
  test behind a `cfg`, or by name in an attribute, as in `#[serde(skip_serializing_if = "is_zero")]`.
  Search the text before deleting one.

## Editing

`edit_symbols` replaces a symbol's lines. Doc comments and attributes above it are not part of it.
Use `preview: true` when unsure. A file changed since it was read is re-read first, so the right
lines are replaced, and if the edit still does not apply it fails rather than guess.

After editing, a `trace` shows the new calls at once. `sync` is rarely needed; changes are followed
by themselves.
