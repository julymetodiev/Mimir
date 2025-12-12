# Mímir Architecture

## Vision

**Semantic Code Intelligence Engine** - структурирано разбиране на код за AI агенти.

```
Традиционно:  grep "process_data" → 47 matches → read file1 → read file2 → ръчно
Mímir:        trace("process_data") → definition + callers + callees + similar + edit_targets
```

### What Mímir Is NOT
- Not an IDE
- Not an LSP server (but can use LSP)
- Not a Git wrapper
- Not another "AI coding assistant"

### What Mímir IS
Semantic Code Intelligence Engine - gives AI agents structured understanding of code instead of raw text files.

### Success Criteria
- **Accuracy**: >85% symbol resolution without LSP, >95% with LSP
- **Performance**: <100ms for trace() in 50k LOC project
- **Cold start**: <5s to first working query (progressive)
- **Memory**: <500MB for 100k LOC project
- **Languages**: Rust, TypeScript, Python, Go in V1

---

## Core Principles

```
1. READS ARE FREE        Lock-free достъп. Читателите никога не чакат writers.
2. WRITES ARE SERIALIZED Всички writes през канал → coordinator прилага последователно.
3. MODULES ARE INDEPENDENT  Self-contained. Комуникация само през handles + events.
4. HANDLES ARE CHEAP     Clone е O(1). Share freely между threads.
5. EVENTS FOR LOOSE COUPLING  Модулите не знаят кой слуша.
6. SNAPSHOTS FOR CONSISTENCY  ArcSwap за consistent reads.
7. LAYERS ONLY DEPEND DOWNWARD  Never circular dependencies.
```

---

## 8-Layer Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│  LAYER 7: APPLICATION                                                        │
│  mimir-cli, mimir-tui                                                        │
│  Entry points. Wires everything together.                                    │
├─────────────────────────────────────────────────────────────────────────────┤
│  LAYER 6: AGENT                                                              │
│  mimir-llm, mimir-tools, mimir-agent                                         │
│  LLM orchestration, tool definitions, agent loop.                            │
├─────────────────────────────────────────────────────────────────────────────┤
│  LAYER 5: EXECUTION                                                          │
│  mimir-edit, mimir-tasks, mimir-context, mimir-executor                      │
│  Task lifecycle, editing, recovery, LLM context management.                  │
├─────────────────────────────────────────────────────────────────────────────┤
│  LAYER 4: GRAPH                                                              │
│  mimir-graph, mimir-graph-actor                                              │
│  Facade over indexes. Unified query interface. Coordinates sync.             │
├─────────────────────────────────────────────────────────────────────────────┤
│  LAYER 3: INDEX                                                              │
│  mimir-symbols, mimir-relations, mimir-files, mimir-embeddings               │
│  Stateful. Lock-free reads, channel writes. Each exposes Handle.             │
├─────────────────────────────────────────────────────────────────────────────┤
│  LAYER 2: PARSING                                                            │
│  mimir-parse-core, mimir-parse-rust, mimir-parse-ts, mimir-parse-py, ...     │
│  Stateless. parse(source) → tree + symbols. Fully parallelizable.            │
├─────────────────────────────────────────────────────────────────────────────┤
│  LAYER 1: STORAGE                                                            │
│  mimir-content, mimir-cache, mimir-persist, mimir-snapshot                   │
│  I/O operations. Lock-free where possible. No business logic.                │
├─────────────────────────────────────────────────────────────────────────────┤
│  LAYER 0: PRIMITIVES                                                         │
│  mimir-types, mimir-error, mimir-protocol                                    │
│  Pure data types. No logic. No async. No channels.                           │
└─────────────────────────────────────────────────────────────────────────────┘
```

**Dependency Rule**: Layers can ONLY depend on layers below them. Never upward, never sideways.

---

## Full Workspace Structure

```
mimir/
├── Cargo.toml                      # Workspace root
├── rust-toolchain.toml
├── .cargo/config.toml
│
├── crates/
│   │
│   │  ╔═══════════════════════════════════════════════════════════╗
│   │  ║                     LAYER 0: PRIMITIVES                   ║
│   │  ║         Zero dependencies on other mimir crates           ║
│   │  ╚═══════════════════════════════════════════════════════════╝
│   │
│   ├── mimir-types/                 # Foundational types
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── id.rs               # SymbolId, FileId, TaskId, etc.
│   │       ├── symbol.rs           # Symbol, SymbolKind, QualifiedName
│   │       ├── reference.rs        # Reference, ReferenceKind
│   │       ├── location.rs         # Location, Span, Range
│   │       └── language.rs         # Language enum
│   │
│   ├── mimir-error/                 # Error types
│   │   └── src/
│   │       ├── lib.rs
│   │       └── errors.rs           # MimirError, StorageError, etc.
│   │
│   ├── mimir-protocol/              # Inter-module communication
│   │   └── src/
│   │       ├── commands.rs         # GraphCommand, TaskCommand
│   │       ├── events.rs           # GraphEvent, TaskEvent
│   │       └── responses.rs        # TraceResult, EditResult
│   │
│   │  ╔═══════════════════════════════════════════════════════════╗
│   │  ║                    LAYER 1: STORAGE                       ║
│   │  ║              Depends only on: Layer 0                     ║
│   │  ╚═══════════════════════════════════════════════════════════╝
│   │
│   ├── mimir-content/               # Content-addressed storage
│   │   └── src/
│   │       ├── traits.rs           # ContentStore trait
│   │       ├── backend/
│   │       │   ├── filesystem.rs   # File-based backend
│   │       │   └── memory.rs       # In-memory for testing
│   │       └── compression.rs      # Zstd compression
│   │
│   ├── mimir-cache/                 # Caching layer
│   │   └── src/
│   │       ├── traits.rs           # Cache trait
│   │       ├── lru.rs              # LRU cache (moka)
│   │       └── tiered.rs           # Hot/warm/cold tiered cache
│   │
│   ├── mimir-persist/               # Serialization
│   │   └── src/
│   │       └── lib.rs              # bincode serialization
│   │
│   ├── mimir-snapshot/              # Snapshot & restore
│   │   └── src/
│   │       ├── snapshot.rs         # Snapshot, SnapshotId
│   │       ├── create.rs
│   │       ├── restore.rs
│   │       └── gc.rs               # Garbage collection
│   │
│   │  ╔═══════════════════════════════════════════════════════════╗
│   │  ║                   LAYER 2: PARSING                        ║
│   │  ║            Depends on: Layer 0, Layer 1                   ║
│   │  ╚═══════════════════════════════════════════════════════════╝
│   │
│   ├── mimir-parse-core/            # Language adapter infrastructure
│   │   └── src/
│   │       ├── traits.rs           # LanguageAdapter trait
│   │       ├── registry.rs         # AdapterRegistry
│   │       ├── extracted.rs        # ExtractedSymbol, ExtractedReference
│   │       └── bridge.rs           # Bridge, BridgeKind
│   │
│   ├── mimir-parse-rust/            # Rust language support
│   │   └── src/
│   │       ├── adapter.rs          # RustAdapter impl
│   │       ├── symbols.rs          # Symbol extraction
│   │       ├── references.rs       # Reference extraction
│   │       ├── bridges.rs          # Axum/Actix endpoint detection
│   │       └── queries/*.scm       # Tree-sitter queries
│   │
│   ├── mimir-parse-ts/              # TypeScript/JavaScript
│   ├── mimir-parse-py/              # Python
│   ├── mimir-parse-go/              # Go
│   │
│   │  ╔═══════════════════════════════════════════════════════════╗
│   │  ║                     LAYER 3: INDEX                        ║
│   │  ║         Depends on: Layer 0, Layer 1, Layer 2             ║
│   │  ╚═══════════════════════════════════════════════════════════╝
│   │
│   ├── mimir-symbols/               # Symbol table (DashMap)
│   │   └── src/
│   │       ├── table.rs            # DashMap<SymbolId, Arc<Symbol>>
│   │       ├── index.rs            # Name index, file index
│   │       └── handle.rs           # SymbolsHandle
│   │
│   ├── mimir-relations/             # Relationship graph (ArcSwap<petgraph>)
│   │   └── src/
│   │       ├── graph.rs            # petgraph wrapper
│   │       └── handle.rs           # RelationsHandle
│   │
│   ├── mimir-files/                 # File index (DashMap)
│   │   └── src/
│   │       ├── scanner.rs          # Directory scanning
│   │       ├── watcher.rs          # File system watcher
│   │       ├── ignore.rs           # .gitignore style patterns
│   │       └── handle.rs           # FilesHandle
│   │
│   ├── mimir-embeddings/            # Embedding index (ArcSwap<HNSW>)
│   │   └── src/
│   │       ├── encoder/
│   │       │   ├── local.rs        # ONNX-based
│   │       │   └── openai.rs       # API-based
│   │       ├── index/
│   │       │   └── hnsw.rs         # HNSW implementation
│   │       └── handle.rs           # EmbeddingsHandle
│   │
│   │  ╔═══════════════════════════════════════════════════════════╗
│   │  ║                     LAYER 4: GRAPH                        ║
│   │  ║              Depends on: Layers 0-3                       ║
│   │  ╚═══════════════════════════════════════════════════════════╝
│   │
│   ├── mimir-graph/                 # Graph facade
│   │   └── src/
│   │       ├── queries/
│   │       │   ├── trace.rs        # trace() implementation
│   │       │   ├── find.rs         # find_symbol, find_references
│   │       │   └── search.rs       # Semantic search
│   │       ├── mutations/
│   │       │   ├── add.rs
│   │       │   ├── remove.rs
│   │       │   └── update.rs
│   │       └── sync.rs             # Sync with filesystem
│   │
│   ├── mimir-graph-actor/           # Actor wrapper
│   │   └── src/
│   │       ├── actor.rs            # GraphActor
│   │       ├── handle.rs           # GraphHandle (client)
│   │       └── commands.rs         # Command processing
│   │
│   │  ╔═══════════════════════════════════════════════════════════╗
│   │  ║                   LAYER 5: EXECUTION                      ║
│   │  ║              Depends on: Layers 0-4                       ║
│   │  ╚═══════════════════════════════════════════════════════════╝
│   │
│   ├── mimir-edit/                  # File editing
│   │   └── src/
│   │       ├── operations/
│   │       │   ├── lines.rs        # Line-based edits
│   │       │   ├── bytes.rs        # Byte-range edits
│   │       │   ├── ast.rs          # AST-aware edits
│   │       │   └── rename.rs       # Symbol rename
│   │       ├── validation.rs
│   │       ├── conflict.rs
│   │       └── transaction.rs      # Atomic multi-file edits
│   │
│   ├── mimir-tasks/                 # Task management
│   │   └── src/
│   │       ├── task.rs             # Task, TaskStatus
│   │       ├── plan.rs             # TaskPlan, PlannedStep
│   │       ├── checkpoint.rs       # Checkpoint, recovery
│   │       └── store/
│   │           ├── filesystem.rs
│   │           └── memory.rs
│   │
│   ├── mimir-context/               # LLM context management
│   │   └── src/
│   │       ├── hierarchical.rs     # Core/Session/Working
│   │       ├── compression.rs
│   │       ├── summarizer.rs
│   │       └── loader.rs           # Smart code loading
│   │
│   ├── mimir-executor/              # Task execution engine
│   │   └── src/
│   │       ├── executor.rs
│   │       ├── steps/
│   │       │   ├── analyze.rs
│   │       │   ├── edit.rs
│   │       │   ├── shell.rs
│   │       │   └── verify.rs
│   │       ├── recovery.rs
│   │       └── shutdown.rs
│   │
│   │  ╔═══════════════════════════════════════════════════════════╗
│   │  ║                     LAYER 6: AGENT                        ║
│   │  ║              Depends on: Layers 0-5                       ║
│   │  ╚═══════════════════════════════════════════════════════════╝
│   │
│   ├── mimir-llm/                   # LLM provider abstraction
│   │   └── src/
│   │       ├── traits.rs           # LlmProvider trait
│   │       ├── message.rs
│   │       └── providers/
│   │           ├── anthropic.rs
│   │           ├── openai.rs
│   │           └── local.rs
│   │
│   ├── mimir-tools/                 # Agent tools
│   │   └── src/
│   │       ├── traits.rs           # Tool trait
│   │       ├── registry.rs
│   │       ├── builtin/
│   │       │   ├── trace.rs
│   │       │   ├── edit.rs
│   │       │   ├── search.rs
│   │       │   └── shell.rs
│   │       └── sandbox.rs
│   │
│   ├── mimir-agent/                 # Agent orchestration
│   │   └── src/
│   │       ├── agent.rs
│   │       ├── planner.rs
│   │       ├── loop.rs
│   │       └── prompts/
│   │
│   │  ╔═══════════════════════════════════════════════════════════╗
│   │  ║                   LAYER 7: APPLICATION                    ║
│   │  ║                 Depends on: everything                    ║
│   │  ╚═══════════════════════════════════════════════════════════╝
│   │
│   ├── mimir-cli/                   # Command-line interface
│   │   └── src/
│   │       ├── main.rs
│   │       └── commands/
│   │           ├── init.rs
│   │           ├── trace.rs
│   │           ├── search.rs
│   │           ├── task.rs
│   │           ├── status.rs
│   │           └── resume.rs
│   │
│   └── mimir-tui/                   # Terminal UI (optional)
│
├── tests/                           # Integration tests
│   ├── integration/
│   └── fixtures/
│
└── benches/                         # Benchmarks
```

---

## Data Structures by Access Pattern

### Lock-Free Read (DashMap)

Used for: **Fine-grained, high-frequency lookups**

```
┌─────────────────────────────────────────────────────────────────┐
│  SYMBOL TABLE                                                    │
│  DashMap<SymbolId, Arc<Symbol>>                                  │
│                                                                  │
│  Read:  dashmap.get(&id) → директно, lock-free                  │
│  Write: През канал → coordinator insert/remove                   │
│                                                                  │
│  Why: Sharded locks. Update на един символ не блокира друг.      │
└─────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────┐
│  FILE INDEX                                                      │
│  DashMap<PathBuf, FileEntry>                                     │
│                                                                  │
│  Read:  dashmap.get(&path) → директно                           │
│  Write: През канал при file system events                        │
└─────────────────────────────────────────────────────────────────┘
```

### Snapshot Read (ArcSwap)

Used for: **Complex structures requiring consistent traversal**

```
┌─────────────────────────────────────────────────────────────────┐
│  RELATIONSHIP GRAPH                                              │
│  ArcSwap<petgraph::Graph>                                        │
│                                                                  │
│  Read:  arc_swap.load() → получаваш Arc<Graph>, traverse free   │
│  Write: Coordinator builds new graph, arc_swap.store()           │
│                                                                  │
│  Why: Graph traversal needs consistent snapshot.                 │
│       Batch updates са по-ефективни от fine-grained.            │
└─────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────┐
│  EMBEDDING INDEX                                                 │
│  ArcSwap<HnswIndex> + append-only buffer                         │
│                                                                  │
│  Read:  arc_swap.load() → search в snapshot                     │
│  Write: Append to buffer → периодично rebuild + swap             │
│                                                                  │
│  Why: HNSW не е лесно mutable. Batch rebuild е по-ефективен.    │
└─────────────────────────────────────────────────────────────────┘
```

### Fully Lock-Free (Content-Addressed)

```
┌─────────────────────────────────────────────────────────────────┐
│  CONTENT STORE                                                   │
│  Hash → file on disk (zstd compressed)                           │
│                                                                  │
│  Read:  Директно от filesystem (+ memory cache)                 │
│  Write: Атомарен write (write to tmp, rename)                    │
│                                                                  │
│  Why: Content-addressed = идемпотентни writes.                  │
│       Ако двама пишат същото - един успява, резултатът е същият.│
└─────────────────────────────────────────────────────────────────┘
```

---

## Module Interconnection

Modules connect through **HANDLES**, not direct access to internals:

```
                    ┌─────────────────────┐
                    │     Application     │
                    │  Holds all handles  │
                    └──────────┬──────────┘
                               │
         ┌─────────────────────┼─────────────────────┐
         │                     │                     │
         ▼                     ▼                     ▼
  ┌─────────────┐       ┌─────────────┐       ┌─────────────┐
  │GraphHandle  │       │ TaskHandle  │       │ AgentHandle │
  │ Clone: O(1) │       │ Clone: O(1) │       │ Clone: O(1) │
  │ Send + Sync │       │ Send + Sync │       │ Send + Sync │
  └──────┬──────┘       └──────┬──────┘       └──────┬──────┘
         │ channel             │ channel             │ channel
         ▼                     ▼                     ▼
  ┌─────────────┐       ┌─────────────┐       ┌─────────────┐
  │   Graph     │       │    Task     │       │   Agent     │
  │  Internal   │       │  Internal   │       │  Internal   │
  └─────────────┘       └─────────────┘       └─────────────┘
```

---

## Data Flow Patterns

### Pattern 1: Query (Read Path) - NO CHANNEL

```
Client              Handle                Internal Data
  │                   │                        │
  │  trace("foo")     │                        │
  │──────────────────►│                        │
  │                   │  symbols.get("foo")    │
  │                   │───────────────────────►│ DashMap (lock-free)
  │                   │◄───────────────────────│
  │                   │  graph.load().traverse │
  │                   │───────────────────────►│ ArcSwap (snapshot)
  │                   │◄───────────────────────│
  │   TraceResult     │                        │
  │◄──────────────────│                        │

  Note: NO channel, no async wait. Direct, parallel, fast.
```

### Pattern 2: Mutation (Write Path) - THROUGH CHANNEL

```
Client          Handle           Coordinator         Data
  │               │                   │                │
  │  edit(op)     │                   │                │
  │──────────────►│                   │                │
  │               │  Command::Edit    │                │
  │               │  + oneshot reply  │                │
  │               │──────────────────►│                │
  │               │                   │  apply         │
  │               │                   │───────────────►│
  │               │                   │  broadcast     │
  │               │                   │═══════════════►│
  │               │    EditResult     │                │
  │               │◄──────────────────│                │
  │  EditResult   │                   │                │
  │◄──────────────│                   │                │

  Note: Write is serialized, but doesn't block reads.
```

### Pattern 3: Sync (Batch Update)

```
FileWatcher       Coordinator                 Data
    │                  │                        │
    │  FileChanged     │                        │
    │─────────────────►│                        │
    │  (debounce...)   │                        │
    │                  │  1. Scan changes       │
    │                  │  2. Parse files        │
    │                  │  3. Build new snapshot │
    │                  │                        │
    │                  │  arc_swap.store(new)   │
    │                  │───────────────────────►│
    │                  │  broadcast(SyncDone)   │
    │                  │═══════════════════════►│

  Note: Changes are debounced, applied atomically.
        Readers see either old or new state, never partial.
```

---

## Event System

### Event Taxonomy

```
GRAPH EVENTS
├── SymbolAdded { symbol_id, name, kind }
├── SymbolRemoved { symbol_id }
├── SymbolUpdated { symbol_id, changes }
├── ReferenceAdded { from, to, kind }
├── ReferenceRemoved { from, to }
├── SyncStarted { files_count }
├── SyncProgress { processed, total }
├── SyncCompleted { stats }
└── SyncFailed { error }

TASK EVENTS
├── TaskCreated { task_id, description }
├── TaskStarted { task_id }
├── StepStarted { task_id, step_id }
├── StepCompleted { task_id, step_id }
├── CheckpointCreated { task_id, checkpoint_id }
├── TaskPaused { task_id, reason }
├── TaskResumed { task_id }
├── TaskCompleted { task_id, summary }
└── TaskFailed { task_id, error }

EDIT EVENTS
├── EditStarted { file, operation }
├── EditCompleted { file, changes }
├── EditFailed { file, error }
├── ConflictDetected { file, details }
└── TransactionCommitted { files }

SYSTEM EVENTS
├── ShutdownRequested
├── ShutdownComplete
└── ErrorOccurred { module, error }
```

### Event Subscription

```
                    ┌─────────────────────────────┐
                    │      Event Bus              │
                    │   (broadcast channel)       │
                    └──────────────┬──────────────┘
                                   │
       ┌───────────────────────────┼───────────────────────────┐
       │                           │                           │
       ▼                           ▼                           ▼
┌─────────────────┐        ┌─────────────────┐        ┌─────────────────┐
│ Embedding       │        │ Progress        │        │ Task Manager    │
│ Updater         │        │ Reporter        │        │                 │
├─────────────────┤        ├─────────────────┤        ├─────────────────┤
│ Subscribes to:  │        │ Subscribes to:  │        │ Subscribes to:  │
│ • SymbolAdded   │        │ • ALL events    │        │ • SyncCompleted │
│ • SymbolUpdated │        │   (for display) │        │ • EditCompleted │
│ • SymbolRemoved │        │                 │        │ • EditFailed    │
└─────────────────┘        └─────────────────┘        └─────────────────┘
```

---

## Channel Topology

```
                              COMMANDS (bounded mpsc)
    ┌──────────────────────────────────────────────────────────────────┐
    │                                                                  │
    │   ┌─────────┐      ┌─────────┐      ┌─────────┐                 │
    │   │  CLI    │      │  Agent  │      │  Watcher│                 │
    │   └────┬────┘      └────┬────┘      └────┬────┘                 │
    │        │                │                │                       │
    │        └────────────────┼────────────────┘                       │
    │                         │                                        │
    │                         ▼                                        │
    │              ┌─────────────────────┐                            │
    │              │   Command Channel   │  bounded, backpressure     │
    │              └──────────┬──────────┘                            │
    │                         │                                        │
    │                         ▼                                        │
    │              ┌─────────────────────┐                            │
    │              │    COORDINATOR      │                            │
    │              │  Applies mutations  │                            │
    │              │  Broadcasts events  │                            │
    │              └──────────┬──────────┘                            │
    └─────────────────────────┼────────────────────────────────────────┘
                              │
                              │ EVENTS (broadcast)
                              ▼
              ┌───────────────────────────────────┐
              │        Event Broadcast            │
              └───────────────┬───────────────────┘
                              │
           ┌──────────────────┼──────────────────┐
           ▼                  ▼                  ▼
    ┌─────────────┐    ┌─────────────┐    ┌─────────────┐
    │  Embedding  │    │    Task     │    │    CLI      │
    │  Updater    │    │   Manager   │    │  Progress   │
    └─────────────┘    └─────────────┘    └─────────────┘
```

---

## Storage Layout

```
project/
├── .agent/                         # Mímir state (gitignored)
│   ├── config.toml                 # Configuration
│   │
│   ├── objects/                    # Content-addressed (Blake3 + zstd)
│   │   ├── ab/
│   │   │   └── cdef1234...
│   │   └── ...
│   │
│   ├── index/
│   │   ├── files.bin               # FileId → FileEntry
│   │   ├── symbols.bin             # SymbolId → Symbol
│   │   └── hashes.bin              # Path → ContentHash
│   │
│   ├── graph/
│   │   ├── edges.bin               # petgraph serialized
│   │   └── bridges.bin             # cross-language bridges
│   │
│   ├── embeddings/
│   │   ├── vectors.bin             # raw f32 vectors
│   │   └── hnsw.bin                # HNSW index
│   │
│   ├── tasks/
│   │   ├── active/
│   │   │   └── task_xxx/
│   │   │       ├── task.json
│   │   │       ├── plan.json
│   │   │       ├── context.bin
│   │   │       └── checkpoints/
│   │   ├── completed/
│   │   └── failed/
│   │
│   ├── knowledge/
│   │   ├── project.json
│   │   ├── patterns.json
│   │   └── decisions.json
│   │
│   └── snapshots/
│       └── snap_xxx/
│
└── src/                            # User's code (NOT TOUCHED by Mímir)
```

---

## Key Dependencies

```toml
# Layer 0
thiserror = "1.0"
serde = { version = "1.0", features = ["derive"] }

# Layer 1
blake3 = "1.5"
zstd = "0.13"
moka = { version = "0.12", features = ["sync"] }
bincode = "1.3"

# Layer 2
tree-sitter = "0.22"
tree-sitter-rust = "0.21"
tree-sitter-typescript = "0.21"

# Layer 3
dashmap = "5.5"
arc-swap = "1.7"
petgraph = "0.6"

# Async runtime
tokio = { version = "1.36", features = ["full"] }

# Channels
async-channel = "2.0"
```

---

## Next Documents

- **TRAITS.md** - Core trait definitions
- **PHASES.md** - Implementation phases
- **DECISIONS.md** - Architectural decisions
- **TASKS.md** - Task management system
- **CONTEXT.md** - LLM context management
- **CLI.md** - Usage examples
