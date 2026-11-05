#!/usr/bin/env bash
# One version everywhere it is written: the workspace, the lock file and the plugin. With a version
# as $1, as a tag gives, also that it is this one and that the changelog has its section.
set -euo pipefail
cd "$(dirname "$0")/.."

workspace=$(awk '/^\[workspace.package\]/{on=1;next} /^\[/{on=0} on&&/^version/{gsub(/"/,"",$3);print $3}' Cargo.toml)
locked=$(awk '/^name = "lantern-lattice"$/{getline; gsub(/"/,"",$3); print $3}' Cargo.lock)
plugin=$(sed -n 's/^ *"version": "\(.*\)",$/\1/p' integrations/claude-code/.claude-plugin/plugin.json)

[ -n "$workspace" ] || { echo "no version in Cargo.toml" >&2; exit 1; }
[ "$locked" = "$workspace" ] || { echo "Cargo.lock has $locked, Cargo.toml has $workspace" >&2; exit 1; }
[ "$plugin" = "$workspace" ] || { echo "plugin.json has $plugin, Cargo.toml has $workspace" >&2; exit 1; }

if [ $# -gt 0 ]; then
  [ "$1" = "$workspace" ] || { echo "the tag says $1, Cargo.toml has $workspace" >&2; exit 1; }
  grep -q "^## \[$1\] - " CHANGELOG.md || { echo "CHANGELOG.md has no section for $1" >&2; exit 1; }
fi
echo "$workspace"
