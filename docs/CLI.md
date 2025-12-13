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
$ mimir trace process_user

┌─────────────────────────────────────────────────────────────┐
│ process_user                                                │
│ src/handlers/user.rs:42                                     │
│                                                             │
│ pub async fn process_user(req: UserRequest) -> Result<User> │
├─────────────────────────────────────────────────────────────┤
│ Called by:                                                  │
│   • handle_request     src/api.rs:89                        │
│   • batch_process      src/jobs/batch.rs:156                │
│                                                             │
│ Calls:                                                      │
│   • validate_user      src/validation.rs:23                 │
│   • save_to_db         src/db/users.rs:67                   │
├─────────────────────────────────────────────────────────────┤
│ Similar functions:                                          │
│   • process_admin (0.89)  src/handlers/admin.rs:34          │
│   • update_user (0.84)    src/handlers/user.rs:98           │
└─────────────────────────────────────────────────────────────┘
```

### Cross-Language Trace

```bash
$ mimir trace handle_login --cross-language

┌─────────────────────────────────────────────────────────────┐
│ handle_login                                                │
│ src/handlers/auth.rs:45                                     │
│ Language: Rust                                              │
├─────────────────────────────────────────────────────────────┤
│ HTTP Endpoint: POST /api/auth/login                         │
│                                                             │
│ Called from frontend:                                       │
│   • loginUser()        web/src/api/auth.ts:23               │
│     fetch('/api/auth/login', { method: 'POST', ... })       │
│                                                             │
│   • AuthForm.submit()  web/src/components/AuthForm.tsx:67   │
│     calls loginUser()                                       │
├─────────────────────────────────────────────────────────────┤
│ Backend calls:                                              │
│   • verify_password    src/auth/password.rs:12              │
│   • create_session     src/auth/session.rs:45               │
│   • generate_jwt       src/auth/jwt.rs:78                   │
└─────────────────────────────────────────────────────────────┘
```

### JSON Output

```bash
$ mimir trace process_user --json | jq '.callers'

[
  {
    "name": "handle_request",
    "file": "src/api.rs",
    "line": 89,
    "context": "let user = process_user(req).await?;"
  },
  {
    "name": "batch_process",
    "file": "src/jobs/batch.rs",
    "line": 156,
    "context": "users.push(process_user(item).await?);"
  }
]
```

---

## Semantic Search

```bash
$ mimir search "validate email format"

1. validate_email     src/validation/email.rs:12    (0.94)
2. check_email_format src/utils/email.rs:45        (0.87)
3. is_valid_email     src/models/user.rs:89        (0.82)
```

```bash
$ mimir search "handle authentication" --limit 10

 1. handle_login         src/handlers/auth.rs:45      (0.96)
 2. verify_token         src/middleware/auth.rs:23    (0.94)
 3. authenticate_user    src/services/auth.rs:67      (0.92)
 4. check_permissions    src/middleware/rbac.rs:34    (0.85)
 5. validate_session     src/auth/session.rs:89       (0.83)
 ...
```

---

## Task-Based Workflow

### Starting a Task

```bash
$ mimir task "Split the auth module into separate crates for JWT, OAuth, and sessions"

╭──────────────────────────────────────────────────────────────────────────╮
│ Task: refactor-auth-module (ID: task_a1b2c3)                             │
├──────────────────────────────────────────────────────────────────────────┤
│ Status: PLANNING                                                          │
│                                                                          │
│ Analyzing current auth module structure...                               │
│                                                                          │
│ Found:                                                                    │
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
│ ✓ Checkpoint saved                                                        │
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
│ Active Tasks                                                              │
├──────────────────────────────────────────────────────────────────────────┤
│ 1. refactor-auth-module (task_a1b2c3)                                    │
│    Status: PAUSED at step 3/6                                            │
│    Last activity: 2 hours ago                                            │
│    Files modified: 8                                                     │
│    Files pending: 15                                                     │
│    Checkpoint: cp_x7y8z9                                                  │
╰──────────────────────────────────────────────────────────────────────────╯

$ mimir resume

╭──────────────────────────────────────────────────────────────────────────╮
│ Resuming: refactor-auth-module                                           │
├──────────────────────────────────────────────────────────────────────────┤
│ Restoring context from checkpoint cp_x7y8z9...                           │
│                                                                          │
│ Previously completed:                                                     │
│   ✓ Step 1: Created crates/jwt                                           │
│   ✓ Step 2: Created crates/oauth                                         │
│   → Step 3: Creating crates/sessions (in progress)                       │
│                                                                          │
│ Key decisions made:                                                       │
│   • JWT crate uses jsonwebtoken v9.2                                      │
│   • OAuth crate depends on oauth2 v4.4                                    │
│   • Keeping backward-compatible re-exports in src/auth                   │
│                                                                          │
│ Continuing from step 3...                                                 │
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
│ Options:                                                                  │
│   [1] Make JwtValidator dependencies public                              │
│   [2] Keep JwtValidator in auth module                                    │
│   [3] Rollback to checkpoint and try different approach                  │
│   [4] Pause and let me investigate manually                              │
│                                                                          │
│ Choice [1/2/3/4]:                                                         │
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
│   → Writing tests for: validate_user (3/7 cases done) │
│                                                        │
│ Remaining:                                             │
│   • Tests for: authenticate, refresh_token             │
│   • Edge cases and error handling                      │
│   • Integration tests                                  │
│                                                        │
│ Time elapsed: 4m 32s                                   │
│ Checkpoints: 4 (last: 30s ago)                         │
│                                                        │
│ [p]ause  [c]ancel  [d]etails  [Enter] continue        │
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
    init        Initialize Mímir in current directory
    status      Show project status
    daemon      Manage background daemon (start, stop, status)
    trace       Trace a symbol (show callers, callees, similar)
    search      Semantic code search
    edit        Edit files with conflict detection
    task        Start a new task
    resume      Resume paused task
    recover     Recover from crash
    rollback    Rollback to snapshot
    sync        Sync with filesystem
    watch       Watch for changes
    config      Manage configuration
    agent       Interactive agent mode
    help        Show this help

OPTIONS:
    -v, --verbose    Verbose output
    -q, --quiet      Quiet output
    --json           JSON output
    --version        Show version

$ mimir daemon --help

Manage background daemon

USAGE:
    mimir daemon <COMMAND>

COMMANDS:
    start       Start daemon (use --foreground for debug mode)
    stop        Stop running daemon
    status      Show daemon status
```
