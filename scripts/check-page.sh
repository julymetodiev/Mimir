#!/usr/bin/env bash
# The built page in crates/latt-view/page must come from the sources as they are. With --hash, print
# the hash of those sources.
set -euo pipefail
cd "$(dirname "$0")/.."
sources() {
  # Not Cargo.lock: a release changes it, and rebuilds the page from source anyway.
  git ls-files --cached --others --exclude-standard -- crates/latt-view-page crates/latt-view-wire \
    | LC_ALL=C sort -u | while read -r file; do
    [ -f "$file" ] && { echo "$file"; cat "$file"; }
  done
}
digest() { if command -v sha256sum >/dev/null; then sha256sum | cut -d' ' -f1; else shasum -a 256 | cut -d' ' -f1; fi; }
[ "${1:-}" != "--hash" ] || { sources | digest; exit 0; }
built=$(cat crates/latt-view/page/SOURCES 2>/dev/null || true)
[ "$built" = "$(sources | digest)" ] || { echo "the page in crates/latt-view/page is older than its sources: run scripts/build-page.sh" >&2; exit 1; }
