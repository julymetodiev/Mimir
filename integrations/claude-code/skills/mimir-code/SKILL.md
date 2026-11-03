---
name: mimir-code
description: Use Mimir instead of Grep, Glob, Read, Edit and Write in a project (MCP tools named grep, files, read_file, trace, search, read, edit_symbols, insert_after, edit_lines, replace_text, write_file, sync, status) - for searching text in any file, finding files by name, reading a file, creating a file, before changing, renaming or removing a Rust function, when asking who calls something or what it calls, when tracing a bug through calls, when finding a symbol by name, or when replacing a function, struct or impl or adding code next to one.
---

# Mimir

Mimir holds the project's call graph, resolved by `rust-analyzer`, so `x.get()` links to the `get`
of `x`'s type. Plain text search cannot tell same-named methods apart, so use `trace` for calls and `grep` for text.

## When to use what

| You want | Tool |
|---|---|
| Text anywhere in the project, in any language; each match says which symbol it is in | `grep` |
| Files by name or glob, skipping what git ignores | `files` |
| A whole file or a range of its lines, numbered for `edit_lines` | `read_file` |
| Who calls this, what it calls, before changing it | `trace` |
| A symbol whose name you partly know (`total` counts all matches, not only those returned) | `search` |
| A symbol's source, without knowing its file | `read` |
| Replace whole functions, structs or impls | `edit_symbols` |
| Add a function, method, variant or field after an existing symbol | `insert_after` |
| Change a few words or lines in any file, without line numbers | `replace_text` |
| Replace lines in a file | `edit_lines` |
| Create a file, in any language; `overwrite: true` to replace one | `write_file` |

Outside the project, use your usual tools: Mimir reads only the project's files.

## Reading a trace

- An entry without `possible` is an exact call.
- `possible: "via trait"` and `"implementation"`: which implementation runs is decided at runtime.
  `"as value"`: a function named without being called, so it may run later or never. Check these
  before relying on them.
- `provisional: true` means analysis is still running and calls may be missing. Ask again shortly;
  never conclude that something has no callers from a provisional answer.
- `others_with_this_name` means several symbols share the name and you got the likeliest. Check
  `at`, and pass `file` for another.
- For a constant, type, trait, variant or field, `callers` are the symbols that use it, including
  `impl` blocks and the fields of other types. A read and a write of a field are not told apart.
  A function's `callees` do not list the constants, types or fields it uses. When several share a
  name, pass `file` or a qualified name such as `Config::limit`.
- A constant named only inside a format string, as in `format!("{NAME}")`, shows no reader.
- No caller does not prove a function is dead. It can be called from outside the project, from a
  test behind a `cfg`, or by name in an attribute, as in `#[serde(skip_serializing_if = "is_zero")]`.
  Search the text before deleting one.

## Editing

`edit_symbols` replaces a symbol's lines. Doc comments and attributes above it are not part of it.
When several symbols share a name, such as `new`, it fails and lists them: pass `file`, or a
qualified name such as `Kind::Variant`. `insert_after` adds code below a symbol the same way.
`replace_text` needs the exact text, with enough around it to occur once; otherwise it fails and
names the lines. With several edits, if one does not apply, no file changes. Use `preview: true` when unsure. A file changed since it was read is re-read first, so the right
lines are replaced, and if the edit still does not apply it fails rather than guess.

After editing, a `trace` shows the new calls at once. `sync` is rarely needed; changes are followed
by themselves.
