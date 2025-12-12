# Mímir Core Traits & Types

This document defines the complete type system and trait interfaces.

---

## Layer 0: Primitive Types

### ID Types

All IDs use a consistent pattern via macro:

```rust
// mimir-types/src/id.rs

use std::fmt;
use serde::{Serialize, Deserialize};

macro_rules! define_id {
    ($name:ident) => {
        #[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(u64);

        impl $name {
            pub const fn new(value: u64) -> Self {
                Self(value)
            }

            pub const fn value(&self) -> u64 {
                self.0
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}({})", stringify!($name), self.0)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }
    };
}

define_id!(SymbolId);
define_id!(FileId);
define_id!(TaskId);
define_id!(StepId);
define_id!(CheckpointId);
define_id!(SnapshotId);
```

### ContentHash

```rust
/// Content hash (Blake3, 32 bytes)
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ContentHash([u8; 32]);

impl ContentHash {
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }
}

impl fmt::Debug for ContentHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ContentHash({})", &self.to_hex()[..12])
    }
}
```

### Location & Span

```rust
// mimir-types/src/location.rs

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Location {
    pub file_id: FileId,
    pub line: u32,      // 1-indexed
    pub column: u32,    // 1-indexed
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Span {
    pub start: u32,     // byte offset
    pub end: u32,       // byte offset
}

impl Span {
    pub fn new(start: u32, end: u32) -> Self {
        debug_assert!(start <= end);
        Self { start, end }
    }

    pub fn len(&self) -> u32 {
        self.end - self.start
    }

    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    pub fn contains(&self, offset: u32) -> bool {
        offset >= self.start && offset < self.end
    }

    pub fn overlaps(&self, other: &Span) -> bool {
        self.start < other.end && other.start < self.end
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Range {
    pub start_line: u32,
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
}
```

### Language

```rust
// mimir-types/src/language.rs

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    Rust,
    TypeScript,
    JavaScript,
    Python,
    Go,
    Java,
    Kotlin,
    C,
    Cpp,
    Unknown,
}

impl Language {
    pub fn from_extension(ext: &str) -> Self {
        match ext {
            "rs" => Self::Rust,
            "ts" | "tsx" => Self::TypeScript,
            "js" | "jsx" | "mjs" => Self::JavaScript,
            "py" | "pyi" => Self::Python,
            "go" => Self::Go,
            "java" => Self::Java,
            "kt" | "kts" => Self::Kotlin,
            "c" | "h" => Self::C,
            "cpp" | "hpp" | "cc" | "cxx" => Self::Cpp,
            _ => Self::Unknown,
        }
    }

    pub fn extensions(&self) -> &'static [&'static str] {
        match self {
            Self::Rust => &["rs"],
            Self::TypeScript => &["ts", "tsx"],
            Self::JavaScript => &["js", "jsx", "mjs"],
            Self::Python => &["py", "pyi"],
            Self::Go => &["go"],
            Self::Java => &["java"],
            Self::Kotlin => &["kt", "kts"],
            Self::C => &["c", "h"],
            Self::Cpp => &["cpp", "hpp", "cc", "cxx"],
            Self::Unknown => &[],
        }
    }
}
```

---

## Symbol Types

### Symbol

```rust
// mimir-types/src/symbol.rs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Symbol {
    pub id: SymbolId,
    pub name: String,
    pub qualified_name: QualifiedName,
    pub kind: SymbolKind,
    pub language: Language,
    pub file_id: FileId,
    pub location: Location,
    pub span: Span,
    pub signature: Option<Signature>,
    pub visibility: Visibility,
    pub doc: Option<String>,
}
```

### QualifiedName

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct QualifiedName {
    segments: Vec<String>,
}

impl QualifiedName {
    pub fn new(segments: Vec<String>) -> Self {
        debug_assert!(!segments.is_empty(), "QualifiedName cannot be empty");
        Self { segments }
    }

    pub fn from_str(s: &str, separator: &str) -> Self {
        Self::new(s.split(separator).map(String::from).collect())
    }

    /// Last segment (the simple name)
    pub fn name(&self) -> &str {
        self.segments.last().expect("QualifiedName is never empty")
    }

    /// All segments except the last
    pub fn parent(&self) -> Option<Self> {
        if self.segments.len() > 1 {
            Some(Self::new(self.segments[..self.segments.len() - 1].to_vec()))
        } else {
            None
        }
    }

    pub fn segments(&self) -> &[String] {
        &self.segments
    }

    pub fn join(&self, separator: &str) -> String {
        self.segments.join(separator)
    }

    pub fn push(&mut self, segment: impl Into<String>) {
        self.segments.push(segment.into());
    }

    pub fn starts_with(&self, other: &Self) -> bool {
        self.segments.starts_with(&other.segments)
    }
}

impl std::fmt::Display for QualifiedName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.join("::"))
    }
}
```

### SymbolKind

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SymbolKind {
    // Containers
    Module,
    Class,
    Interface,
    Trait,
    Namespace,
    Package,

    // Callables
    Function,
    Method,
    Constructor,
    Closure,
    Macro,

    // Data structures
    Struct,
    Enum,
    EnumVariant,
    Union,
    TypeAlias,

    // Members
    Field,
    Property,
    Constant,
    Variable,
    Parameter,

    // Types
    TypeParameter,

    // Special
    Impl,
    Endpoint,
    Test,
}

impl SymbolKind {
    pub fn is_container(&self) -> bool {
        matches!(self,
            Self::Module | Self::Class | Self::Interface |
            Self::Trait | Self::Namespace | Self::Package
        )
    }

    pub fn is_callable(&self) -> bool {
        matches!(self,
            Self::Function | Self::Method | Self::Constructor |
            Self::Closure | Self::Macro
        )
    }

    pub fn is_type(&self) -> bool {
        matches!(self,
            Self::Struct | Self::Enum | Self::Union |
            Self::TypeAlias | Self::Interface | Self::Trait
        )
    }
}
```

### Signature & Visibility

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Signature {
    pub parameters: Vec<Parameter>,
    pub return_type: Option<String>,
    pub type_parameters: Vec<String>,
    pub raw: String,  // original text
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Parameter {
    pub name: String,
    pub type_annotation: Option<String>,
    pub default_value: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    Public,
    Private,
    Protected,
    Internal,    // pub(crate) in Rust
    Inherited,   // default, inherit from parent
}

impl Default for Visibility {
    fn default() -> Self {
        Self::Inherited
    }
}
```

---

## Reference Types

### Reference

```rust
// mimir-types/src/reference.rs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reference {
    pub from_symbol: SymbolId,
    pub to_symbol: SymbolId,
    pub kind: ReferenceKind,
    pub location: Location,
    pub span: Span,
    pub confidence: Confidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceKind {
    // Code references
    Call,           // function/method call
    Import,         // import statement
    TypeUse,        // type annotation
    Instantiation,  // new Foo() or Foo::new()
    FieldAccess,    // foo.bar
    Inheritance,    // extends, implements
    Implementation, // impl Trait for Struct

    // Data references
    Read,           // variable read
    Write,          // variable write
    Borrow,         // &foo
    MutableBorrow,  // &mut foo
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Confidence {
    Certain,    // 100% sure (e.g., same file, unique name)
    High,       // Very likely (e.g., import resolved)
    Medium,     // Probably correct (e.g., name match with context)
    Low,        // Guess (e.g., name match only)
}

impl Confidence {
    pub fn as_f32(&self) -> f32 {
        match self {
            Self::Certain => 1.0,
            Self::High => 0.9,
            Self::Medium => 0.7,
            Self::Low => 0.5,
        }
    }
}
```

---

## Bridge Types (Cross-Language)

```rust
// mimir-parse-core/src/bridge.rs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Bridge {
    HttpEndpoint {
        method: HttpMethod,
        path: String,
        handler_name: String,
        handler_span: Span,
    },
    HttpCall {
        method: HttpMethod,
        url_pattern: String,
        call_span: Span,
    },
    GrpcService {
        package: String,
        service: String,
        methods: Vec<String>,
    },
    FfiExport {
        name: String,
        export_name: String,
        span: Span,
    },
    FfiImport {
        import_name: String,
        span: Span,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
    Options,
    Any,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeMatch {
    pub source_bridge: Bridge,
    pub target_bridge: Bridge,
    pub confidence: f32,
    pub evidence: Vec<Evidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Evidence {
    ExactPathMatch,
    PatternMatch { pattern: String },
    FrameworkConvention,
    UserAnnotation,
    ConfigFile { path: PathBuf },
}
```

---

## Error Types

```rust
// mimir-error/src/errors.rs

use thiserror::Error;

/// Top-level error type for Mímir
#[derive(Error, Debug)]
pub enum MimirError {
    #[error("Storage error: {0}")]
    Storage(#[from] StorageError),

    #[error("Parse error: {0}")]
    Parse(#[from] ParseError),

    #[error("Graph error: {0}")]
    Graph(#[from] GraphError),

    #[error("Edit error: {0}")]
    Edit(#[from] EditError),

    #[error("Task error: {0}")]
    Task(#[from] TaskError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Internal error: {0}")]
    Internal(String),
}

#[derive(Error, Debug)]
pub enum StorageError {
    #[error("Content not found: {hash:?}")]
    ContentNotFound { hash: ContentHash },

    #[error("Failed to read file: {path}")]
    ReadFailed { path: PathBuf, source: std::io::Error },

    #[error("Failed to write file: {path}")]
    WriteFailed { path: PathBuf, source: std::io::Error },

    #[error("Compression error: {0}")]
    Compression(String),

    #[error("Serialization error: {0}")]
    Serialization(String),
}

#[derive(Error, Debug)]
pub enum ParseError {
    #[error("Failed to parse file: {path}")]
    ParseFailed { path: PathBuf, details: String },

    #[error("Unsupported language: {language}")]
    UnsupportedLanguage { language: String },

    #[error("Invalid syntax at {location}")]
    InvalidSyntax { location: String, message: String },
}

#[derive(Error, Debug)]
pub enum GraphError {
    #[error("Symbol not found: {name}")]
    SymbolNotFound { name: String },

    #[error("Ambiguous symbol: {name} has {count} matches")]
    AmbiguousSymbol { name: String, count: usize },

    #[error("File not found: {path}")]
    FileNotFound { path: PathBuf },

    #[error("Cycle detected in graph")]
    CycleDetected,
}

#[derive(Error, Debug)]
pub enum EditError {
    #[error("File changed externally: {path}")]
    ConflictDetected {
        path: PathBuf,
        expected: ContentHash,
        actual: ContentHash,
    },

    #[error("Invalid edit range: {start}..{end} in file with {total} lines")]
    InvalidRange { start: u32, end: u32, total: u32 },

    #[error("Symbol not found for edit: {name}")]
    SymbolNotFound { name: String },

    #[error("Edit would create invalid syntax")]
    InvalidSyntax { details: String },

    #[error("Transaction failed: {0}")]
    TransactionFailed(String),
}

#[derive(Error, Debug)]
pub enum TaskError {
    #[error("Task not found: {id}")]
    NotFound { id: String },

    #[error("Invalid task state transition: {from} -> {to}")]
    InvalidStateTransition { from: String, to: String },

    #[error("Checkpoint not found: {id}")]
    CheckpointNotFound { id: String },

    #[error("Recovery failed: {0}")]
    RecoveryFailed(String),

    #[error("Context limit exceeded")]
    ContextLimitExceeded,
}
```

---

## Layer 1: Storage Traits

### ContentStore

```rust
// mimir-content/src/traits.rs

use async_trait::async_trait;

/// Content-addressed storage
///
/// Stores arbitrary bytes indexed by their hash.
/// Same content = same hash = automatic deduplication.
#[async_trait]
pub trait ContentStore: Send + Sync {
    /// Store content, returning its hash
    ///
    /// If content already exists, returns existing hash without re-storing.
    async fn store(&self, content: &[u8]) -> Result<ContentHash, StorageError>;

    /// Retrieve content by hash
    async fn get(&self, hash: &ContentHash) -> Result<Option<Vec<u8>>, StorageError>;

    /// Check if content exists
    async fn exists(&self, hash: &ContentHash) -> Result<bool, StorageError>;

    /// Delete content (for garbage collection)
    async fn delete(&self, hash: &ContentHash) -> Result<bool, StorageError>;

    /// List all stored hashes (for GC)
    async fn list_hashes(&self) -> Result<Vec<ContentHash>, StorageError>;
}
```

### ContentStoreExt

```rust
/// Extension methods for ContentStore
#[async_trait]
pub trait ContentStoreExt: ContentStore {
    async fn store_string(&self, content: &str) -> Result<ContentHash, StorageError> {
        self.store(content.as_bytes()).await
    }

    async fn get_string(&self, hash: &ContentHash) -> Result<Option<String>, StorageError> {
        match self.get(hash).await? {
            Some(bytes) => Ok(Some(String::from_utf8_lossy(&bytes).into_owned())),
            None => Ok(None),
        }
    }
}

impl<T: ContentStore> ContentStoreExt for T {}
```

### Cache

```rust
// mimir-cache/src/traits.rs

pub trait Cache<K, V>: Send + Sync
where
    K: Hash + Eq + Send + Sync,
    V: Clone + Send + Sync,
{
    fn get(&self, key: &K) -> Option<V>;
    fn insert(&self, key: K, value: V);
    fn remove(&self, key: &K) -> Option<V>;
    fn contains(&self, key: &K) -> bool;
    fn clear(&self);
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool { self.len() == 0 }
    fn stats(&self) -> CacheStats;
}

#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub size_bytes: u64,
}

impl CacheStats {
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 { 0.0 } else { self.hits as f64 / total as f64 }
    }
}
```

---

## Layer 2: Language Adapter Trait

The core contract for language support:

```rust
// mimir-parse-core/src/traits.rs

use async_trait::async_trait;

/// The contract that all language adapters must implement
///
/// This is THE interface between the graph layer and language-specific code.
/// Adding a new language = implementing this trait.
#[async_trait]
pub trait LanguageAdapter: Send + Sync + 'static {
    // ═══════════════════════════════════════════════════════════════════
    // METADATA
    // ═══════════════════════════════════════════════════════════════════

    /// Unique identifier for this language
    fn language(&self) -> Language;

    /// File extensions this adapter handles
    fn file_extensions(&self) -> &'static [&'static str];

    /// Additional file patterns (e.g., "Cargo.toml", "package.json")
    fn file_patterns(&self) -> &'static [&'static str] {
        &[]
    }

    /// Check if this adapter can handle a file
    fn can_handle(&self, path: &Path) -> bool {
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if self.file_extensions().contains(&ext) {
                return true;
            }
        }
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            for pattern in self.file_patterns() {
                if pattern.ends_with(name) {
                    return true;
                }
            }
        }
        false
    }

    // ═══════════════════════════════════════════════════════════════════
    // PARSING
    // ═══════════════════════════════════════════════════════════════════

    /// Parse source code into an AST
    fn parse(&self, source: &str) -> Result<ParsedTree, ParseError>;

    /// Parse with incremental update (optional optimization)
    fn parse_incremental(
        &self,
        source: &str,
        old_tree: &ParsedTree,
        edit: &TreeEdit,
    ) -> Result<ParsedTree, ParseError> {
        self.parse(source) // default: full reparse
    }

    // ═══════════════════════════════════════════════════════════════════
    // EXTRACTION
    // ═══════════════════════════════════════════════════════════════════

    /// Extract all symbols from parsed tree
    fn extract_symbols(
        &self,
        tree: &ParsedTree,
        source: &str,
        file_id: FileId,
    ) -> Vec<ExtractedSymbol>;

    /// Extract all references from parsed tree
    fn extract_references(
        &self,
        tree: &ParsedTree,
        source: &str,
        file_id: FileId,
    ) -> Vec<ExtractedReference>;

    /// Detect cross-language bridges (optional)
    fn detect_bridges(
        &self,
        tree: &ParsedTree,
        source: &str,
        file_id: FileId,
    ) -> Vec<Bridge> {
        vec![]
    }

    // ═══════════════════════════════════════════════════════════════════
    // RESOLUTION (optional, for better accuracy)
    // ═══════════════════════════════════════════════════════════════════

    /// Resolve an import path to a qualified name
    fn resolve_import(
        &self,
        _import_path: &str,
        _from_file: &Path,
        _project_root: &Path,
    ) -> Option<String> {
        None
    }

    /// Get the scope at a given location
    fn get_scope_at(
        &self,
        _tree: &ParsedTree,
        _source: &str,
        _location: Location,
    ) -> ScopeInfo {
        ScopeInfo::default()
    }

    // ═══════════════════════════════════════════════════════════════════
    // FORMATTING (optional, for better display)
    // ═══════════════════════════════════════════════════════════════════

    /// Format a symbol signature for display
    fn format_signature(&self, symbol: &ExtractedSymbol, source: &str) -> String {
        symbol.text.clone()
    }

    /// Extract documentation comment for a symbol
    fn extract_doc(
        &self,
        _tree: &ParsedTree,
        _source: &str,
        _symbol_span: Span,
    ) -> Option<String> {
        None
    }
}
```

### Supporting Types

```rust
/// Wrapper around tree-sitter's parsed tree
pub struct ParsedTree {
    pub(crate) inner: tree_sitter::Tree,
    pub(crate) has_errors: bool,
}

impl ParsedTree {
    pub fn root_node(&self) -> tree_sitter::Node<'_> {
        self.inner.root_node()
    }

    pub fn has_errors(&self) -> bool {
        self.has_errors
    }
}

/// Information for incremental parsing
pub struct TreeEdit {
    pub start_byte: usize,
    pub old_end_byte: usize,
    pub new_end_byte: usize,
    pub start_position: tree_sitter::Point,
    pub old_end_position: tree_sitter::Point,
    pub new_end_position: tree_sitter::Point,
}

/// Symbol as extracted by adapter (before resolution)
#[derive(Debug, Clone)]
pub struct ExtractedSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub span: Span,
    pub text: String,
    pub parent_name: Option<String>,
    pub visibility: Visibility,
    pub signature_text: Option<String>,
    pub doc_comment: Option<String>,
    pub attributes: Vec<String>,
}

/// Reference as extracted by adapter (before resolution)
#[derive(Debug, Clone)]
pub struct ExtractedReference {
    pub from_symbol_name: String,
    pub to_name: String,
    pub kind: ReferenceKind,
    pub span: Span,
    pub context_text: String,
}

/// Scope information at a location
#[derive(Debug, Clone, Default)]
pub struct ScopeInfo {
    pub module_path: Option<String>,
    pub imports: Vec<String>,
    pub local_symbols: Vec<String>,
}
```

### Adapter Registry

```rust
// mimir-parse-core/src/registry.rs

pub struct AdapterRegistry {
    adapters: DashMap<Language, Arc<dyn LanguageAdapter>>,
    extension_map: DashMap<String, Language>,
}

impl AdapterRegistry {
    pub fn new() -> Self {
        Self {
            adapters: DashMap::new(),
            extension_map: DashMap::new(),
        }
    }

    /// Register a language adapter
    pub fn register<A: LanguageAdapter>(&self, adapter: A) {
        let lang = adapter.language();
        for ext in adapter.file_extensions() {
            self.extension_map.insert(ext.to_string(), lang);
        }
        self.adapters.insert(lang, Arc::new(adapter));
    }

    /// Get adapter for a file path
    pub fn adapter_for(&self, path: &Path) -> Option<Arc<dyn LanguageAdapter>> {
        let ext = path.extension()?.to_str()?;
        let lang = self.extension_map.get(ext)?;
        self.adapters.get(&*lang).map(|a| a.clone())
    }

    /// Get adapter by language
    pub fn get(&self, language: Language) -> Option<Arc<dyn LanguageAdapter>> {
        self.adapters.get(&language).map(|a| a.clone())
    }
}
```

---

## Handle Pattern

Every module exposes a **Handle** - cheap, cloneable, thread-safe:

```rust
pub struct SymbolsHandle {
    // Lock-free read access
    data: Arc<DashMap<SymbolId, Arc<Symbol>>>,
    name_index: Arc<DashMap<String, Vec<SymbolId>>>,

    // Write channel
    write_tx: mpsc::Sender<SymbolCommand>,

    // Event broadcast
    events: broadcast::Sender<SymbolEvent>,
}

impl SymbolsHandle {
    // READ: Direct, lock-free
    pub fn get(&self, id: SymbolId) -> Option<Arc<Symbol>> {
        self.data.get(&id).map(|r| r.clone())
    }

    pub fn find_by_name(&self, name: &str) -> Vec<Arc<Symbol>> {
        self.name_index.get(name)
            .map(|ids| ids.iter().filter_map(|id| self.get(*id)).collect())
            .unwrap_or_default()
    }

    // WRITE: Through channel, async
    pub async fn insert(&self, symbol: Symbol) -> Result<SymbolId> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.write_tx.send(SymbolCommand::Insert {
            symbol,
            reply: reply_tx
        }).await?;
        reply_rx.await?
    }

    // SUBSCRIBE: Get event stream
    pub fn subscribe(&self) -> broadcast::Receiver<SymbolEvent> {
        self.events.subscribe()
    }
}

impl Clone for SymbolsHandle {
    fn clone(&self) -> Self {
        Self {
            data: self.data.clone(),
            name_index: self.name_index.clone(),
            write_tx: self.write_tx.clone(),
            events: self.events.clone(),
        }
    }
}

// Handle is Send + Sync
unsafe impl Send for SymbolsHandle {}
unsafe impl Sync for SymbolsHandle {}
```

### Module Creation Pattern

```rust
pub fn create_symbols_module() -> SymbolsHandle {
    let data = Arc::new(DashMap::new());
    let name_index = Arc::new(DashMap::new());
    let (write_tx, write_rx) = mpsc::channel(1000);
    let (events, _) = broadcast::channel(1000);

    // Spawn coordinator task
    let coordinator = SymbolsCoordinator {
        data: data.clone(),
        name_index: name_index.clone(),
        commands: write_rx,
        events: events.clone(),
    };
    tokio::spawn(coordinator.run());

    // Return handle
    SymbolsHandle {
        data,
        name_index,
        write_tx,
        events,
    }
}
```

---

## Edit Operations

```rust
// mimir-edit/src/operations.rs

#[derive(Clone, Serialize, Deserialize)]
pub enum EditOp {
    /// Replace lines (most reliable)
    ReplaceLines {
        file: PathBuf,
        start_line: u32,
        end_line: u32,
        content: String,
        expected_hash: Option<ContentHash>,
    },

    /// Insert at position
    InsertAt {
        file: PathBuf,
        line: u32,
        position: InsertPosition,
        content: String,
    },

    /// Delete lines
    DeleteLines {
        file: PathBuf,
        start_line: u32,
        end_line: u32,
    },

    /// Replace by byte range (precise)
    ReplaceBytes {
        file: PathBuf,
        start: u32,
        end: u32,
        content: String,
        expected_hash: Option<ContentHash>,
    },

    /// AST-aware: replace node
    ReplaceNode {
        file: PathBuf,
        node_kind: SymbolKind,
        node_name: String,
        new_content: String,
    },

    /// Rename symbol (graph-powered)
    RenameSymbol {
        symbol_id: SymbolId,
        new_name: String,
        scope: RenameScope,
    },

    /// Atomic multi-edit
    Transaction {
        edits: Vec<EditOp>,
        on_conflict: ConflictStrategy,
    },

    /// Create new file
    CreateFile {
        path: PathBuf,
        content: String,
    },

    /// Delete file
    DeleteFile {
        path: PathBuf,
    },
}

#[derive(Clone, Copy, Serialize, Deserialize)]
pub enum InsertPosition {
    Before,
    After,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
pub enum RenameScope {
    File,
    Module,
    Workspace,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
pub enum ConflictStrategy {
    Fail,   // abort on any conflict
    Force,  // overwrite
    Merge,  // try 3-way merge
    Skip,   // skip conflicting files
}

#[derive(Clone, Serialize, Deserialize)]
pub struct EditResult {
    pub success: bool,
    pub files_modified: Vec<PathBuf>,
    pub warnings: Vec<EditWarning>,
    pub new_snapshot: Option<SnapshotId>,
}
```

---

## Protocol Types

### Commands

```rust
// mimir-protocol/src/commands.rs

pub enum GraphCommand {
    // Queries
    Trace { name: String, options: TraceOptions, reply: oneshot::Sender<TraceResult> },
    FindSymbol { name: String, reply: oneshot::Sender<Vec<Symbol>> },
    FindReferences { symbol_id: SymbolId, reply: oneshot::Sender<Vec<Reference>> },

    // Mutations
    Sync { force: bool, reply: oneshot::Sender<SyncStats> },
    Edit { operation: EditOp, reply: oneshot::Sender<EditResult> },

    // Control
    Shutdown,
}

pub enum TaskCommand {
    Create { request: String, reply: oneshot::Sender<TaskId> },
    Start { task_id: TaskId, reply: oneshot::Sender<()> },
    Pause { task_id: TaskId, reply: oneshot::Sender<()> },
    Resume { task_id: TaskId, reply: oneshot::Sender<()> },
    Cancel { task_id: TaskId, reply: oneshot::Sender<()> },
    GetStatus { task_id: TaskId, reply: oneshot::Sender<TaskStatus> },
}
```

### TraceOptions & TraceResult

```rust
// mimir-protocol/src/responses.rs

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct TraceOptions {
    pub max_depth: u32,
    pub include_callers: bool,
    pub include_callees: bool,
    pub include_similar: bool,
    pub cross_language: bool,
    pub context_lines: u32,
    pub similarity_threshold: f32,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct TraceResult {
    pub symbol: Symbol,
    pub definition: CodeContext,
    pub callers: Vec<CallSite>,
    pub callees: Vec<CallSite>,
    pub similar: Vec<SimilarSymbol>,
    pub cross_language: Vec<CrossLanguageRef>,
    pub edit_targets: Vec<EditTarget>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct CodeContext {
    pub file_path: PathBuf,
    pub start_line: u32,
    pub end_line: u32,
    pub content: String,
    pub language: Language,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct CallSite {
    pub symbol: Symbol,
    pub location: Location,
    pub context: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SimilarSymbol {
    pub symbol: Symbol,
    pub similarity: f32,
    pub reason: String,
}
```

---

## Next: See TASKS.md for task management system
