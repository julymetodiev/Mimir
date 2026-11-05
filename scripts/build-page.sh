#!/usr/bin/env bash
# Build the page of `mimir view` into crates/mimir-view/page, with a hash of what it was built from.
# Needs: rustup target add wasm32-unknown-unknown; cargo install wasm-bindgen-cli at the version in
# Cargo.lock; wasm-opt from binaryen.
set -euo pipefail
cd "$(dirname "$0")/.."
out=crates/mimir-view/page
wanted=$(awk '/^name = "wasm-bindgen"$/{getline; gsub(/"/,"",$3); print $3}' Cargo.lock)
have=$( (wasm-bindgen --version 2>/dev/null || true) | awk '{print $2}')
[ "$have" = "$wanted" ] || { echo "wasm-bindgen $wanted is needed, found ${have:-none}: cargo install wasm-bindgen-cli --version $wanted --locked" >&2; exit 1; }

cargo build --release --locked --target wasm32-unknown-unknown -p mimir-view-page
mkdir -p "$out"
wasm-bindgen --target web --no-typescript --out-dir "$out" \
  target/wasm32-unknown-unknown/release/mimir_view_page.wasm
wasm-opt -Os --enable-reference-types --enable-bulk-memory --enable-nontrapping-float-to-int \
  --enable-sign-ext --enable-multivalue --enable-mutable-globals \
  "$out/mimir_view_page_bg.wasm" -o "$out/mimir_view_page_bg.wasm"
cp crates/mimir-view-page/index.html "$out/index.html"
scripts/check-page.sh --hash > "$out/SOURCES"
echo "built $out"
