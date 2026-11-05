#!/usr/bin/env bash
# Architecture rule 6: no Rust file over 500 lines, tests included. What flatc generates is not ours to split.
# Fails on a file over the limit that is not in scripts/file-size-allowlist.txt, and on an entry
# there that is missing or back within it, so the list only shrinks.
#   --list   print every file over the limit
set -euo pipefail
cd "$(dirname "$0")/.."
LIMIT=500
over() {
  git ls-files --cached --others --exclude-standard -- '*.rs' | sort -u | grep -v -E '^crates/latt-graph-fb/src/(generated|edges_generated|references_generated)\.rs$' | while IFS= read -r f; do
    [ -f "$f" ] || continue
    n=$(wc -l <"$f" | tr -d ' ')
    if [ "$n" -gt "$LIMIT" ]; then printf '%s\n' "$f"; fi
  done
}
if [ "${1:-}" = "--list" ]; then over; exit 0; fi
allowed=$(grep -v -E '^[[:space:]]*(#|$)' scripts/file-size-allowlist.txt || true)
current=$(over)
status=0
while IFS= read -r f; do
  [ -n "$f" ] || continue
  if ! grep -qxF "$f" <<<"$allowed"; then echo "error: $f is over $LIMIT lines; split it by responsibility"; status=1; fi
done <<<"$current"
while IFS= read -r f; do
  [ -n "$f" ] || continue
  if ! grep -qxF "$f" <<<"$current"; then echo "error: $f is within $LIMIT lines or gone; take it off the allowlist"; status=1; fi
done <<<"$allowed"
if [ "$status" = 0 ]; then echo "file size check passed"; fi
exit "$status"
