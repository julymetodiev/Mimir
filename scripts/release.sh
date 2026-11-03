#!/usr/bin/env bash
# Take a release from [Unreleased] to a tag. See RELEASING.md.
#
#   scripts/release.sh <patch|minor|major|X.Y.Z> [--push] [-y]
#
#   patch, minor, major  raise the version in Cargo.toml by that much
#   X.Y.Z                this version, which may be the current one if it has no tag yet
#   --push               push the commit and the tag, which starts the build
#   -y                   do not ask first
set -euo pipefail
cd "$(dirname "$0")/.."

die() { echo "error: $*" >&2; exit 1; }

push=0 yes=0 asked=
for argument in "$@"; do
  case "$argument" in
    --push) push=1 ;;
    -y|--yes) yes=1 ;;
    -h|--help) sed -n '2,9p' "$0" | cut -c3-; exit 0 ;;
    -*) die "unknown flag $argument" ;;
    *) [ -z "$asked" ] || die "more than one version given"; asked=$argument ;;
  esac
done
[ -n "$asked" ] || { sed -n '2,9p' "$0" | cut -c3-; exit 2; }

current=$(scripts/check-version.sh)
IFS=. read -r major minor patch <<<"$current"
case "$asked" in
  major) version="$((major + 1)).0.0" ;;
  minor) version="$major.$((minor + 1)).0" ;;
  patch) version="$major.$minor.$((patch + 1))" ;;
  *) version=$asked ;;
esac
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "$version is not a version such as 0.2.0"
tag="v$version"

[ "$(git rev-parse --abbrev-ref HEAD)" = main ] || die "not on main"
git fetch --quiet origin main
[ "$(git rev-parse HEAD)" = "$(git rev-parse origin/main)" ] || die "main is not level with origin/main"
git rev-parse -q --verify "refs/tags/$tag" >/dev/null && die "$tag already exists"
! git ls-remote --tags --exit-code origin "refs/tags/$tag" >/dev/null 2>&1 || die "$tag already exists on origin"

unreleased=$(awk '/^## \[Unreleased\]/{on=1;next} /^## \[/{on=0} on' CHANGELOG.md | grep -c '[^[:space:]]' || true)
[ "$unreleased" -gt 0 ] || die "[Unreleased] in CHANGELOG.md says nothing"

# Files outside the release stay out of its commit, but the checks will see them.
others=$(git status --porcelain)
echo "Release $tag (Cargo.toml has $current), push: $([ $push = 1 ] && echo yes || echo no)"
[ -z "$others" ] || { echo "Changes that stay out of the commit:"; echo "$others"; }
if [ $yes != 1 ]; then
  read -r -p "Go on? [y/N] " reply
  [ "$reply" = y ] || die "stopped"
fi

python3 - "$version" <<'PY'
import datetime, re, sys
version = sys.argv[1]

def change(path, pattern, replacement, count=1):
    text = open(path).read()
    new, found = re.subn(pattern, replacement, text, count=count, flags=re.M)
    if not found:
        sys.exit(f"nothing to change in {path}: {pattern}")
    open(path, "w").write(new)

change("Cargo.toml", r'(\[workspace\.package\]\nversion = )"[^"]+"', rf'\1"{version}"')
change("integrations/claude-code/.claude-plugin/plugin.json", r'^(  "version": )"[^"]+"', rf'\1"{version}"')
today = datetime.date.today().isoformat()
change("CHANGELOG.md", r"^## \[Unreleased\]\n", f"## [Unreleased]\n\n## [{version}] - {today}\n")
PY
cargo update --workspace --quiet
scripts/check-version.sh "$version" >/dev/null

scripts/check.sh

git add Cargo.toml Cargo.lock CHANGELOG.md integrations/claude-code/.claude-plugin/plugin.json
git commit --quiet -m "Release $tag"
git tag -a "$tag" -m "Mimir $version"
echo "Made $tag"

if [ $push = 1 ]; then
  git push --atomic origin main "$tag"
  echo "Pushed: the build is at https://github.com/julymetodiev/Mimir/actions"
else
  echo "To publish: git push --atomic origin main $tag"
fi
