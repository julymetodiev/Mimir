# Releasing Mimir

One command takes a release from `[Unreleased]` to a tag:

```bash
scripts/release.sh patch           # or minor, major, or an exact version such as 0.2.0
scripts/release.sh patch --push    # and push, which starts the build
```

It stops unless you are on `main`, level with `origin/main`, and `[Unreleased]` in `CHANGELOG.md` says
something. Then it:

1. sets the version in `Cargo.toml`, `Cargo.lock` and the plugin's `plugin.json`;
2. dates the changelog's section;
3. runs `scripts/check.sh`;
4. commits `Release vX.Y.Z` and makes the tag.

Without `--push`, nothing has left your machine. To undo that: `git tag -d vX.Y.Z` and
`git reset --hard HEAD~1`.

## What the tag starts

`.github/workflows/release.yml` checks that the tag matches the version and the changelog, builds the
page of `mimir view` anew from its sources, builds
`mimir` for macOS and Linux on arm64 and x86-64, runs each binary its runner can run, and publishes a
GitHub release with archives, `SHA256SUMS` and the changelog's section as notes.

## After a release

The Homebrew tap, [julymetodiev/homebrew-tap](https://github.com/julymetodiev/homebrew-tap), is
updated by hand with the new URLs and checksums from `SHA256SUMS`.

## A version that is wrong

A published version stays. Fix it in the next one, and say so in the changelog.
