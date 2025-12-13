# Mímir Implementation Phases

This document outlines the implementation plan in phases with milestones.

---

## Milestones Overview

| Milestone | Description | Key Deliverable | Status |
|-----------|-------------|-----------------|--------|
| M1 | Storage + Init | `mimir init`, content store | ✅ Done |
| M2 | Single-Language Graph | `mimir trace` for Rust | ✅ Done |
| M3 | Actor + Sync | File watching, incremental sync | ✅ Done |
| M4 | Multi-Language | TS, Python, Go + cross-language trace | ⏳ |
| M5 | Semantic Search | `mimir search` with embeddings | ⏳ |
| M6 | Agent | `mimir agent` autonomous coding | ⏳ |

---

## Phase 0: Project Setup

**Focus**: Repository and workspace setup

```
Tasks:
├── Repository setup
│   ├── Create GitHub repo
│   ├── Setup CI/CD (GitHub Actions)
│   ├── Configure rustfmt, clippy
│   └── Setup pre-commit hooks
│
├── Crate structure
│   ├── Create workspace Cargo.toml
│   ├── Initialize Layer 0 crates:
│   │   ├── mimir-types
│   │   └── mimir-error
│   ├── Initialize Layer 1 crates:
│   │   ├── mimir-content
│   │   ├── mimir-cache
│   │   └── mimir-persist
│   └── Setup internal dependencies
│
├── Dependencies audit
│   ├── Lock versions for:
│   │   ├── tree-sitter = "0.22"
│   │   ├── dashmap = "5.5"
│   │   ├── petgraph = "0.6"
│   │   ├── moka = "0.12"
│   │   ├── blake3 = "1.5"
│   │   └── tokio = "1.36"
│   └── Verify no conflicts
│
└── Documentation
    ├── README.md
    ├── ARCHITECTURE.md
    └── CONTRIBUTING.md
```

**Deliverables**:
- Workspace compiles
- CI runs clippy + tests
- Basic documentation in place

---

## Phase 1: Storage Foundation (M1) ✅ COMPLETED

**Focus**: Content store, file index, basic persistence

**Completed**: December 2025 | Rust 1.90 (2024 edition) | 59 tests passing

### Part A: Core Storage

```
Tasks:
├── mimir-types
│   ├── ID types (SymbolId, FileId, etc.)
│   ├── Location, Span types
│   ├── Language enum
│   └── Tests: serialization round-trip
│
├── mimir-error
│   ├── Error hierarchy (MimirError, StorageError, etc.)
│   └── Tests: error conversions
│
├── mimir-content
│   ├── ContentHash type
│   ├── ContentStore trait
│   ├── Filesystem backend
│   │   ├── Blake3 hashing
│   │   ├── Zstd compression
│   │   └── Atomic writes (write to tmp, rename)
│   ├── Memory backend (for testing)
│   ├── LRU cache (moka)
│   └── Tests: store/retrieve/dedup
│
└── Basic serialization
    ├── bincode for internal structures
    └── Tests: round-trip
```

### Part B: File Index

```
Tasks:
├── mimir-files
│   ├── FileEntry structure
│   ├── FileIndex (DashMap)
│   ├── Directory scanner
│   │   ├── Ignore patterns (.gitignore style)
│   │   ├── Language detection
│   │   └── Incremental scan (mtime check)
│   ├── ChangeSet (added/modified/deleted)
│   └── Tests: scan/detect changes
│
├── mimir-snapshot
│   ├── Snapshot structure
│   ├── Create snapshot
│   ├── List snapshots
│   ├── Restore snapshot
│   └── Tests: create/restore cycle
│
└── CLI: init command
    ├── `mimir init` creates .agent/
    ├── `mimir status` shows indexed files
    └── Integration tests
```

**M1 Deliverables**: ✅ All completed
- ✅ `mimir init` creates `.agent/` directory
- ✅ `mimir status` shows indexed files
- ✅ Content deduplication works
- ✅ Snapshots can be created/restored

**Implemented Crates**:
- `mimir-types` - IDs, ContentHash, Location, Span, Language
- `mimir-error` - Error hierarchy (MimirError, StorageError, etc.)
- `mimir-persist` - Bincode serialization
- `mimir-cache` - LRU cache with moka
- `mimir-content` - ContentStore trait + Memory/Filesystem backends
- `mimir-snapshot` - Snapshot create/list/restore
- `mimir-files` - FileIndex (DashMap), Scanner with gitignore
- `mimir-cli` - CLI with init/status commands

---

## Phase 2: Core Graph (M2) ✅ COMPLETED

**Focus**: Symbol table, reference graph, basic queries

**Completed**: December 2025 | Rust 1.90 (2024 edition) | 115 tests passing (56 new)

### Part A: Data Structures ✅

```
Tasks:
├── mimir-types (additions) ✅
│   ├── Symbol, QualifiedName
│   ├── SymbolKind enum
│   ├── Reference, ReferenceKind
│   ├── Confidence levels
│   └── Serialization
│
├── mimir-symbols ✅ (15 tests)
│   ├── Symbol table
│   │   ├── DashMap<SymbolId, Arc<Symbol>>
│   │   ├── Name index (name → Vec<SymbolId>)
│   │   └── File index (FileId → Vec<SymbolId>)
│   ├── SymbolsHandle pattern
│   └── Tests: CRUD operations
│
└── mimir-relations ✅ (11 tests)
    ├── petgraph::StableGraph wrapper
    ├── ArcSwap for snapshot reads
    ├── Add/remove edges
    ├── Basic traversal
    └── Tests: graph operations
```

### Part B: First Language Adapter ✅

```
Tasks:
├── mimir-parse-core ✅ (6 tests)
│   ├── LanguageAdapter trait
│   ├── ParsedTree wrapper
│   ├── ExtractedSymbol, ExtractedReference
│   ├── AdapterRegistry
│   └── Tests: registry operations
│
├── mimir-parse-rust ✅ (9 tests)
│   ├── tree-sitter-rust setup
│   ├── Symbol extraction (functions, structs, enums, traits, impl, modules)
│   ├── Reference extraction (calls, use statements)
│   └── Tests: parse real Rust files
│
└── Integration ✅
    ├── Build graph from file set
    ├── Symbol resolution (basic)
    └── Tests: small Rust project
```

### Part C: Graph Queries ✅

```
Tasks:
├── mimir-graph ✅ (9 tests)
│   ├── Graph facade (combines symbols + relations)
│   ├── find_symbol(name) → Vec<Symbol>
│   ├── get_callers(symbol_id) → Vec<Symbol>
│   ├── get_callees(symbol_id) → Vec<Symbol>
│   └── Tests: query accuracy
│
├── trace() implementation ✅
│   ├── TraceOptions (max_depth, callers_only, callees_only)
│   ├── TraceResult with callers/callees chains
│   ├── TraceNode with depth and relationship
│   └── Tests: trace on sample code
│
└── CLI integration ✅
    ├── `mimir trace --file <file> --symbol <name>`
    ├── `--depth` option for max trace depth
    ├── `--json` for JSON output
    └── Integration tests
```

**M2 Deliverables**: ✅ All completed
- ✅ `mimir trace --file src/main.rs --symbol process_data` returns definition + callers + callees
- ✅ Works on Rust projects (tree-sitter-rust)
- ✅ JSON output for tooling integration (`--json` flag)

**Implemented Crates**:
- `mimir-symbols` - SymbolTable with DashMap, NameIndex, FileIndex, QualifiedIndex
- `mimir-relations` - RelationGraph with petgraph StableGraph + ArcSwap
- `mimir-parse-core` - LanguageAdapter trait, AdapterRegistry, ExtractedSymbol/Reference
- `mimir-parse-rust` - RustAdapter with tree-sitter-rust for symbol/reference extraction
- `mimir-graph` - CodeGraph facade combining symbols + relations, TraceOptions/TraceResult

**Example Usage**:
```bash
$ mimir trace -f crates/mimir-graph/src/facade.rs -s add_symbol

Trace for: add_symbol (method)
  File: crates/mimir-graph/src/facade.rs:56

Callers (5):
  ← test_find_with_options (function)
  ← test_remove_file (function)
  ← test_trace (function)
  ← test_call_relationships (function)
  ← test_add_and_find_symbol (function)

Total: 6 symbols, max depth: 1
```

---

## Phase 3: Actor & Sync (M3) ✅ COMPLETED

**Focus**: Concurrent architecture, file watching, sync

**Completed**: December 2025 | Rust 1.90 (2024 edition) | 189 tests passing (56 new)

### Part A: Channel Architecture ✅

```
Tasks:
├── mimir-protocol ✅ (16 tests)
│   ├── GraphCommand enum (AddFile, RemoveFile, FindSymbol, Trace, Stats, Sync, Shutdown)
│   ├── GraphEvent enum (FileAdded, FileUpdated, FileRemoved, SyncStarted, SyncCompleted)
│   ├── FindOptions, TraceOptions configuration
│   ├── Response types (FindResponse, TraceResponse, StatsResponse, SyncResult)
│   ├── Typed channel wrappers (CommandSender, EventSender, etc.)
│   └── IPC protocol (IpcCommand, IpcResponse) for daemon communication
│
├── mimir-graph-actor ✅ (14 tests: 10 unit + 4 integration)
│   ├── GraphActor (owns graph, registry, file tracking)
│   ├── Command processing loop with async support
│   ├── Reply channels (oneshot for request-reply)
│   ├── File tracking with content hashes
│   └── Tests: concurrent access, full workflow
│
├── GraphHandle (client) ✅
│   ├── Async query methods (find_symbol, trace, stats, sync)
│   ├── Event subscription via broadcast channel
│   ├── Clone is O(1), Send + Sync
│   └── Tests: multiple clients
│
├── File watcher ✅
│   ├── notify + notify-debouncer-mini integration
│   ├── 100ms debouncing by default
│   ├── Configurable ignore patterns (target/, node_modules/, .git/)
│   ├── Language detection for watched files
│   └── Tests: configuration, ignore patterns
│
└── Daemon architecture ✅
    ├── DaemonServer (runs GraphActor + FileWatcher)
    ├── DaemonClient (Unix socket IPC)
    ├── CLI commands: daemon start, stop, status
    ├── Auto-detection: CLI uses daemon when available
    └── Background mode with PID file management
```

### Part B: Sync & Edit ✅

```
Tasks:
├── Sync implementation ✅
│   ├── Scanner integration from mimir-files
│   ├── Content hash comparison for change detection
│   ├── Incremental update (add/update/remove files)
│   ├── SyncResult with detailed statistics
│   └── Tests: full sync workflow, incremental sync
│
├── mimir-edit ✅ (16 tests)
│   ├── EditOp enum (Replace, Insert, Delete, ReplaceLines)
│   ├── Editor with apply(), preview(), apply_batch()
│   ├── Line-based edits
│   ├── Byte-range edits
│   ├── Optimistic locking (content hash)
│   ├── Conflict detection (HashMismatch error)
│   ├── Backup file creation
│   └── Tests: edit + verify
│
├── CLI integration ✅
│   ├── `mimir edit --file <f> --mode replace --start <s> --end <e> --content <c>`
│   ├── `mimir edit --preview` for diff preview
│   ├── `mimir edit-batch --json <ops>` for batch operations
│   └── `--backup` flag for backup creation
│
└── Integration tests ✅ (4 tests)
    ├── test_full_sync_workflow: init → sync → query
    ├── test_incremental_sync: add/modify/delete detection
    ├── test_add_file_directly: manual file addition
    └── test_trace_symbols: symbol tracing
```

**M3 Deliverables**: ✅ All completed
- ✅ File changes detected automatically (FileWatcher with notify)
- ✅ Incremental sync (only changed files via content hash)
- ✅ Safe concurrent access (actor pattern with channels)
- ✅ Edit operations with conflict detection
- ✅ Daemon mode with auto-sync (background process with Unix socket IPC)

**Implemented Crates**:
- `mimir-protocol` - GraphCommand, GraphEvent, Response types, typed channels, IPC protocol
- `mimir-graph-actor` - GraphActor, GraphHandle, FileWatcher with debouncing
- `mimir-edit` - EditOp, EditError, Editor, Diff with optimistic locking
- `mimir-cli` - Daemon server/client, daemon commands (start, stop, status)

**Example Usage**:
```bash
# Start daemon (background process with file watcher)
$ mimir daemon start
Daemon started (PID: 12345)

# Start daemon in foreground (for debugging)
$ mimir daemon start --foreground
Starting daemon in foreground mode...
INFO Starting Mimir daemon for: /path/to/project
INFO File watcher started
INFO Daemon listening on: .agent/daemon.sock

# Check daemon status
$ mimir daemon status
Daemon running (PID: 12345)
Uptime: 2h 15m
Files indexed: 342
Symbols: 1847

# Stop daemon
$ mimir daemon stop
Daemon stopped

# Trace command auto-detects daemon
$ mimir trace -f src/main.rs -s process_data
Trace for: process_data (function) (via daemon)
  File: src/main.rs:42
...

# Programmatic usage via GraphHandle
let sync_result = handle.sync().await?;
println!("Added: {}, Updated: {}, Removed: {}",
    sync_result.files_added,
    sync_result.files_updated,
    sync_result.files_removed);

# Preview a replace operation
$ mimir edit -f src/main.rs -m replace -s 10 -e 20 -c "new_code" -p

# Apply line replacement with backup
$ mimir edit -f src/lib.rs -m lines -s 5 -e 10 -c "// new lines\n" -b
```

---

## Phase 4: Multi-Language (M4)

**Focus**: TypeScript, Python, Go adapters + cross-language bridges

### Part A: Language Adapters

```
Tasks:
├── mimir-parse-ts
│   ├── tree-sitter-typescript setup
│   ├── Symbol extraction (functions, classes, interfaces)
│   ├── Reference extraction
│   ├── Import resolution
│   ├── JSX support
│   └── Tests: real TS/TSX files
│
├── mimir-parse-py
│   ├── tree-sitter-python setup
│   ├── Symbol extraction (functions, classes)
│   ├── Reference extraction
│   └── Tests: real Python files
│
└── mimir-parse-go
    ├── tree-sitter-go setup
    ├── Symbol extraction
    ├── Reference extraction
    └── Tests
```

### Part B: Cross-Language Bridges

```
Tasks:
├── Bridge detection
│   ├── Bridge enum (HttpEndpoint, HttpCall, etc.)
│   ├── Rust: Axum/Actix endpoint detection
│   ├── TypeScript: fetch/axios call detection
│   ├── Bridge matching algorithm
│   └── Confidence scoring
│
├── Cross-language trace
│   ├── trace() with cross_language=true
│   ├── Protocol detection (HTTP/gRPC)
│   ├── CrossLanguageRef in TraceResult
│   └── Tests: frontend→backend tracing
│
└── CLI enhancements
    ├── `mimir trace --cross-language`
    ├── Language filter options
    └── Output formatting
```

**M4 Deliverables**:
- Support for Rust, TypeScript, Python, Go
- `mimir trace --cross-language` follows HTTP calls across frontend/backend

---

## Phase 5: Semantic Search (M5)

**Focus**: Vector embeddings and semantic search

### Part A: Embedding Infrastructure

```
Tasks:
├── mimir-embeddings
│   ├── EmbeddingEncoder trait
│   ├── Local encoder (ONNX Runtime)
│   ├── API encoder (OpenAI)
│   └── Tests: encode/compare
│
├── HNSW index
│   ├── Index implementation
│   ├── Add/remove/search
│   ├── ArcSwap + append buffer pattern
│   ├── Persistence
│   └── Tests: recall accuracy
│
└── Embedding manager
    ├── Background encoding queue
    ├── Incremental updates on graph changes
    └── Tests: large-scale indexing
```

### Part B: Semantic Search

```
Tasks:
├── Search implementation
│   ├── Query encoding
│   ├── K-NN search
│   ├── Filter by language/kind
│   ├── SimilarSymbol in TraceResult
│   └── Tests: search relevance
│
└── CLI integration
    ├── `mimir search "validate user input"`
    ├── `mimir trace --similar`
    └── Output formatting
```

**M5 Deliverables**:
- `mimir search "handle authentication"` finds semantically similar code
- Trace results include similar implementations

---

## Phase 6: Agent Integration (M6)

**Focus**: LLM orchestration, tools, agent loop

### Part A: Tool Definitions

```
Tasks:
├── mimir-tools
│   ├── Tool trait
│   ├── ToolRegistry
│   ├── JSON Schema generation
│   ├── Built-in tools:
│   │   ├── TraceTool
│   │   ├── EditTool
│   │   ├── SearchTool
│   │   ├── ReadFileTool
│   │   └── ShellTool
│   └── Tests: tool execution
│
├── mimir-llm
│   ├── LlmProvider trait
│   ├── Anthropic implementation
│   ├── OpenAI implementation
│   ├── Streaming support
│   └── Tests: basic completion
│
└── Agent config format
    ├── TOML schema
    ├── Parsing & validation
    └── Hot reload
```

### Part B: Task System

```
Tasks:
├── mimir-tasks
│   ├── Task, TaskPlan, PlannedStep
│   ├── TaskStatus state machine
│   ├── Checkpoint system
│   ├── Task storage (filesystem)
│   └── Tests: task lifecycle
│
├── mimir-context
│   ├── HierarchicalContext
│   ├── Context compression
│   ├── Smart code loading
│   └── Tests: context management
│
└── mimir-executor
    ├── Step execution loop
    ├── Recovery from checkpoint
    ├── Graceful shutdown
    └── Tests: execution flow
```

### Part C: Agent Runtime

```
Tasks:
├── mimir-agent
│   ├── Agent struct
│   ├── Tool execution loop
│   ├── Context management integration
│   ├── Error handling
│   └── Tests: agent workflow
│
├── CLI integration
│   ├── `mimir agent` interactive mode
│   ├── `mimir task <request>`
│   ├── `mimir resume`
│   └── Progress display
│
└── Security & sandbox
    ├── Path restrictions
    ├── Command whitelist
    ├── Resource limits
    └── Tests: escape attempts
```

**M6 Deliverables**:
- `mimir agent` runs autonomous coding agent
- Tasks with checkpointing and recovery
- Sandboxed execution

---

## Testing Strategy

### Unit Tests

```rust
// tests/unit/symbol_test.rs
#[test]
fn test_qualified_name_parsing() {
    let qn = QualifiedName::from_str("crate::module::func", "::");
    assert_eq!(qn.name(), "func");
    assert_eq!(qn.parent().unwrap().to_string(), "crate::module");
}

// tests/unit/content_store_test.rs
#[test]
fn test_content_deduplication() {
    let store = ContentStore::new_temp();
    let content = b"hello world";
    let hash1 = store.store(content).unwrap();
    let hash2 = store.store(content).unwrap();
    assert_eq!(hash1, hash2);
}
```

### Integration Tests

```rust
// tests/integration/rust_adapter_test.rs
#[test]
fn test_rust_symbol_extraction() {
    let adapter = RustAdapter::new();
    let source = include_str!("fixtures/sample.rs");
    let tree = adapter.parse(source).unwrap();
    let symbols = adapter.extract_symbols(&tree, source, FileId::new(1));
    insta::assert_yaml_snapshot!(symbols);
}

// tests/integration/cross_language_test.rs
#[tokio::test]
async fn test_http_bridge_detection() {
    let graph = build_test_graph(&[
        ("api.ts", include_str!("fixtures/api.ts")),
        ("handlers.rs", include_str!("fixtures/handlers.rs")),
    ]).await;

    let result = graph.trace("list_users", TraceOptions {
        cross_language: true,
        ..Default::default()
    }).await.unwrap();

    assert_eq!(result.cross_language.len(), 1);
}
```

### Benchmark Tests

```rust
// benches/graph_bench.rs
use criterion::{criterion_group, criterion_main, Criterion};

fn bench_trace(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let graph = rt.block_on(setup_large_project()); // 10k files

    c.bench_function("trace_simple", |b| {
        b.iter(|| {
            rt.block_on(graph.trace("process_data", Default::default()))
        });
    });
}

criterion_group!(benches, bench_trace);
criterion_main!(benches);
```

---

## Configuration Schema

```toml
# .agent/config.toml

[project]
name = "my-project"
root = "."
output_dir = ".agent"

[indexing]
include = ["**/*.rs", "**/*.ts", "**/*.py", "**/*.go"]
exclude = [
    "**/node_modules/**",
    "**/target/**",
    "**/.git/**",
]
max_file_size_kb = 500

[languages]
enabled = ["rust", "typescript", "python", "go"]

[languages.rust]
edition = "2021"

[languages.typescript]
tsconfig = "tsconfig.json"
jsx = true

[resolution]
use_lsp = false
lsp_timeout_ms = 1000
fallback_to_name = true

[bridges]
enabled = true
confidence_threshold = 0.7

[cache]
max_memory_mb = 512
embedding_cache_size = 10000
ast_cache_size = 1000

[embeddings]
enabled = true
model = "local"  # or "openai"

[embeddings.local]
model_path = "models/code-embeddings.onnx"

[sync]
watch_enabled = true
watch_debounce_ms = 100
sync_on_query = true

[snapshots]
auto_snapshot = true
max_snapshots = 20
retention_days = 30
```

---

## Next: See DECISIONS.md for architectural rationale
