# Security policy

## Supported versions

Fixes go to the latest release.

## Reporting a vulnerability

Please do not open a public issue. Report it privately through
[GitHub security advisories](https://github.com/julymetodiev/Mimir/security/advisories/new). You will
get an answer within 72 hours, and a fix is coordinated with you before it is made public.

## What to know before you run it

- **Mimir runs `rust-analyzer` on your project, and that runs the project's build scripts and proc
  macros.** Indexing an untrusted repository runs its code, as `cargo build` would. Measure or try
  Mimir on a project you do not trust only in a throwaway environment.
- **The tools can write.** Through MCP, an agent can change any file inside the project. A path outside
  the project is refused.
- **The daemon listens on a Unix socket** in a per-user directory, not on the network. Anyone who can
  use that socket can read and edit the project.
- **Nothing leaves your machine.** Mimir makes no network calls of its own. `rust-analyzer` and `cargo`
  may fetch a project's dependencies.
