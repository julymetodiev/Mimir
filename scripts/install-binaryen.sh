#!/usr/bin/env bash
# The binaryen whose wasm-opt builds the page of `latt view`. Another version builds another page:
# binaryen 108 named the table of functions as the table of JavaScript values, and the page of 0.6.0
# could not start.
#
#   scripts/install-binaryen.sh --version   print the version scripts/build-page.sh needs
#   scripts/install-binaryen.sh DIR         install it into DIR, on Linux x86-64, and print its bin
set -euo pipefail

version=133
sha256=2dc9c7813f5375db93d96ead4b78222fcc3e2677bbb832297af4797782a37489 # binaryen-version_133-x86_64-linux.tar.gz

[ "${1:-}" != "--version" ] || { echo "$version"; exit 0; }
dir=${1:?usage: scripts/install-binaryen.sh --version | DIR}
[ "$(uname -s)-$(uname -m)" = Linux-x86_64 ] || {
  echo "only Linux x86-64 is installed here; elsewhere install wasm-opt $version from binaryen" >&2; exit 1; }

archive=binaryen-version_${version}-x86_64-linux.tar.gz
mkdir -p "$dir"
curl -fsSL -o "$dir/$archive" "https://github.com/WebAssembly/binaryen/releases/download/version_${version}/$archive"
echo "$sha256  $dir/$archive" | sha256sum -c - >&2
tar -xzf "$dir/$archive" -C "$dir"
echo "$dir/binaryen-version_${version}/bin"
