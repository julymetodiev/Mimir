#!/usr/bin/env bash
# Architecture rule 10: every direct dependency is on its latest stable version.
# Needs cargo-outdated. Exits non-zero and names what is behind.
set -euo pipefail
cd "$(dirname "$0")/.."
cargo outdated --workspace --root-deps-only --exit-code 1
