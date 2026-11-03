---
name: mimir-code
description: Use Mimir instead of grep and for code navigation (MCP tools named grep, trace, search, read, edit_symbols, insert_after, edit_lines, sync, status) - for searching text in any file of the project, before changing, renaming or removing a Rust function, when asking who calls something or what it calls, when tracing a bug through calls, when finding a symbol by name, or when replacing a function, struct or impl or adding code next to one.
---

# Mimir

Mimir holds the project's call graph, resolved by `rust-analyzer`, so `x.get()` links to the `get`
of `x`'s type. Plain text search cannot tell same-named methods apart, so use `trace` for calls and `grep` for text.

## When to use what

| You want | Tool |
|---|---|
| Text anywhere in the project, in any language; each match says which symbol it is in | `grep` |
| Who calls this, what it calls, before changing it | `trace` |
| A symbol whose name you partly know (`total` counts all matches, not only those returned) | `search` |
| A symbol's source, without knowing its file | `read` |
| Replace whole functions, structs or impls | `edit_symbols` |
| Add a function, method, variant or field after an existing symbol | `insert_after` |
| Replace lines in a file | `edit_lines` |

Finding files by name, and reading a whole file: use your usual tools.

## Reading a trace

- An entry without `possible` is an exact call.
- `possible: "via trait"` and `"implementation"`: which implementation runs is decided at runtime.
  `"as value"`: a function named without being called, so it may run later or never. Check these
  before relying on them.
- `provisional: true` means analysis is still running and calls may be missing. Ask again shortly;
  never conclude that something has no callers from a provisional answer.
- `others_with_this_name` means several symbols share the name and you got the likeliest. Check
  `at`, and pass `file` for another.
- For a constant or static, `callers` are the symbols that read it. A function's `callees` do not
  list the plain constants it reads.
- No caller does not prove a function is dead. It can be called from outside the project, from a
  test behind a `cfg`, or by name in an attribute, as in `#[serde(skip_serializing_if = "is_zero")]`.
  Search the text before deleting one.

## Editing

`edit_symbols` replaces a symbol's lines. Doc comments and attributes above it are not part of it.
When several symbols share a name, such as `new`, it fails and lists them: pass `file`, or a
qualified name such as `Kind::Variant`. `insert_after` adds code below a symbol the same way.
Use `preview: true` when unsure. A file changed since it was read is re-read first, so the right
lines are replaced, and if the edit still does not apply it fails rather than guess.

After editing, a `trace` shows the new calls at once. `sync` is rarely needed; changes are followed
by themselves.
