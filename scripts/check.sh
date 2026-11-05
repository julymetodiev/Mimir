#!/usr/bin/env bash
# Every check of docs/ARCHITECTURE.md section 6, stopping at the first that fails. Full output goes
# to the directory given as $1, or a temporary one. --new compares with HEAD: run it before a commit.
set -euo pipefail
cd "$(dirname "$0")/.."
out=${1:-$(mktemp -d)}
mkdir -p "$out"
run() { # name, command...
  local name=$1; shift
  if "$@" >"$out/$name.log" 2>&1; then echo "ok $name"; else echo "FAILED $name: see $out/$name.log"; tail -20 "$out/$name.log"; exit 1; fi
}
run architecture scripts/check-architecture.sh
run new scripts/check-architecture.sh --new HEAD
run file-size scripts/check-file-size.sh
run version scripts/check-version.sh
run page scripts/check-page.sh
run page-clippy cargo clippy --target wasm32-unknown-unknown -p latt-view-page --all-features -- -D warnings
run deps scripts/check-deps.py
run fmt cargo fmt --all -- --check
run clippy cargo clippy --workspace --all-targets --all-features -- -D warnings
run test scripts/build.sh test
run deny cargo deny --workspace --all-features check
RUSTDOCFLAGS="-D warnings" run docs cargo doc --workspace --no-deps --all-features
echo "all checks passed"
