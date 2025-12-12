# Mímir Architectural Decisions

This document explains the key architectural decisions and their rationale.

---

## Comparison with Existing Systems

```
┌─────────────────┬──────────────┬──────────────┬──────────────┬─────────────┐
│ Feature         │ Claude Code  │ Cursor       │ Aider        │ Mímir       │
├─────────────────┼──────────────┼──────────────┼──────────────┼─────────────┤
│ Context persist │ ❌ Per-chat   │ ❌ Per-chat   │ ❌ Per-chat   │ ✅ Always    │
│ Task resume     │ ❌ Manual     │ ❌ Manual     │ ❌ Manual     │ ✅ Automatic │
│ Crash recovery  │ ❌ Lost       │ ❌ Lost       │ ❌ Lost       │ ✅ Checkpoint│
│ Context mgmt    │ ❌ Fills up   │ ❌ Fills up   │ 🟡 Basic     │ ✅ Hierarchical│
│ Symbol graph    │ ❌ Grep       │ 🟡 LSP-based │ ❌ Grep       │ ✅ Native    │
│ Cross-language  │ ❌ None       │ ❌ None       │ ❌ None       │ ✅ Bridges   │
│ Edit safety     │ 🟡 Diff-based│ 🟡 Diff-based│ 🟡 Diff-based│ ✅ AST+Lock  │
│ Progress viz    │ ❌ None       │ ❌ None       │ ❌ None       │ ✅ Full      │
│ Rollback        │ ❌ Git manual │ ❌ Git manual │ 🟡 Git auto  │ ✅ Snapshots │
│ Long tasks      │ ❌ Context die│ ❌ Context die│ ❌ Context die│ ✅ Planned   │
└─────────────────┴──────────────┴──────────────┴──────────────┴─────────────┘
```

### What Mímir Does Better

**1. Never Lose Work**
```bash
# Current systems: Crash = start over
# Mímir: Crash = minor inconvenience

$ mimir recover
Found interrupted task: refactor-auth (crashed 10 min ago)
Last checkpoint: step 4/6, 94% complete
Recovering... ✓
Continuing from where you left off.
```

**2. Intelligent Context, Not Dumb Truncation**
```bash
# Current systems: Context full → old messages deleted randomly
# Mímir: Context full → smart compression + summarization

Context management:
├── Core context (5k tokens) - never compressed
├── Session context (50k tokens) - LRU + summarization
└── Working context (variable) - cleared between steps
```

**3. Understand Code, Not Just Text**
```bash
# Current systems:
grep "processUser" → 47 matches, figure it out yourself

# Mímir:
mimir trace processUser →
  Definition: exact location + signature
  Call hierarchy: who calls it, what it calls
  Similar functions: semantic matches
  Edit targets: exact byte offsets for safe editing
  Cross-language: frontend fetch() that calls this endpoint
```

**4. Safe Edits with Verification**
```bash
# Current systems:
# "Let me edit that function... oops, I matched the wrong one"

# Mímir:
Editing processUser in src/handlers.rs...
Pre-edit verification:
  ✓ Symbol resolved unambiguously
  ✓ File unchanged since analysis (hash: a1b2c3)
  ✓ No syntax errors in new code
  ✓ All imports satisfied
Applied edit. Post-verification:
  ✓ File parses correctly
  ✓ Symbol still resolvable
  ✓ No broken references
```

**5. Real Progress Tracking**
```bash
# Current systems: Black box, no idea what's happening

# Mímir:
Progress: ████████████░░░░░░░░ 60%
Completed: create_user, update_user, delete_user
In progress: validate_user (3/7 cases done)
Remaining: authenticate, refresh_token
Checkpoints: 4 (last: 30s ago)
```

**6. Predictable, Plannable Work**
```bash
# Current systems: "I'll try this... no wait, let me try that..."

# Mímir:
Proposed plan for: add-db-caching
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

Analysis:
  • Found 23 database query functions
  • 18 are cacheable (pure queries)
  • 5 have side effects (skip caching)

Plan:
  1. Create cache module (crates/cache)
  2. Add cache trait and implementations
  3. Update 18 query functions
  4. Add cache invalidation to mutations
  5. Run tests

Risk: LOW (additive changes, no breaking changes)
```

---

## Decision 1: Lock-Free Reads + Channel-Serialized Writes

### Problem

Two naive approaches have issues:

**Naive "everything through channels"**:
```
┌─────────┐    channel    ┌─────────┐
│ Client  │ ───────────► │  Actor  │  single thread bottleneck!
└─────────┘              └─────────┘
```

**Naive "everything lock-free"**:
```
Multiple clients ──► Complex locks + atomics = hard to reason about
```

### Solution: Hybrid Approach

**Key Insight**: Operations have different characteristics.

```
╔═══════════════════════════════════════════════════════════════════════════╗
║  READ-HEAVY, CONCURRENT         │  WRITE-SERIALIZED, ORDERED              ║
╠═════════════════════════════════╪═════════════════════════════════════════╣
║  • Symbol lookup                │  • Add/remove symbols                   ║
║  • Reference traversal          │  • Update graph edges                   ║
║  • Embedding search             │  • File sync                            ║
║  • Cache access                 │  • Checkpoint creation                  ║
║                                 │  • Task state transitions               ║
║                                 │                                         ║
║  → Lock-free structures         │  → Channel + dedicated writer           ║
║  → Direct access                │  → Serialized access                    ║
║  → Maximum parallelism          │  → Guaranteed ordering                  ║
╚═══════════════════════════════════════════════════════════════════════════╝
```

### Architecture

**"Readers Roam Free, Writers Go Through Gates"**

```
                         READERS (direct lock-free access)
                                          │
        ┌─────────────────────────────────┼─────────────────────────────────┐
        │                                 │                                 │
        ▼                                 ▼                                 ▼
┌───────────────┐               ┌───────────────┐               ┌───────────────┐
│   DashMap     │               │   Petgraph    │               │    HNSW       │
│   Symbols     │               │   (ArcSwap)   │               │   Embeddings  │
│               │               │               │               │               │
│  read: O(1)   │               │  read: O(1)   │               │  read: O(log) │
│  lock-free    │               │  lock-free    │               │  lock-free    │
└───────────────┘               └───────────────┘               └───────────────┘
        ▲                                 ▲                                 ▲
        │                                 │                                 │
        └─────────────────────────────────┼─────────────────────────────────┘
                                          │
                         WRITERS (through channel to coordinator)
                                          │
                                          ▼
                         ┌─────────────────────────────────────┐
                         │        WRITE COORDINATOR             │
                         │  • Receives mutations via channel   │
                         │  • Applies them sequentially        │
                         │  • Guarantees consistency           │
                         └─────────────────────────────────────┘
```

---

## Decision 2: Data Structure Selection

### Symbol Table: DashMap

```
Character: Many reads, rare writes, fine-grained updates
Structure: DashMap<SymbolId, Arc<Symbol>>

Read:  dashmap.get(&id) → direct, lock-free
Write: Through channel → coordinator insert/remove

Why: Sharded locks, but for reads practically lock-free.
     Update of one symbol doesn't block reading another.
```

### Relationship Graph: ArcSwap

```
Character: Many traversals, batch updates during sync
Structure: ArcSwap<Graph>

Read:  arc_swap.load() → get Arc<Graph>, traverse freely
Write: Coordinator builds new graph, arc_swap.store()

Why: Graph traversal needs consistent snapshot.
     We don't want edges disappearing mid-traversal.
     Batch updates are more efficient than fine-grained.
```

### Embedding Index: ArcSwap + Append Buffer

```
Character: Search-heavy, batch inserts, rare deletion
Structure: ArcSwap<HnswIndex> + append-only buffer

Read:  arc_swap.load() → search in snapshot
Write: Append to buffer → periodic rebuild + swap

Why: HNSW isn't easily mutable. Batch rebuild is efficient.
     Search must be fast, insert can be batched.
```

### Content Store: Fully Lock-Free

```
Character: Append-only (content-addressed = never update)
Structure: Fully lock-free

Read:  Direct read from filesystem (cached)
Write: Atomic write (write to tmp, rename)

Why: Content-addressed = idempotent writes.
     If two write the same thing - one succeeds, result identical.
```

---

## Decision 3: Immutable Snapshots + Atomic Swap

### Principle

Instead of mutating structures, create **new versions** and **swap atomically**.

```
Reader 1 ────► Arc<Graph v1> ◄──── Reader 2
                    │
                    │ (still reading v1)
                    │
Writer ──────► Creates Graph v2
                    │
                    ▼
              AtomicSwap!
                    │
                    ▼
Reader 3 ────► Arc<Graph v2>

Readers 1,2 still have Arc<v1> - safe!
When they drop the reference, v1 is freed.
```

### Benefits

- No reader ever blocks
- Readers see consistent snapshots
- Writers don't need to coordinate with readers
- Old versions freed automatically via reference counting

---

## Decision 4: Handle Pattern

### Anatomy

Handle is a "smart pointer" to a module:
- Clone: O(1) - just Arc clone
- Send + Sync: can be shared between threads
- Cheap: contains only Arc references and channel senders

```rust
struct SymbolsHandle {
    // Lock-free read access
    data: Arc<DashMap<SymbolId, Arc<Symbol>>>,
    name_index: Arc<DashMap<String, Vec<SymbolId>>>,

    // Write channel
    write_tx: mpsc::Sender<SymbolCommand>,

    // Event broadcast
    events: broadcast::Sender<SymbolEvent>,
}
```

### Properties

- **Clone is cheap** - O(1), just Arc clones
- **Share freely** - between threads, tasks, components
- **Module lives as long as handles exist** - natural lifetime

---

## Decision 5: Event-Based Loose Coupling

### Pattern

Modules emit events without knowing who listens:

```
              COMMANDS (bounded mpsc)
                         │
                         ▼
              ┌─────────────────────┐
              │    COORDINATOR      │
              │  Applies mutations  │
              │  Broadcasts events  │
              └──────────┬──────────┘
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

### Benefits

- Emitters don't know about listeners
- Listeners filter what interests them
- Adding new listener requires no emitter changes
- Easy to add logging, metrics, debugging

---

## Decision 6: Module Independence

### Principle

Every module is a **self-contained unit** with a clear interface.

```
┌─────────────────────────────────────────────────────────────────────┐
│                         MODULE                                       │
│                                                                      │
│  ┌───────────────────────────────────────────────────────────────┐  │
│  │                      PUBLIC INTERFACE                          │  │
│  │                                                                │  │
│  │  • Handle (cloneable, cheap, thread-safe)                     │  │
│  │  • Config (how to configure)                                  │  │
│  │  • Events (what it emits)                                      │  │
│  │                                                                │  │
│  └───────────────────────────────────────────────────────────────┘  │
│                              │                                       │
│                              │ owns                                  │
│                              ▼                                       │
│  ┌───────────────────────────────────────────────────────────────┐  │
│  │                    INTERNAL STATE                              │  │
│  │                                                                │  │
│  │  • Data structures (private)                                  │  │
│  │  • Background tasks (private)                                 │  │
│  │  • Channels (private)                                         │  │
│  │                                                                │  │
│  └───────────────────────────────────────────────────────────────┘  │
│                                                                      │
└─────────────────────────────────────────────────────────────────────┘
```

### Benefits

- Modules don't know about each other directly
- Communication only through handles and events
- No shared mutable state
- Easy to test in isolation
- Easy to replace implementations

---

## Decision 7: Layered Architecture

### Dependency Rule

**Layers can ONLY depend on layers below them. Never upward, never sideways.**

```
Layer 7: Application (CLI, TUI)
Layer 6: Agent (LLM, tools)
Layer 5: Execution (tasks, edits, context)
Layer 4: Graph (facade, queries)
Layer 3: Index (symbols, relations, files, embeddings)
Layer 2: Parsing (language adapters)
Layer 1: Storage (content, cache, persist)
Layer 0: Primitives (types, errors)
```

### Benefits

- Clear dependency direction
- No circular dependencies possible
- Lower layers are stable, upper layers can change
- Easy to understand and maintain

---

## Risk Assessment

| Risk | Probability | Impact | Mitigation |
|------|-------------|--------|------------|
| Tree-sitter parse failures | Medium | High | Fallback to regex, error recovery |
| Symbol resolution accuracy | High | Medium | Layered approach, optional LSP |
| Memory usage on large projects | Medium | High | Tiered cache, lazy loading |
| Cross-language bridge accuracy | High | Medium | Confidence levels, manual annotations |
| Embedding model quality | Medium | Medium | Fine-tuning, multiple models |
| Concurrent edit conflicts | Low | Medium | Optimistic locking, conflict detection |

### Mitigation: Tiered Memory Management

```rust
pub struct TieredSymbolStore {
    // Hot: recently accessed, in memory
    hot: moka::sync::Cache<SymbolId, Arc<Symbol>>,

    // Warm: same module, in memory but evictable
    warm: moka::sync::Cache<SymbolId, Arc<Symbol>>,

    // Cold: everything else, on disk
    cold: sled::Db,
}

impl TieredSymbolStore {
    pub fn get(&self, id: SymbolId) -> Option<Arc<Symbol>> {
        // Try hot first
        if let Some(s) = self.hot.get(&id) {
            return Some(s);
        }

        // Try warm
        if let Some(s) = self.warm.get(&id) {
            self.hot.insert(id, s.clone()); // Promote to hot
            return Some(s);
        }

        // Load from cold
        if let Ok(Some(bytes)) = self.cold.get(id.to_bytes()) {
            let symbol: Symbol = bincode::deserialize(&bytes).ok()?;
            let arc = Arc::new(symbol);
            self.warm.insert(id, arc.clone()); // Add to warm
            return Some(arc);
        }

        None
    }
}
```

### Mitigation: Bridge Confidence Tracking

```rust
pub struct BridgeMatch {
    pub bridge: Bridge,
    pub confidence: f32,
    pub evidence: Vec<Evidence>,
}

pub enum Evidence {
    ExactPathMatch,
    PatternMatch { pattern: String },
    FrameworkConvention,
    UserAnnotation,
    ConfigFile { path: PathBuf },
}

impl BridgeMatch {
    pub fn should_include(&self, threshold: f32) -> bool {
        self.confidence >= threshold
    }
}
```

---

## Summary: 7 Architectural Principles

```
1. READS ARE FREE
   Lock-free access to data structures.
   Readers never wait for writers.

2. WRITES ARE SERIALIZED
   All writes go through channel.
   Coordinator applies sequentially.
   Guarantees consistency without locks.

3. MODULES ARE INDEPENDENT
   Every module is self-contained.
   Communication only through handles and events.
   No shared mutable state.

4. HANDLES ARE CHEAP
   Clone is O(1). Share freely between threads.
   Module lives as long as handles exist.

5. EVENTS FOR LOOSE COUPLING
   Modules don't know who listens.
   Consumers filter what interests them.
   Adding new listener requires no emitter change.

6. SNAPSHOTS FOR CONSISTENCY
   Complex structures (Graph) use ArcSwap.
   Readers see consistent snapshot.
   Writers build new version, swap atomically.

7. LAYERS ONLY DEPEND DOWNWARD
   Never circular dependencies.
   Upper layers compose lower layers.
   Lower layers don't know about upper.
```

---

## Configuration Decisions

### Storage Layout

```
.agent/                         # Mímir state directory
├── config.toml                 # User configuration
├── objects/                    # Content-addressed storage
├── index/                      # Serialized indexes
├── graph/                      # Graph state
├── embeddings/                 # Vector embeddings
├── tasks/                      # Task state
│   ├── active/
│   ├── completed/
│   └── failed/
├── knowledge/                  # Learned project info
└── snapshots/                  # Point-in-time snapshots
```

### Why `.agent/` Not `.mimir/`?

- More generic - could be used by other tools
- Shorter to type
- `.mimir/` is an alternative if preferred

### Why TOML for Config?

- Human readable and editable
- Good library support in Rust
- Standard in Rust ecosystem (Cargo.toml)
- Better than YAML for config (no quirks)

### Why bincode for Internal State?

- Very fast serialization
- Compact binary format
- Good for internal state that doesn't need to be human-readable
- JSON for task files (debugging/inspection)

---

## Next: See ARCHITECTURE.md for full structure
