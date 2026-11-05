#!/usr/bin/env bash
# Write the Homebrew formula of a published release into the tap. See RELEASING.md.
#
#   scripts/update-tap.sh X.Y.Z           show how it differs from the tap's
#   scripts/update-tap.sh X.Y.Z --push    and commit it there
#
# Through the GitHub API, with the account active in `gh`, which must be able to write to the tap: this
# needs no clone, and no SSH key or setting of git. With TAP_FORMULA set to a path, only write the formula there.
set -euo pipefail
version=${1:?usage: scripts/update-tap.sh X.Y.Z [--push]}
push=${2:-}
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "error: $version is not a version such as 0.2.0" >&2; exit 1; }
repository=https://github.com/julymetodiev/lantern-lattice
base="$repository/releases/download/v$version"

sums=$(curl -fsSL "$base/SHA256SUMS") || { echo "error: v$version has no published SHA256SUMS" >&2; exit 1; }
sum_of() { # target
  local found
  found=$(awk -v file="latt-v$version-$1.tar.gz" '$2 == file { print $1 }' <<<"$sums")
  [ -n "$found" ] || { echo "error: SHA256SUMS has no archive for $1" >&2; exit 1; }
  echo "$found"
}
entry() { # indent, target
  printf '%surl "%s/latt-v%s-%s.tar.gz"\n%ssha256 "%s"\n' "$1" "$base" "$version" "$2" "$1" "$(sum_of "$2")"
}

formula() {
  cat <<RUBY
class LanternLattice < Formula
  desc "The call graph from your compiler or type checker, for humans and coding agents"
  homepage "$repository"
  license "MIT"

  on_macos do
    on_arm do
$(entry "      " aarch64-apple-darwin)
    end
    on_intel do
$(entry "      " x86_64-apple-darwin)
    end
  end

  on_linux do
    on_arm do
$(entry "      " aarch64-unknown-linux-gnu)
    end
    on_intel do
$(entry "      " x86_64-unknown-linux-gnu)
    end
  end

  def install
    bin.install "latt"
  end

  def caveats
    <<~EOS
      For exact answers, Lantern Lattice needs rust-analyzer for Rust, tsrs for TypeScript and JavaScript,
      pyrefly for Python, go with scip-go and gopls for Go, and clangd with clangd-indexer for
      C and C++, from https://github.com/clangd/clangd/releases:
        rustup component add rust-analyzer rust-src
        npm install -g tsrs
        pipx install pyrefly
        go install github.com/scip-code/scip-go/cmd/scip-go@latest
        go install golang.org/x/tools/gopls@latest

      In Claude Code:
        /plugin marketplace add julymetodiev/lantern-lattice
        /plugin install latt@latt

      For other agents, and to see the graph with \`latt view\`:
        $repository
    EOS
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/latt --version")
  end
end
RUBY
}

if [ -n "${TAP_FORMULA:-}" ]; then
  formula > "$TAP_FORMULA"
  exit 0
fi

place=repos/julymetodiev/homebrew-tap/contents/Formula/lantern-lattice.rb
work=$(mktemp -d)
formula > "$work/new.rb"
# A first release has no file there yet, and so nothing to name as what it replaces.
sha=$(gh api "$place" --jq .sha 2>/dev/null || true)
if [ -n "$sha" ]; then
  gh api "$place" --jq .content | base64 --decode > "$work/old.rb"
else
  : > "$work/old.rb"
fi
diff "$work/old.rb" "$work/new.rb" | head -40 || true
if [ "$push" = --push ]; then
  gh api --method PUT "$place" --silent \
    -f message="Update lantern-lattice to v$version" \
    -f content="$(base64 < "$work/new.rb" | tr -d '\n')" \
    ${sha:+-f sha="$sha"}
  echo "The tap now has lantern-lattice $version"
else
  echo "Not pushed. The formula is in $work/new.rb"
fi
