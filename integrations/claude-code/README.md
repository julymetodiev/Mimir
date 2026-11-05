# Lantern Lattice for Claude Code

Connects Claude Code to [Lantern Lattice](../../README.md), the call graph of a project. It bundles:

- **MCP server** `latt`, the `latt mcp` command, which runs in the project's directory.
- **Skill** `latt-code`: when to use it instead of searching text.
- **Hook**: the first `Read`, `Grep` or `Glob` of a git repository in a session is refused once, with
  what to use instead, unless the session has used Lantern Lattice already. Repeating the call goes through. Images,
  PDFs and files outside the repository are left alone. A search through `Bash` is reminded of it once and
  never refused. It reads no graph and takes about 15 ms; a `latt` too old for it changes nothing.

## Install

The `latt` binary must be on your `PATH`: see [how to install it](../../README.md#install).

Then in Claude Code:

```
/plugin marketplace add julymetodiev/lantern-lattice
/plugin install latt@latt
```

Nothing else to start. The first tool call in a git repository starts Lantern Lattice's daemon, which stops
after 30 idle minutes. The first index of a project takes a minute or more; until it is done, an
answer is marked provisional.
