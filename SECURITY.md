# Security policy

## Supported versions

Fixes go to the latest release.

## Reporting a vulnerability

Please do not open a public issue. Report it privately through
[GitHub security advisories](https://github.com/julymetodiev/lantern-lattice/security/advisories/new). You will
get an answer within 72 hours, and a fix is coordinated with you before it is made public.

## What to know before you run it

- **Lantern Lattice runs `rust-analyzer` on your project, and that runs the project's build scripts and proc
  macros.** Indexing an untrusted repository runs its code, as `cargo build` would. Measure or try
  Lantern Lattice on a project you do not trust only in a throwaway environment.
- **Lantern Lattice runs `pyrefly` on your project, and that runs the project's `.venv/bin/python`,** to ask where its
  packages are. A repository that holds a `.venv` runs what it holds, so treat it as you do `rust-analyzer`.
- **Lantern Lattice runs `go list -export` on your project, which compiles it,** with the compiler and, for `cgo`, the C compiler, with
  what the source asks for as far as Go's list of allowed flags lets it. It runs no test and no `//go:generate`. Lantern Lattice does not set
  `GOTOOLCHAIN`, so as for any `go` command a `go.mod` may make Go fetch the toolchain it names, and run it. Treat an untrusted
  repository as you do with `rust-analyzer`.
- **Lantern Lattice runs the `tsrs`, the `pyrefly`, the `go`, the `scip-go` and the `gopls` on your `PATH`,** never one from a project's
  `node_modules` or environment. `tsrs` is a young beta.
- **The tools can write.** Through MCP, an agent can change any file inside the project. A path outside
  the project is refused.
- **The daemon listens on a Unix socket** in a per-user directory, not on the network. Anyone who can
  use that socket can read and edit the project.
- **Nothing leaves your machine.** Lantern Lattice makes no network calls of its own. `rust-analyzer` and `cargo`
  may fetch a project's dependencies, as `go list` may, from whatever fetches modules for your machine. `scip-go` would ask a
  module's host about its packages, so Lantern Lattice runs it with its proxy set to a closed local port. What `gopls` sends was not looked at.
