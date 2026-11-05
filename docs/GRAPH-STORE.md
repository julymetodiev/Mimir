# The graph on disk

Status: built, for every language. What is not built is in section 7.

## 1. Why

The graph lived only in the daemon's memory, and each start parsed every file and linked every edge. The daemon stops after 30 idle
minutes, so this recurred. `SymbolId` came from a counter, so an id meant nothing after a restart.

## 2. The files

Two files in `.mimir/graph`, FlatBuffers, each read whole into one buffer and checked once:

- `graph.fb`: symbols, names, files and edges. The schema is `crates/mimir-graph-fb/graph.fbs`.
- `references.fb`: the references of each file with a hash of its text. The schema is `crates/mimir-graph-fb/references.fbs`.

`symbols` is sorted by id, so an id is found by binary search. `from` and `to` in an edge are positions in `symbols`; `edges` is sorted
by `from`, so a symbol's edges are one slice, and `incoming` does the same the other way. A name and a qualified name are found by binary
search.

References are apart from the graph so that a change to the graph does not write them, as the graph is rebuilt on each edit and they are
needed only by a link.

Each file is written to a new file and renamed. A file of another format version, of another version of Mimir, or broken, is treated as no
file: it is built and written again.

## 3. Ids

A `FileId` is a hash of the path relative to the project. A `SymbolId` is a hash of the file's id, the qualified name, the kind's name and
the order among symbols of that file with the same name and kind. Two ids that come out the same stay two symbols: the second takes the next
free id, which then depends on which came first, until the file holds it.

## 4. Reading

`CodeGraph` holds one buffer, the same bytes as the file. The offsets are checked by the safe `root_as_graph_with_opts`, and positions by
`check` in `mimir-graph/src/file.rs`, because a position out of range would otherwise stop a query. No symbol is copied for that.

A query builds a `Symbol` only for what it returns: its answer copies these strings anyway. Nothing is built at start.

The generated code has the `unsafe` it takes to read bytes as numbers, and is in `mimir-graph-fb`, so no other crate needs any. `self_cell`
keeps the checked bytes and the view on them together. The generated code is in the repository, so that building needs no `flatc`;
`scripts/generate-graph-fb.sh --check` compares it with the schema.

`mmap` was measured and not taken: on a prototype at 342 144 symbols, 65 MB, read and verify took 70.7 ms against 62 ms, the same queries were
no slower, it loads every page anyway when it verifies, and a program changing a mapped file after the check would break what the check
promised. The buffer stays in memory, which the system could not drop as it can pages of a map.

## 5. Start, edit and link

- **Start:** read the file. A file on disk whose size and time equal those stored is taken as unchanged, and only the others are read and
  hashed. A tool that changes a file and keeps its size and time is missed until `sync --full`. With nothing changed there is no link and no
  parse: edges come from the file.
- **Edit:** the file's symbols change in the buffer at once. FlatBuffers do not change in place, so this builds a new buffer of the whole
  graph, and several files at once take one step. There is no second layer in memory.
- **Link:** still all files. Before the first link of a run, the text of every file is read, and references are taken from `references.fb`
  where the hash of the text is the same; only the others are parsed. `graph.fb` and `references.fb` are written after a link that finished,
  unless files changed meanwhile or indexing failed.
- A tool installed after the last link does not improve edges until `sync --full`, as a start with no change does not link.
- `graph.json` is written only by `mimir sync --json`, for `scripts/bench`.

## 6. Measured

On an M1 Pro, one run each, with an index already built:

| | yaml-cpp | curl |
|---|---|---|
| Symbols, edges | 13 554, 39 384 | 15 549, 64 245 |
| `graph.fb`, `references.fb` | 2.3 MB, 4.6 MB | 4.3 MB, 7.8 MB |
| First query after a stop | 0.02 s | 0.03 s |
| First edit, with references from the file | 0.50 s | 0.51 s |
| First edit, parsing all files | 0.95 s | 1.26 s |
| A later edit | 0.11 s | 0.33 s |

The first edit with and without the file was measured on different files of curl, so its row is a guide there, not a comparison. Nothing was
run at LLVM's size. On the prototype, curl repeated 22 times, as a graph of LLVM's size, took 1.4 s to build and write; that repeats one
project's shape, so it is no measurement of LLVM.

On `ml` (Linux, 10 cores), the same binaries, files and queries, against the counter's binary. A median of three, except the first query of the
new one, which is its one run with nothing changed. Memory is the project's daemon alone:

| | leveldb | yaml-cpp | fmt | curl |
|---|---|---|---|---|
| First query after a stop | 1.97 s, now 0.04 s | 1.38 s, now 0.03 s | 0.72 s, now 0.03 s | 2.98 s, now 0.07 s |
| Memory after it | 140 MB, now 18 MB | 107 MB, now 17 MB | 59 MB, now 16 MB | 172 MB, now 20 MB |
| First edit | 0.65 s, now 0.75 s | 0.77 s, now 0.74 s | 0.14 s, now 0.16 s | 0.88 s, now 0.86 s |
| Memory after it | 197 MB, now 104 MB | 218 MB, now 125 MB | 90 MB, now 62 MB | 326 MB, now 237 MB |
| A later edit | 0.68 s, now 0.67 s | 0.28 s, now 0.35 s | 0.14 s, now 0.16 s | 0.64 s, now 0.72 s |

A later edit is up to a quarter slower, as it builds the whole buffer. With a file changed while the daemon was stopped, the first query took
0.40 s on yaml-cpp, 0.22 s on fmt and 0.74 s on curl. All edges were the same as the counter's on these four.

## 7. Not built

- **Linking only what changed.** At these sizes a link is about 0.1 s, and it can change edges, so it waits for a project that shows the need.
- **Text out of memory after a link.** Every edit would then read all files again: the first edit above against a later one. The text is not
  in memory after a start, only from the first link until the daemon stops.
- **Building a new buffer without copying every symbol.** Unmeasured at LLVM's size.
- **`fill_local_symbols`: several `clangd`, with progress.** One, in order, silent. Over 20 minutes on LLVM, not finished, with the bug of
  reads counted as changes, since fixed.
- **`kept_definitions` once.** Reads and decodes the whole index for each answer of `clangd`.
- **`Dex`: shared paths and ids.** One `String` per reference.
- **A part of LLVM, as `llvm/lib/Support`, and all of LLVM.** Only on the owner's word.
