#!/usr/bin/env bash
# The one way to build and test here, with the same options every time, so a profile holds one copy
# of each crate. Another `-p` or other features unify the features of dependencies differently, and
# cargo then compiles the same code again beside the old copy.
#
# Usage: scripts/build.sh debug | release | install | test [NAME]
#   install  builds release, puts it where `latt` is run from and stops the daemon on the old one
#   test     all tests, or those whose name has NAME
set -euo pipefail
cd "$(dirname "$0")/.."
options=(--workspace --all-features --locked)
case "${1:-}" in
  debug) cargo build "${options[@]}" ;;
  release) cargo build --release "${options[@]}" ;;
  install)
    cargo build --release "${options[@]}"
    install -m 755 target/release/latt "${CARGO_HOME:-$HOME/.cargo}/bin/latt"
    "${CARGO_HOME:-$HOME/.cargo}/bin/latt" daemon stop
    ;;
  test)
    shift
    cargo test "${options[@]}" --no-fail-fast "$@"
    ;;
  *)
    echo "usage: scripts/build.sh debug | release | install | test [NAME]" >&2
    exit 2
    ;;
esac
