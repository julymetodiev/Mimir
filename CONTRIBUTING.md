# Contributing to Mimir

Read [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) first: it is the specification, and code follows it
exactly. If it is silent or contradicts what you need, open an issue before writing code.

## Set up

You need Rust (the version in `rust-version` in `Cargo.toml`, or newer) on macOS or Linux, and
`rustup component add rust-analyzer rust-src` for the tests that use a real index. Without it those
tests skip themselves.

```bash
git clone https://github.com/julymetodiev/Mimir.git
cd Mimir
scripts/check.sh
```

`scripts/check.sh` runs every check CI runs for the code: architecture rules, file size, newest
dependencies, formatting, clippy and tests.

## The rules

1. **No patches.** Fix the root cause, never silence it. If something is unused, delete it.
2. **Name the layer before writing a piece** (architecture, section 2). If it fits none, ask.
3. **A rule without a machine check is incomplete.** Add its check in the same change.
4. **Never guess a call target.** A rule that cannot be sure leaves the edge out.
5. **A claim about accuracy needs a measurement.** `scripts/bench` has the scripts. Run them on a
   throwaway copy of a project that is a git repository: they edit files, and `rust-analyzer` runs the
   project's build scripts and proc macros.
6. **No `#[allow]`, no `pub(crate)` unless nothing else is possible, no file over 500 lines.**

## A change

- A bug fix comes with a test that fails without it.
- Add what users will notice to `CHANGELOG.md`, under `[Unreleased]`.
- Say in the description what you checked, and what you know is still open.

By contributing you agree that your work is released under the [MIT license](LICENSE).
