# Mímir CLI Usage Examples

This document shows how to use Mímir from the command line.

---

## Basic Commands

### Initialize Project

```bash
$ cd my-rust-project
$ mimir init

✓ Indexed 342 files
✓ Found 1,847 symbols
✓ Detected languages: Rust, TypeScript
✓ Ready
```

### Check Status

```bash
$ mimir status

Project: my-rust-project
Root: /home/user/projects/my-rust-project
State: .agent/

Files:
  Indexed: 342
  Languages: Rust (280), TypeScript (62)

Graph:
  Symbols: 1,847
  References: 4,293
  Bridges: 12 (HTTP endpoints)

Tasks:
  Active: 0
  Completed: 3

Last sync: 2 minutes ago
```

---

## Daemon Commands

The daemon runs as a background process that keeps the graph in sync with file changes automatically.

### Start Daemon

```bash
# Start in background
$ mimir daemon start
Daemon started (PID: 12345)

# Start in foreground (for debugging)
$ mimir daemon start --foreground
Starting daemon in foreground mode...
INFO Starting Mimir daemon for: /home/user/projects/my-project
INFO Graph actor starting for root: /home/user/projects/my-project
INFO File watcher started for: /home/user/projects/my-project
INFO Performing initial sync...
INFO Scanned 342 files in 45ms
INFO Sync completed: 342 added, 0 updated, 0 removed in 120ms
INFO Initial sync complete: 342 files, 1847 symbols
INFO Daemon listening on: .agent/daemon.sock
```

### Check Daemon Status

```bash
$ mimir daemon status

Daemon running (PID: 12345)
Uptime: 2h 15m
Socket: .agent/daemon.sock
Files indexed: 342
Symbols: 1847
```

### Stop Daemon

```bash
$ mimir daemon stop
Daemon stopped
```

### Auto-Detection

When the daemon is running, CLI commands automatically use it:

```bash
$ mimir trace -f src/main.rs -s process_data

Trace for: process_data (function) (via daemon)
  File: src/main.rs:42

Callers (3):
  ← handle_request (function)
  ← batch_process (function)
  ← test_process (function)

Total: 4 symbols, max depth: 1
```

The `(via daemon)` indicator shows the query used the daemon's live graph.

---

## Tracing Symbols

### Basic Trace

```bash
$ mimir trace -s process_user

Trace for: process_user (Function)
  Lines: 42-67

Callers (2):
  ← handle_request (Function)
  ← batch_process (Function)

Callees (3):
  → validate_user (Function)
  → save_to_db (Function)
  → log_action (Function)

Total: 6 symbols, max depth: 3
```

### Trace with Source Code (`--code`)

The `--code` flag shows the source code of the traced symbol:

```bash
$ mimir trace -s handle_add_file --code

Trace for: handle_add_file (Method)
  Lines: 132-233
  (via daemon)

```
 132│     fn handle_add_file(
 133│         &mut self,
 134│         path: PathBuf,
 135│         content: String,
 136│         language: Language,
 137│         events: &EventSender,
 138│     ) -> Result<FileId, GraphError> {
 139│         debug!("Adding file: {}", path.display());
 140│         // ... rest of the function
 233│     }
```

Callers (1):
  ← run (Method)

Callees (25):
  → add_symbol (Method)
  → extract_symbols (Method)
  → parse (Method)
  ...

Total: 27 symbols, max depth: 3
```

### Single File Trace

Trace a symbol from a specific file without using the project graph:

```bash
$ mimir trace -f src/handlers/user.rs -s process_user --code

Trace for: process_user (Function)
  File: src/handlers/user.rs:42-67

```
  42│ pub async fn process_user(req: UserRequest) -> Result<User> {
  43│     let validated = validate_user(&req)?;
  44│     let user = save_to_db(validated).await?;
  45│     Ok(user)
  46│ }
```

Callees (2):
  → validate_user (Function)
  → save_to_db (Function)

Total: 3 symbols, max depth: 3
```

### JSON Output

```bash
$ mimir trace -s process_user --json | jq '.root'

{
  "id": 42,
  "name": "process_user",
  "qualified_name": "handlers::user::process_user",
  "kind": "function",
  "file_id": 5,
  "file_path": "src/handlers/user.rs",
  "start_line": 42,
  "end_line": 67,
  "language": "rust"
}
```

### Custom Depth

```bash
$ mimir trace -s main -d 5  # Trace up to 5 levels deep
```

### Filter by Direction

Show only callers (who calls this symbol):

```bash
$ mimir trace -s process_user --callers

Trace for: process_user (Function)
  Lines: 42-67

Callers (3):
  ← handle_request (Function)
  ← batch_process (Function)
    ← main (Function)
  ← test_process (Function)

Total: 4 symbols, max depth: 3
```

Show only callees (what this symbol calls):

```bash
$ mimir trace -s process_user --callees

Trace for: process_user (Function)
  Lines: 42-67

Callees (3):
  → validate_user (Function)
  → save_to_db (Function)
    → execute_query (Method)
  → log_action (Function)

Total: 4 symbols, max depth: 3
```

---

## Editing Files

The `mimir edit` command provides precise file editing with conflict detection and automatic graph sync.

### Edit Modes

- **lines**: Replace line ranges (most common, recommended)
- **symbol**: Replace entire symbol with new content
- **replace**: Replace byte ranges
- **insert**: Insert at byte position
- **delete**: Delete byte range

### Line-Based Editing (Recommended)

Edit by line numbers - the most reliable approach:

```bash
# Preview changes first
$ mimir edit -f src/lib.rs -m lines -s 42 -e 42 -c '    let result = compute_value();' --preview

Preview of changes to: /path/to/src/lib.rs
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -42,1 +42,1 @@
-    let result = old_function();
+    let result = compute_value();

# Apply the edit
$ mimir edit -f src/lib.rs -m lines -s 42 -e 42 -c '    let result = compute_value();'

Edited: /path/to/src/lib.rs
  Bytes: a1b2c3d4 → e5f6g7h8 (+5)
  Lines: 1 modified (net: +0)
  Graph synced: 1403 symbols
```

### Multi-Line Replacement

Replace multiple lines at once:

```bash
$ mimir edit -f src/config.rs -m lines -s 10 -e 15 -c 'const MAX_CONNECTIONS: u32 = 100;
const TIMEOUT_MS: u32 = 5000;
const RETRY_COUNT: u32 = 3;'

Edited: /path/to/src/config.rs
  Bytes: ... → ... (-42)
  Lines: 3 modified (net: -3)
  Graph synced: 1403 symbols
```

### Insert New Content

Insert at a specific byte position:

```bash
$ mimir edit -f src/lib.rs -m insert -s 256 -c '// New comment\n'
```

### Delete Content

Delete a byte range:

```bash
$ mimir edit -f src/lib.rs -m delete -s 100 -e 150
```

### Symbol-Based Editing

Edit code by symbol name instead of file path and line numbers:

```bash
# Edit a specific line within a symbol (relative line numbers)
$ mimir edit --symbol process_user -m lines -s 2 -e 2 -c '    let validated = new_validate(&req)?;'

Resolved symbol 'process_user' to src/handlers/user.rs:42-67
Edited: src/handlers/user.rs
  Lines: 1 modified (net: +0)
  Graph synced: 1403 symbols
```

When using `--symbol`:
- Line numbers are relative to the symbol's start (line 1 = first line of the function)
- File path is resolved automatically
- Works with all edit modes

### Replace Entire Symbol

Replace a complete function/struct with new code:

```bash
$ mimir edit --symbol validate_user -m symbol -c 'pub fn validate_user(user: &User) -> Result<()> {
    // New improved validation
    if user.name.is_empty() || user.email.is_empty() {
        return Err(ValidationError::MissingFields);
    }
    Ok(())
}' --preview

Resolved symbol 'validate_user' to src/validation.rs:10-25
Preview of changes to: src/validation.rs
--- a/src/validation.rs
+++ b/src/validation.rs
@@ -10,16 +10,7 @@
-pub fn validate_user(user: &User) -> Result<()> {
-    // Old validation logic
-    ...
-}
+pub fn validate_user(user: &User) -> Result<()> {
+    // New improved validation
+    if user.name.is_empty() || user.email.is_empty() {
+        return Err(ValidationError::MissingFields);
+    }
+    Ok(())
+}
```

### Batch Edits

Apply multiple edits from JSON:

```bash
$ mimir edit-batch -j '[
  {"file_path": "src/a.rs", "kind": {"ReplaceLines": {"start_line": 10, "end_line": 12, "new_content": "new code"}}},
  {"file_path": "src/b.rs", "kind": {"ReplaceLines": {"start_line": 5, "end_line": 5, "new_content": "updated line"}}}
]'

Applied 2 edits:
  src/a.rs (+10 bytes, -2 lines)
  src/b.rs (+3 bytes, +0 lines)
```

### Batch Symbol Edits

Edit multiple symbols at once:

```bash
$ mimir edit-symbols -j '[
  {"symbol": "validate_user", "content": "pub fn validate_user(u: &User) -> Result<()> { Ok(()) }"},
  {"symbol": "validate_email", "content": "pub fn validate_email(e: &str) -> bool { !e.is_empty() }"}
]' --preview

  validate_user → src/validation.rs:10-25
  validate_email → src/validation.rs:30-45

Previewing 2 symbol edits:

--- src/validation.rs ---
...
```

### Workflow: Search → Read → Edit

The recommended workflow using symbol-based commands:

```bash
# 1. Search for symbols
$ mimir search -p "process.*user"

Found 2 symbols matching 'process.*user':
  1. process_user [function] - src/handlers/user.rs:42
  2. process_user_batch [function] - src/handlers/batch.rs:89

# 2. Read the symbol's code
$ mimir read -s process_user

process_user  [function]  src/handlers/user.rs:42-67

  42│ pub fn process_user(req: Request) -> Result<User> {
  43│     let old_value = compute();
  44│     save_to_db(old_value).await?;
  45│     Ok(user)
  46│ }

# 3. Edit by symbol name (relative line 2 = absolute line 43)
$ mimir edit --symbol process_user -m lines -s 2 -e 2 -c '    let new_value = improved_compute();'

Resolved symbol 'process_user' to src/handlers/user.rs:42-67
Edited: src/handlers/user.rs
  Lines: 1 modified (net: +0)
  Graph synced: 1403 symbols

# 4. Or replace entire symbol
$ mimir edit --symbol process_user -m symbol -c 'pub fn process_user(req: Request) -> Result<User> {
    let value = improved_compute();
    save_to_db(value).await?;
    Ok(user)
}'

# 5. Verify the change
$ mimir read -s process_user
```

---

## Symbol Search

Search for symbols by name pattern (supports regex):

```bash
$ mimir search -p "handle.*file"

Found 3 symbols matching 'handle.*file':

  1. handle_add_file (<GraphActor>::handle_add_file) [method] - crates/mimir-graph-actor/src/actor.rs:132
  2. handle_remove_file (<GraphActor>::handle_remove_file) [method] - crates/mimir-graph-actor/src/actor.rs:245
  3. handle_update_file (<GraphActor>::handle_update_file) [method] - crates/mimir-graph-actor/src/actor.rs:180

Total: 3 matches
```

### Filter by Kind

```bash
$ mimir search -p "User" -k struct

Found 5 symbols matching 'User':

  1. User [struct] - src/models/user.rs:12
  2. UserRequest [struct] - src/api/requests.rs:45
  3. UserResponse [struct] - src/api/responses.rs:23
  ...
```

### Show Code Snippets

```bash
$ mimir search -p "validate" --code

Found 2 symbols matching 'validate':

  1. validate_user [function] - src/validation.rs:10
     ```
       10│ pub fn validate_user(user: &User) -> Result<()> {
       11│     if user.name.is_empty() {
       12│         return Err(ValidationError::EmptyName);
       13│     }
       14│     Ok(())
       15│ }
     ```
  ...
```

### JSON Output

```bash
$ mimir search -p "handle" --json | jq '.symbols[0]'

{
  "id": 42,
  "name": "handle_request",
  "qualified_name": "handlers::api::handle_request",
  "kind": "Function",
  "file_path": "src/handlers/api.rs",
  "start_line": 45,
  "end_line": 89
}
```

---

## Reading Symbols

Read source code by symbol name:

```bash
$ mimir read -s process_user

process_user  [function]  src/handlers/user.rs:42-67

  42│ pub async fn process_user(req: UserRequest) -> Result<User> {
  43│     let validated = validate_user(&req)?;
  44│     let user = save_to_db(validated).await?;
  45│     log_action("user_processed", &user.id);
  46│     Ok(user)
  47│ }
```

### With Context Lines

Show lines before (`-B`) and after (`-A`) the symbol:

```bash
$ mimir read -s process_user -B 3 -A 2

process_user  [function]  src/handlers/user.rs:42-67

·  39│ }
·  40│
·  41│ /// Process a user request.
   42│ pub async fn process_user(req: UserRequest) -> Result<User> {
   43│     let validated = validate_user(&req)?;
   44│     let user = save_to_db(validated).await?;
   45│     log_action("user_processed", &user.id);
   46│     Ok(user)
   47│ }
·  48│
·  49│ /// Delete a user.
```

Context lines are marked with `·` to distinguish them from the symbol's code.

### JSON Output

```bash
$ mimir read -s process_user --json

{
  "symbol": "process_user",
  "kind": "Function",
  "file": "src/handlers/user.rs",
  "start_line": 42,
  "end_line": 67,
  "content": "pub async fn process_user(...) -> Result<User> {\n    ...\n}",
  "lines": [
    {"number": 42, "content": "pub async fn process_user...", "is_symbol": true},
    ...
  ]
}
```

---

## Task-Based Workflow

### Starting a Task

```bash
$ mimir task "Split the auth module into separate crates for JWT, OAuth, and sessions"

╭──────────────────────────────────────────────────────────────────────────╮
│ Task: refactor-auth-module (ID: task_a1b2c3)                             │
├──────────────────────────────────────────────────────────────────────────┤
│ Status: PLANNING                                                         │
│                                                                          │
│ Analyzing current auth module structure...                               │
│                                                                          │
│ Found:                                                                   │
│   • src/auth/mod.rs (450 lines)                                          │
│   • src/auth/jwt.rs (280 lines)                                          │
│   • src/auth/oauth.rs (520 lines)                                        │
│   • src/auth/session.rs (190 lines)                                      │
│   • 23 files depend on auth module                                       │
│                                                                          │
│ Proposed plan:                                                           │
│   1. Create crates/jwt with JWT functionality                            │
│   2. Create crates/oauth with OAuth functionality                        │
│   3. Create crates/sessions with session management                      │
│   4. Update auth module to re-export from new crates                     │
│   5. Update 23 dependent files                                           │
│   6. Run tests to verify                                                 │
│                                                                          │
│ Risk: MEDIUM (many file changes)                                         │
│                                                                          │
│ Proceed? [Y/n/modify]                                                    │
╰──────────────────────────────────────────────────────────────────────────╯
```

### Task Execution

```bash
$ y

╭──────────────────────────────────────────────────────────────────────────╮
│ [Step 1/6] Creating crates/jwt...                                        │
│                                                                          │
│ ✓ Created crates/jwt/Cargo.toml                                          │
│ ✓ Created crates/jwt/src/lib.rs                                          │
│ ✓ Moving JWT types and functions...                                      │
│   • JwtToken → crates/jwt/src/token.rs                                   │
│   • JwtClaims → crates/jwt/src/claims.rs                                 │
│   • encode_jwt → crates/jwt/src/encode.rs                                │
│   • decode_jwt → crates/jwt/src/decode.rs                                │
│ ✓ Checkpoint saved                                                       │
│                                                                          │
│ Progress: ████░░░░░░░░░░░░░░░░ 17%                                       │
╰──────────────────────────────────────────────────────────────────────────╯
```

### Interrupt & Resume

```bash
# User presses Ctrl+C during execution
^C
⏸ Interrupt received, saving state...
✓ Checkpoint saved at step 3/6
✓ State saved. Resume with: mimir resume

# Later...
$ mimir status

╭──────────────────────────────────────────────────────────────────────────╮
│ Active Tasks                                                             │
├──────────────────────────────────────────────────────────────────────────┤
│ 1. refactor-auth-module (task_a1b2c3)                                    │
│    Status: PAUSED at step 3/6                                            │
│    Last activity: 2 hours ago                                            │
│    Files modified: 8                                                     │
│    Files pending: 15                                                     │
│    Checkpoint: cp_x7y8z9                                                 │
╰──────────────────────────────────────────────────────────────────────────╯

$ mimir resume

╭──────────────────────────────────────────────────────────────────────────╮
│ Resuming: refactor-auth-module                                           │
├──────────────────────────────────────────────────────────────────────────┤
│ Restoring context from checkpoint cp_x7y8z9...                           │
│                                                                          │
│ Previously completed:                                                    │
│   ✓ Step 1: Created crates/jwt                                           │
│   ✓ Step 2: Created crates/oauth                                         │
│   → Step 3: Creating crates/sessions (in progress)                       │
│                                                                          │
│ Key decisions made:                                                      │
│   • JWT crate uses jsonwebtoken v9.2                                     │
│   • OAuth crate depends on oauth2 v4.4                                   │
│   • Keeping backward-compatible re-exports in src/auth                   │
│                                                                          │
│ Continuing from step 3...                                                │
╰──────────────────────────────────────────────────────────────────────────╯

[Step 3/6] Creating crates/sessions... (continuing)
```

### Error Recovery

```bash
╭──────────────────────────────────────────────────────────────────────────╮
│ [Step 4/6] Updating dependent files...                                   │
│                                                                          │
│ ⚠ Error in src/api/middleware.rs:                                        │
│   Cannot resolve import: `use crate::auth::JwtValidator`                 │
│   JwtValidator was moved but has private dependencies                    │
│                                                                          │
│ Options:                                                                 │
│   [1] Make JwtValidator dependencies public                              │
│   [2] Keep JwtValidator in auth module                                   │
│   [3] Rollback to checkpoint and try different approach                  │
│   [4] Pause and let me investigate manually                              │
│                                                                          │
│ Choice [1/2/3/4]:                                                        │
╰──────────────────────────────────────────────────────────────────────────╯

$ 3

Rolling back to checkpoint cp_x7y8z9...
✓ Files restored: 8
✓ Graph reindexed

Let's try a different approach. Instead of moving JwtValidator entirely,
I'll create a facade in the jwt crate that delegates to the shared
validation logic. This maintains the public API while improving organization.

[Step 3/6] Creating crates/sessions (revised approach)...
```

---

## Progress Visualization

```bash
╭────────────────────────────────────────────────────────╮
│ Task: Add comprehensive test coverage to user module   │
├────────────────────────────────────────────────────────┤
│ Progress: ████████████░░░░░░░░ 60%                     │
│                                                        │
│ Completed:                                             │
│   ✓ Analyzed user module (12 functions)                │
│   ✓ Created test file structure                        │
│   ✓ Generated tests for: create_user, update_user,     │
│     delete_user, get_user, list_users                  │
│                                                        │
│ In progress:                                           │
│   → Writing tests for: validate_user (3/7 cases done)  │
│                                                        │
│ Remaining:                                             │
│   • Tests for: authenticate, refresh_token             │
│   • Edge cases and error handling                      │
│   • Integration tests                                  │
│                                                        │
│ Time elapsed: 4m 32s                                   │
│ Checkpoints: 4 (last: 30s ago)                         │
│                                                        │
│ [p]ause  [c]ancel  [d]etails  [Enter] continue         │
╰────────────────────────────────────────────────────────╯
```

---

## Plan-Only Mode

```bash
$ mimir task "add caching to database queries" --plan-only

Proposed plan for: add-db-caching
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

Analysis:
  • Found 23 database query functions
  • 18 are cacheable (pure queries)
  • 5 have side effects (skip caching)

Plan:
  1. Create cache module (crates/cache)
  2. Add cache trait and implementations
  3. Update 18 query functions:
     • get_user, get_users
     • get_product, list_products
     • get_order, list_orders
     ... (12 more)
  4. Add cache invalidation to mutations
  5. Add cache configuration
  6. Run tests

Checkpoints: After steps 1, 3, 4, 6
Risk: LOW (additive changes, no breaking changes)

[e]xecute  [m]odify plan  [s]ave for later  [c]ancel
```

---

## Recovery Commands

### Recover from Crash

```bash
$ mimir recover

Found interrupted task: refactor-auth (crashed 10 min ago)
Last checkpoint: step 4/6, 94% complete
Recovering... ✓
Continuing from where you left off.
```

### Rollback to Snapshot

```bash
$ mimir rollback

Available snapshots:
  1. snap_20240115_143022  Before: refactor-auth-module
  2. snap_20240115_120000  Before: add-user-validation
  3. snap_20240114_180000  Before: fix-login-bug

Select snapshot [1]:

Rolling back to snap_20240115_143022...
✓ Restored 15 files
✓ Graph reindexed
```

---

## Configuration

```bash
$ mimir config --show

[project]
name = "my-project"
root = "."
output_dir = ".agent"

[indexing]
include = ["**/*.rs", "**/*.ts", "**/*.py"]
exclude = ["**/node_modules/**", "**/target/**"]
max_file_size_kb = 500

[languages]
enabled = ["rust", "typescript", "python"]

[cache]
max_memory_mb = 512
embedding_cache_size = 10000

[sync]
watch_enabled = true
watch_debounce_ms = 100

$ mimir config set cache.max_memory_mb 1024
✓ Updated cache.max_memory_mb = 1024
```

---

## Sync Commands

```bash
# Manual sync
$ mimir sync

Scanning for changes...
Found: 3 modified, 1 added, 0 deleted
Parsing changed files...
Updating graph...
✓ Sync complete (423ms)

# Force full rebuild
$ mimir sync --force

Rebuilding entire index...
Scanning 342 files...
Parsing...
Building graph...
✓ Full rebuild complete (4.2s)

# Watch mode
$ mimir watch

Watching for changes...
[10:23:15] src/handlers/user.rs modified
[10:23:15] Syncing... done (89ms)
[10:24:02] src/models/user.rs modified
[10:24:02] src/handlers/user.rs modified
[10:24:02] Syncing... done (124ms)
^C Stopped watching.
```

---

## Agent Mode

```bash
$ mimir agent

╭────────────────────────────────────────────────────────╮
│ Mímir Agent v0.1.0                                     │
│ Type your request or 'help' for commands               │
╰────────────────────────────────────────────────────────╯

> Add input validation to all API endpoints

Analyzing API endpoints...
Found 15 endpoints without validation.

I'll add validation using the validator crate. Here's my plan:
1. Add validator dependency
2. Create validation schemas for each endpoint
3. Add validation middleware
4. Update endpoint handlers

Shall I proceed? [Y/n]

> y

[Step 1/4] Adding validator dependency...
✓ Updated Cargo.toml

[Step 2/4] Creating validation schemas...
...
```

---

## Help

```bash
$ mimir --help

Mímir - Semantic Code Intelligence Engine

USAGE:
    mimir <COMMAND>

COMMANDS:
    init          Initialize Mímir in current directory
    status        Show project status
    daemon        Manage background daemon (start, stop, status)
    sync          Sync with filesystem
    search        Search for symbols by pattern
    read          Read source code by symbol name
    trace         Trace a symbol (show callers, callees)
    edit          Edit files with conflict detection
    edit-batch    Apply batch edits from JSON
    edit-symbols  Apply batch symbol edits from JSON
    help          Show this help

OPTIONS:
    -v, --verbose    Verbose output
    --json           JSON output
    --version        Show version

$ mimir search --help

Search for symbols by pattern

OPTIONS:
    -p, --pattern <PATTERN>  Pattern to search for (supports regex)
    -k, --kind <KIND>        Filter by symbol kind (function, struct, enum, etc.)
    -l, --limit <LIMIT>      Maximum results to return [default: 20]
        --json               Output as JSON
    -c, --code               Show source code snippet

$ mimir read --help

Read source code by symbol name

OPTIONS:
    -s, --symbol <SYMBOL>    Symbol name to read
    -B, --before <LINES>     Lines of context before [default: 0]
    -A, --after <LINES>      Lines of context after [default: 0]
    -n, --numbers            Show line numbers [default: true]
        --json               Output as JSON

$ mimir trace --help

Trace call chains for a symbol

OPTIONS:
    -s, --symbol <SYMBOL>    Symbol name to trace
    -f, --file <FILE>        Source file (optional)
    -d, --depth <DEPTH>      Maximum trace depth [default: 3]
    -c, --code               Show source code snippet
        --callers            Show only callers
        --callees            Show only callees
        --json               Output as JSON

$ mimir edit --help

Edit a file with conflict detection

OPTIONS:
    -f, --file <FILE>        File to edit (optional if --symbol provided)
        --symbol <SYMBOL>    Symbol name to edit
    -m, --mode <MODE>        Edit mode: replace, insert, delete, lines, symbol
    -s, --start <POS>        Start position (relative to symbol if --symbol used)
    -e, --end <POS>          End position
    -c, --content <TEXT>     New content
    -p, --preview            Preview changes without applying
    -b, --backup             Create backup file

$ mimir edit-symbols --help

Apply batch symbol edits from JSON

OPTIONS:
    -j, --json <JSON>        JSON: [{"symbol": "name", "content": "code"}, ...]
    -p, --preview            Preview changes without applying
    -b, --backup             Create backup files
```
