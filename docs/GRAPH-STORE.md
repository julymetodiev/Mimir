# The graph on disk

Status: built, for every language. What is not built is in section 7.

## 1. Why

The graph lived only in the daemon's memory, and each start parsed every file and linked every edge. The daemon stops after 30 idle
minutes, so this recurred. `SymbolId` came from a counter, so an id meant nothing after a restart.

## 2. The files

Three files in `.mimir/graph`, FlatBuffers, each read whole into one buffer and checked once:

- `graph.fb`: symbols, names and files. The schema is `crates/mimir-graph-fb/graph.fbs`.
- `edges.fb`: the edges, with a stamp, a hash of the ids of the symbols they are between. The schema is `crates/mimir-graph-fb/edges.fbs`.
- `references.fb`: the references of each file with a hash of its text. The schema is `crates/mimir-graph-fb/references.fbs`.

`symbols` is sorted by id, so an id is found by binary search. `from` and `to` in an edge are positions in `symbols`; `edges` is sorted
by `from`, so a symbol's edges are one slice, and `incoming` does the same the other way. A name and a qualified name are found by binary
search.

Edges are apart from the symbols so that a link writes no symbol: encoding every symbol took 520 ms at 101 304 symbols, and the edges 7 ms. Edges
whose stamp is not that of the symbols are refused, so a crash between two writes leaves the symbols and asks for a link. References are apart
so that a change to the graph does not write them, as they are needed only by a link.

Each file is written to a new file and renamed. A file of another format version, of another version of Mimir, or broken, is treated as no
file: it is built and written again.

## 3. Ids

A `FileId` is a hash of the path relative to the project. A `SymbolId` is a hash of the file's id, the qualified name, the kind's name and
the order among symbols of that file with the same name and kind. Two ids that come out the same stay two symbols: the second takes the next
free id, which then depends on which came first, until the file holds it.

## 4. Reading

`CodeGraph` holds the buffers, the same bytes as the files. The offsets are checked by the safe `root_as_graph_with_opts`, and positions by
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
- **Edit:** the file's symbols change at once. FlatBuffers do not change in place, so this builds new buffers of all symbols and edges, and
  several files at once take one step. A link builds only the edges. There is no second layer in memory.
- **Link:** still all files. Before the first link of a run, the text of every file is read, and references are taken from `references.fb`
  where the hash of the text is the same; only the others are parsed. `graph.fb` and `references.fb` are written after a link that finished,
  unless files changed meanwhile or indexing failed. A thread of its own writes them, so no query waits, and of several waiting only the last.
  References are encoded there too, the larger part of the cost: 24 ms on yaml-cpp and 50 ms on curl, against 1 to 6 ms to write `graph.fb`.
  What a crash loses, the next start sees by the hash of each file. Stopping waits for what is left to write.
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
| First query after a stop | 2.08 s, now 0.04 s | 1.44 s, now 0.04 s | 0.79 s, now 0.04 s | 2.81 s, now 0.07 s |
| Memory after it | 140 MB, now 18 MB | 107 MB, now 18 MB | 59 MB, now 16 MB | 197 MB, now 20 MB |
| First edit | 0.67 s, now 0.70 s | 0.73 s, now 0.69 s | 0.14 s, now 0.13 s | 0.89 s, now 0.79 s |
| Memory after it | 196 MB, now 96 MB | 220 MB, now 145 MB | 83 MB, now 72 MB | 390 MB, now 237 MB |
| A later edit | 0.68 s, now 0.63 s | 0.31 s, now 0.26 s | 0.15 s, now 0.13 s | 0.64 s, now 0.58 s |

With a file changed while the daemon was stopped, the first query took 0.33 s on yaml-cpp, 0.19 s on fmt and 0.56 s on curl. All edges were the
same as the counter's on these four, and references from the file gave the same edges as parsing.

On `llvm/lib/Support` with all of `llvm/include` as a project of its own, on `ml`: 101 304 symbols, 47 324 edges, `graph.fb` 20 MB, `references.fb`
19 MB, one run of each kind. The first `sync` with its index took 91 s. All edges were the same.

| | Counter | File |
|---|---|---|
| First query after a stop | 6.1 s | 0.16 s |
| Memory after it | 356 MB | 37 MB |
| A later edit | 0.76 s | 0.58 s |
| Memory after three edits | 694 MB | 423 MB |

The first edit took 2.51 s. Before edges had a buffer of their own, a later edit took 1.53 s here. Two builds of one index differ by an edge, so
a comparison is on one index.

## 7. Not built

- **Linking only what changed.** At these sizes a link is about 0.1 s, and it can change edges, so it waits for a project that shows the need.
- **Text out of memory after a link.** Every edit would then read all files again: the first edit above against a later one. The text is not
  in memory after a start, only from the first link until the daemon stops.
- **Building new symbols without copying every symbol.** The first step of each edit, about 0.7 s at 101 304 symbols. A query waits for it only
  if it comes right after the edit.
- **`fill_local_symbols`: several `clangd`, with progress.** One, in order, silent. Over 20 minutes on LLVM, not finished, with the bug of
  reads counted as changes, since fixed.
- **`kept_definitions` once.** Reads and decodes the whole index for each answer of `clangd`.
- **`Dex`: shared paths and ids.** One `String` per reference.
- **A part of LLVM, as `llvm/lib/Support`, and all of LLVM.** Only on the owner's word.
