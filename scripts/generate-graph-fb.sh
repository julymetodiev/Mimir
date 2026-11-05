#!/usr/bin/env bash
# Write the code generated from the schema of the graph on disk, kept in the repository so that
# building needs no flatc. With --check, fail if what is kept differs from the schema.
set -euo pipefail
cd "$(dirname "$0")/.."
wanted=25.12.19
found=$(flatc --version | awk '{print $3}')
[ "$found" = "$wanted" ] || { echo "flatc $wanted is needed, found $found" >&2; exit 1; }
out=$(mktemp -d)
trap 'rm -rf "$out"' EXIT
flatc --rust -o "$out" crates/mimir-graph-fb/graph.fbs crates/mimir-graph-fb/edges.fbs crates/mimir-graph-fb/references.fbs
if [ "${1:-}" = "--check" ]; then
  diff -q "$out/graph_generated.rs" crates/mimir-graph-fb/src/generated.rs
  diff -q "$out/edges_generated.rs" crates/mimir-graph-fb/src/edges_generated.rs
  diff -q "$out/references_generated.rs" crates/mimir-graph-fb/src/references_generated.rs
else
  cp "$out/graph_generated.rs" crates/mimir-graph-fb/src/generated.rs
  cp "$out/edges_generated.rs" crates/mimir-graph-fb/src/edges_generated.rs
  cp "$out/references_generated.rs" crates/mimir-graph-fb/src/references_generated.rs
fi
