# Measurements

The scripts behind the numbers in the main README. Those using tree-sitter run with:

```bash
uv run --python 3.12 --with tree-sitter --with tree-sitter-rust python3 scripts/bench/<script>.py ...
```

Run them on a throwaway copy of a project that is a git repository: they edit files, and
`rust-analyzer` runs its build scripts and proc macros. Without git, `.gitignore` does not apply
and `target/` ends up in the graph. In `.mimir/graph`, the project's index is the largest `.scip`.

The daemon starts by itself with the first command; after rebuilding `mimir`, run `mimir daemon stop` between measurements.

| Script | Answers |
|---|---|
| `ra_truth.py` | Which calls does the language server's call hierarchy report? Input for `scip_eval.py`. |
| `scip_eval.py` | How do call edges in `graph.json` score against a SCIP index? Counts files under `crates/`. |
| `value_sites.py` | Which non-call occurrences of project functions does an index hold, in what syntax? |
| `value_eval.py` | How do function values from syntax rules compare with those from an index? |
| `uses_eval.py` | Do the users `trace` shows for a type, field or constant match the server's find-references? |
| `lsp_probe.py` | How fast and how exact is a running analyzer on given files? |
| `edit_compare.py` | After edits, does a running analyzer give what a new index gives? |
| `daemon_latency.py` | How long does `trace` take through the daemon, from no daemon on and after edits? |
| `graph_diff.py` | Which edges differ between two `graph.json`? |
| `roots.py` | What does each index of a project add, and which files does none cover? |
| `ts_compare.py` | How does a running `tsrs` compare with a `scip-typescript` index, site by site? |

## What the numbers mean

The 100% for calls compares Mimir with the SCIP index it reads, so it shows that Mimir loses
nothing on the way, not that `rust-analyzer` is right. The independent check is the hand-written
list in `crates/mimir-resolve-ra/tests/fixture/expected.rs`. Function values have no complete truth to compare
with beyond that list, and `value_eval.py` compares two resolvers with each other.
