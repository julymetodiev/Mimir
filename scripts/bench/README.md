# Measurements

The scripts behind the numbers in the main README. Those using tree-sitter run with:

```bash
uv run --python 3.12 --with tree-sitter --with tree-sitter-rust python3 scripts/bench/<script>.py ...
```

Run them on a throwaway copy of a project that is a git repository: they edit files, and
`rust-analyzer` runs its build scripts and proc macros. Without git, `.gitignore` does not apply
and `target/` ends up in the graph. In `.latt/graph`, the project's index is the largest `.scip`.

The daemon starts by itself with the first command; after rebuilding `latt`, run `latt daemon stop` between measurements.

| Script | Answers |
|---|---|
| `ra_truth.py` | Which calls does the language server's call hierarchy report? Input for `scip_eval.py`. |
| `scip_eval.py` | How do call edges in `graph.json` score against a SCIP index? Counts files under `crates/`. |
| `value_sites.py` | Which non-call occurrences of project functions does an index hold, in what syntax? |
| `value_eval.py` | How do function values from syntax rules compare with those from an index? |
| `uses_eval.py` | Do the users `trace` shows for a type, field or constant match the server's find-references? |
| `lsp_probe.py` | How fast and how exact is a running analyzer on given files? |
| `edit_matrix.py` | Every language and kind of edit, in under a minute: the graph after the edit against a clean start |
| `edit_compare.py` | After edits, does a running analyzer give what a new index gives? |
| `daemon_latency.py` | How long does `trace` take through the daemon, from no daemon on and after edits? |
| `graph_diff.py` | Which edges differ between two `graph.json`, from `latt sync --json`? |
| `roots.py` | What does each index of a project add, and which files does none cover? |
| `scip_compare.py` | How does a running `tsrs` compare with a `scip-typescript` index, site by site? |
| `go_compare.py` | GO2: does every disagreement between `gopls` and the index of a Go root fall in a named class? |

## What the numbers mean

The 100% for calls compares Lantern Lattice with the SCIP index it reads, so it shows that Lantern Lattice loses
nothing on the way, not that `rust-analyzer` is right. The independent check is the hand-written
list in `crates/latt-resolve-ra/tests/fixture/expected.rs`. Function values have no complete truth to compare
with beyond that list, and `value_eval.py` compares two resolvers with each other.
