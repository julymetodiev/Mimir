# C# through Roslyn: proposed contract

Proposal for section 8 of [ARCHITECTURE.md](ARCHITECTURE.md), related to [issue #1](https://github.com/julymetodiev/lantern-lattice/issues/1).
This change supplies a fixture and protocol probe. It does not enable C# in `latt`.

## 1. Proposed parts

| Part | Layer | Responsibility |
|---|---|---|
| `latt-parse-csharp` | Domain | Extract declarations and reference sites through tree-sitter |
| `latt-resolve-lsp` | Adapter | Run the separately installed Roslyn server and request definitions |
| `latt-resolve` | Rules | Match compiler targets to graph symbols without name guesses |
| `commands/daemon.rs` | Composition | Register the parser and semantic source |

The Rust engine, graph, CLI, and MCP API remain in place.
Roslyn supplies C# source semantics through LSP. There is no new compiler or project loader in Lantern Lattice.

Initial scope: `.cs` files in SDK-style C# projects, with a project reference and a `.slnx` solution.
Razor, Visual Basic, F#, and C# scripts are outside this proposal.
Full language coverage and the architecture's performance targets remain acceptance work.

## 2. Reproduce the probe

Install the measured server version and a .NET 10 SDK:

```bash
dotnet tool install --global roslyn-language-server --version 5.13.0-1.26514.10
python3 scripts/bench/roslyn_probe.py > roslyn-probe.json
```

Use `--server /path/to/roslyn-language-server` for a local tool installation.
The probe requires only the Python standard library and the .NET tools.
It copies `fixtures/csharp-workspace` to a temporary directory, builds the copy, and starts two server sessions in sequence.
It changes only that copy and removes it afterward. It shuts down each server before removal.
The fixture has no NuGet package dependencies, and its `NuGet.Config` clears package sources.

Roslyn loads projects through MSBuild, which can execute project code.
Run the probe with the supplied fixture and a trusted server installation.
The probe disables Roslyn telemetry.

The script exits with status 1 if any target differs or the edited result differs from the restart result.
A mismatch is diagnostic evidence, not a successful check.

## 3. Measured protocol behavior

Measured on macOS arm64 with .NET SDK 10.0.401 and `roslyn-language-server` 5.13.0-1.26514.10 on November 5, 2026.
Each phase checks 21 hand-marked sites. The checks compare the exact target file, line, and UTF-16 column.

| Phase | Correct positions | Result |
|---|---:|---|
| Initial project load | 21/21 | All expected targets |
| Cross-file return-type change | 21/21 | The caller targets `Beta.Act` instead of `Alpha.Act` |
| One comment line inserted before declarations | 9/21 | 12 target columns differ |
| Fresh server with the edited files on disk | 21/21 | All expected targets |

The initial and restart answers distinguish overloads, same-line overloads, constructors, generics, and extension methods.
They also distinguish interface declarations, virtual overrides, fields, properties, constants, and calls across a project reference.
A call through a local delegate resolves to the local variable. An unresolved dynamic member gives no definition.
The probe also checks a site after an emoji, where UTF-16 columns differ from character and byte counts.

The declaration-movement mismatch is reproducible in the supplied probe.
The probe uses minimal incremental edits, not fixed sleeps or repeated requests until a desired answer appears.
A follow-up reproduced the same 12 failures on Roslyn main at `ff2e51158d0f18bf40381246097f2a5dbd83118e`.
Frozen partial semantics retained a target from an old syntax tree. The LSP handler converted that offset with current document text.
[Roslyn PR #86332](https://github.com/dotnet/roslyn/pull/86332) rejects stale target trees and retries full semantics.
Unchanged target trees retain the frozen-semantic fast path.

With that patch, the same probe passes **84/84**, and edited answers equal the clean restart.
All 16 Roslyn regression cases fail without the fix and pass with it. The 72-test definition suite also passes.
The regression explicitly enables partial semantics, which production enables but test workspaces disable by default.
The fix remains a draft, not a released package. These results do not establish complete incremental graph correctness.

These measurements concern LSP definition locations. They do not measure Lantern Lattice graph accuracy, latency, or memory.
The server advertises call hierarchy support, but the probe does not measure it.

## 4. Project lifecycle

Start Roslyn with `--stdio --autoLoadProjects --telemetryLevel off`.
Let Roslyn select and load projects. Wait for `workspace/projectInitializationComplete` before semantic requests.
The notification reports completion, not successful compilation of every project.
Missing packages, unavailable SDKs, and project errors must remain visible as incomplete coverage.

The user restores the project separately. Lantern Lattice does not install SDKs, tools, or packages.
The server comes from `PATH`, not a repository-controlled environment.
Document the MSBuild execution boundary in `SECURITY.md` when the runtime adapter is added.

With the measured unpatched package, use a fresh semantic pass after an edit and request all C# sites again.
A fixed server can remain alive across the edits in the probe. Broader graph equivalence still needs the acceptance checks below.
Do not reuse persistent C# answers until edit/restart equivalence holds.
C# namespaces, partial types, and overload sets cross file boundaries without Python-style file imports.
The import-based invalidation strategy is insufficient for this language.

The engine currently ignores watcher changes for unknown source extensions.
Project and configuration changes therefore need an explicit full sync until the watcher contract includes those inputs.
The implementation must report this limit. A project-initialization event alone does not invalidate old answers.

## 5. Semantic identity and edges

Keep each declaration's complete source span and exact name position.
Preserve target columns through LSP conversion and graph lookup. A line and a name cannot distinguish same-line overloads.
Never substitute the next method with the same name when an exact target is absent.
An unresolved target remains unresolved, such as targets in external metadata or absent generated files.

| Reference | Proposed result |
|---|---|
| One ordinary method, local function, or explicit constructor at a call site | `Calls` |
| A method used as a value | `UsesAsValue` |
| Interface, virtual, override, or abstract declaration | Possible use until the selected runtime target is proven |
| Several callable declarations | Possible uses, never an arbitrary exact call |
| A delegate variable or unresolved dynamic member | No guessed call edge |
| Field, constant, or type | The corresponding data-use edge |
| Property | A declared property use, without a claim about accessor calls |
| `nameof` | No runtime call or method-value edge |

No syntax-only C# call fallback is proposed. Overloads, local shadowing, extension methods, and dynamic dispatch make name matching unsafe.
Implementations of an interface need separate evidence and must not appear as exact calls merely because they exist.
Partial declarations, linked files, and multiple target frameworks need explicit identity rules before full support.

## 6. Acceptance work

Before the runtime adapter is ready, require parser-to-graph tests for every supported row above.
Include same-line overloads, Unicode positions, local delegate shadowing, default interface methods, and calls across projects.
Check no-server behavior and exact target positions, not only edge counts or equal names.
CI must install the tested server so those integration tests cannot silently skip.

Before incremental answers replace fresh passes, require zero differences against clean graphs for:

- New overloads and changes to return types.
- Partial types, global usings, linked files, and deleted files.
- Project references, package restores, preprocessor symbols, and target-framework changes.
- Declaration movement and unsaved text.

Then measure cold start, first answer after an edit, full resolution, and total process-tree memory on a real C# solution.
Generated code, implicit constructors, operators, indexers, events, and property accessor calls need separate coverage.
The chosen project context must accompany any future cache identity.

## 7. Source and discussion ledger

- [Maintainer response](https://github.com/julymetodiev/lantern-lattice/issues/1#issuecomment-6670094425): fixture, LSP probe, and contract first.
- [Roslyn standalone server](https://github.com/dotnet/roslyn/blob/ff2e51158d0f18bf40381246097f2a5dbd83118e/src/LanguageServer/roslyn-language-server/README.md): transport, project selection, and requirements.
- [Per-client autoload options](https://github.com/dotnet/roslyn/pull/85814): merged September 29, 2026.
- [On-demand project loading](https://github.com/dotnet/roslyn/pull/85805): merged October 26, 2026. Current source is not proof of behavior in the measured package.
- [Definition handler](https://github.com/dotnet/roslyn/blob/ff2e51158d0f18bf40381246097f2a5dbd83118e/src/LanguageServer/Protocol/Handler/Definitions/AbstractGoToDefinitionHandler.cs): source and metadata locations.
- [Call hierarchy across multiple C# documents](https://github.com/dotnet/roslyn/pull/84149): a future independent comparison.

Roslyn's current source revision for this review was `ff2e51158d0f18bf40381246097f2a5dbd83118e`.
The installed tool package has a separate version identity. The installation command above fixes the package version for reproduction.
