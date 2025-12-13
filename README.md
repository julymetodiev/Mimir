# Mimir

**Semantic Code Intelligence Engine** - structured understanding of code for AI agents.

```
Traditional:  grep "process_data" → 47 matches → read file1 → read file2 → manual
Mimir:        trace("process_data") → definition + callers + callees + similar + edit_targets
```

## What Mimir IS

A semantic code intelligence engine that gives AI agents structured understanding of code instead of raw text files.

## What Mimir is NOT

- Not an IDE
- Not an LSP server (but can use LSP)
- Not a Git wrapper
- Not another "AI coding assistant"

## Features

- **Symbol Graph**: Track functions, structs, traits, and their relationships
- **Trace Queries**: Find callers/callees across the codebase
- **Incremental Sync**: Only re-parse changed files
- **File Watching**: Automatic detection of file changes
- **Edit Operations**: Safe code modifications with conflict detection
- **Content Deduplication**: Blake3 hashing with Zstd compression

## Status

| Milestone | Description | Status |
|-----------|-------------|--------|
| M1 | Storage + Init | ✅ Done |
| M2 | Single-Language Graph | ✅ Done |
| M3 | Actor + Sync | ✅ Done |
| M4 | Multi-Language | ⏳ Pending |
| M5 | Semantic Search | ⏳ Pending |
| M6 | Agent | ⏳ Pending |

**189 tests passing** | Rust 1.90 (2024 edition)

## Quick Start

```bash
# Initialize Mimir in your project
mimir init

# Check status
mimir status

# Start daemon (background process with auto-sync)
mimir daemon start

# Trace a symbol (uses daemon when available)
mimir trace -f src/main.rs -s process_data

# Edit with preview
mimir edit -f src/lib.rs -m replace -s 10 -e 20 -c "new_code" -p

# Stop daemon when done
mimir daemon stop
```

## Architecture

```
Layer 7: Application    mimir-cli
Layer 6: Agent          (future: mimir-agent, mimir-llm)
Layer 5: Execution      mimir-edit, mimir-tasks
Layer 4: Graph          mimir-graph, mimir-graph-actor
Layer 3: Index          mimir-symbols, mimir-relations, mimir-files
Layer 2: Parsing        mimir-parse-core, mimir-parse-rust
Layer 1: Storage        mimir-content, mimir-cache, mimir-persist, mimir-snapshot
Layer 0: Primitives     mimir-types, mimir-error, mimir-protocol
```

## Crates

| Crate | Description |
|-------|-------------|
| `mimir-types` | Core types: IDs, Symbol, Location, Language |
| `mimir-error` | Error hierarchy |
| `mimir-protocol` | Commands, events, response types |
| `mimir-content` | Content-addressed storage |
| `mimir-cache` | LRU cache with moka |
| `mimir-persist` | Bincode serialization |
| `mimir-snapshot` | Snapshot create/restore |
| `mimir-files` | File index and scanner |
| `mimir-symbols` | Symbol table (DashMap) |
| `mimir-relations` | Relationship graph (petgraph) |
| `mimir-parse-core` | Language adapter infrastructure |
| `mimir-parse-rust` | Rust language support |
| `mimir-graph` | Graph facade |
| `mimir-graph-actor` | Actor pattern for concurrent access |
| `mimir-edit` | File editing with conflict detection |
| `mimir-cli` | Command-line interface |

## Performance Targets

- **Accuracy**: >85% symbol resolution without LSP, >95% with LSP
- **Performance**: <100ms for trace() in 50k LOC project
- **Cold start**: <5s to first working query
- **Memory**: <500MB for 100k LOC project

## Documentation

- [Architecture](docs/ARCHITECTURE.md) - System design and data flow
- [Phases](docs/PHASES.md) - Implementation milestones
- [Decisions](docs/DECISIONS.md) - Architectural decisions
- [CLI](docs/CLI.md) - Command-line usage
- [Traits](docs/TRAITS.md) - Core trait definitions

## License

MIT
