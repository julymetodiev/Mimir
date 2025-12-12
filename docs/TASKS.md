# Mímir Task Management System

This document describes the task lifecycle, checkpointing, and recovery mechanisms.

---

## Why Task Management?

**Problem with current systems** (Claude Code, Cursor, Aider):
- Context fills up → old messages deleted randomly
- Crash = start over from scratch
- No visibility into progress
- Long tasks fail unpredictably

**Mímir solution**:
- Tasks are first-class citizens with lifecycle
- Automatic checkpointing
- Crash recovery
- Progress tracking
- Context management

---

## Task Lifecycle

```
User Request
     │
     ▼
┌─────────────────┐
│  Parse & Plan   │ ──► Creates Task with steps
└────────┬────────┘
         │
         ▼
┌─────────────────┐     ┌─────────────────┐
│   Checkpoint    │ ──► │  task_001.json  │  Persist IMMEDIATELY
└────────┬────────┘     └─────────────────┘
         │
         ▼
┌─────────────────┐
│  Execute Step 1 │ ──► Analyze current structure
└────────┬────────┘
         │
         ▼
┌─────────────────┐     ┌─────────────────┐
│   Checkpoint    │ ──► │  Update state   │
└────────┬────────┘     └─────────────────┘
         │
         ▼
┌─────────────────┐
│  Execute Step 2 │ ──► Create new files
└────────┬────────┘
         │
        ...
         │
         ▼
┌─────────────────┐
│    Complete     │
└─────────────────┘
```

---

## Task Model

```rust
/// A task with everything needed for recovery
#[derive(Serialize, Deserialize)]
pub struct Task {
    pub id: TaskId,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,

    // User request
    pub original_request: String,
    pub interpreted_goal: String,

    // Plan
    pub plan: TaskPlan,

    // Execution state
    pub status: TaskStatus,
    pub current_step: usize,
    pub steps_completed: Vec<CompletedStep>,

    // Context management
    pub context: TaskContext,

    // Recovery info
    pub checkpoints: Vec<Checkpoint>,
    pub snapshot_id: Option<SnapshotId>,  // Git-like snapshot before start
}

#[derive(Serialize, Deserialize)]
pub struct TaskPlan {
    pub steps: Vec<PlannedStep>,
    pub estimated_duration: Option<Duration>,
    pub risk_assessment: RiskLevel,
    pub rollback_strategy: RollbackStrategy,
}

#[derive(Serialize, Deserialize)]
pub struct PlannedStep {
    pub id: StepId,
    pub description: String,
    pub action: StepAction,
    pub dependencies: Vec<StepId>,      // steps that must complete first
    pub estimated_tokens: u32,          // context budget
    pub can_checkpoint: bool,           // safe to pause after this step
    pub reversible: bool,               // can be undone
}
```

### Step Actions

```rust
#[derive(Serialize, Deserialize)]
pub enum StepAction {
    Analyze { targets: Vec<String> },
    Plan { scope: String },
    CreateFile { path: PathBuf, template: Option<String> },
    EditFile { path: PathBuf, operation: EditOp },
    MoveFile { from: PathBuf, to: PathBuf },
    DeleteFile { path: PathBuf },
    RunCommand { command: String, check_output: bool },
    RunTests { filter: Option<String> },
    Verify { checks: Vec<VerifyCheck> },
    AskUser { question: String },       // needs human input
}
```

### Task Status

```rust
#[derive(Serialize, Deserialize)]
pub enum TaskStatus {
    Planning,
    Executing { step: usize },
    Paused { reason: PauseReason },
    WaitingForUser { question: String },
    Completed { summary: String },
    Failed { error: String, recoverable: bool },
    Cancelled,
}

#[derive(Serialize, Deserialize)]
pub enum PauseReason {
    UserRequested,
    ContextLimitReached,
    ErrorNeedsReview,
    CheckpointReached,
    ExternalInterrupt,
}
```

---

## Checkpoint System

### Checkpoint Structure

```rust
/// Checkpoint = точка от която можем да възстановим
#[derive(Serialize, Deserialize)]
pub struct Checkpoint {
    pub id: CheckpointId,
    pub task_id: TaskId,
    pub step_id: StepId,
    pub created_at: DateTime<Utc>,

    // State snapshot
    pub file_states: HashMap<PathBuf, ContentHash>,
    pub graph_state: GraphStateRef,

    // Context that led here
    pub conversation_summary: String,
    pub key_decisions: Vec<Decision>,
    pub accumulated_knowledge: Knowledge,

    // What's next
    pub remaining_steps: Vec<StepId>,
    pub next_action: StepAction,
}
```

### Knowledge Accumulated

```rust
#[derive(Serialize, Deserialize)]
pub struct Knowledge {
    /// Symbols we've analyzed
    pub analyzed_symbols: HashMap<String, SymbolAnalysis>,

    /// Decisions made
    pub decisions: Vec<Decision>,

    /// Dependencies discovered
    pub discovered_dependencies: Vec<Dependency>,

    /// Issues encountered
    pub encountered_issues: Vec<Issue>,

    /// Files we've read (don't need to read again)
    pub files_read: HashSet<PathBuf>,

    /// Patterns identified
    pub patterns: Vec<CodePattern>,
}

#[derive(Serialize, Deserialize)]
pub struct Decision {
    pub description: String,
    pub reasoning: String,
    pub alternatives_considered: Vec<String>,
    pub timestamp: DateTime<Utc>,
}
```

### Checkpoint Creation

```rust
impl Checkpoint {
    /// Create checkpoint at current state
    pub fn create_now(task: &Task, graph: &CodeGraph) -> Self {
        Self {
            id: CheckpointId::new(),
            task_id: task.id,
            step_id: task.plan.steps[task.current_step].id,
            created_at: Utc::now(),

            file_states: graph.get_file_hashes(),
            graph_state: graph.snapshot_ref(),

            conversation_summary: task.context.summarize(),
            key_decisions: task.context.decisions.clone(),
            accumulated_knowledge: task.context.knowledge.clone(),

            remaining_steps: task.plan.steps[task.current_step..]
                .iter()
                .map(|s| s.id)
                .collect(),
            next_action: task.plan.steps[task.current_step].action.clone(),
        }
    }
}
```

### Checkpoint Restore

```rust
impl Checkpoint {
    /// Restore from checkpoint
    pub async fn restore(&self, graph: &CodeGraph) -> Result<TaskContext> {
        // Verify files haven't changed unexpectedly
        let current_hashes = graph.get_file_hashes();

        for (path, expected_hash) in &self.file_states {
            if let Some(current_hash) = current_hashes.get(path) {
                if current_hash != expected_hash {
                    return Err(RestoreError::FileChanged {
                        path: path.clone(),
                        expected: *expected_hash,
                        actual: *current_hash,
                    });
                }
            }
        }

        // Rebuild context
        Ok(TaskContext {
            knowledge: self.accumulated_knowledge.clone(),
            decisions: self.key_decisions.clone(),
            summary: self.conversation_summary.clone(),
            // ... rebuild rest
        })
    }
}
```

---

## Automatic Checkpointing Strategy

```rust
pub struct CheckpointStrategy {
    /// Checkpoint after every N steps
    pub step_interval: usize,

    /// Checkpoint after N tokens used
    pub token_interval: usize,

    /// Checkpoint after N minutes
    pub time_interval: Duration,

    /// Always checkpoint before these operations
    pub checkpoint_before: HashSet<StepActionType>,

    /// Checkpoint when context usage exceeds this %
    pub context_threshold: f32,
}

impl Default for CheckpointStrategy {
    fn default() -> Self {
        Self {
            step_interval: 3,
            token_interval: 50_000,
            time_interval: Duration::from_secs(300), // 5 min
            checkpoint_before: hashset![
                StepActionType::DeleteFile,
                StepActionType::MoveFile,
                StepActionType::RunCommand,
            ],
            context_threshold: 0.7, // 70% context used
        }
    }
}

impl TaskExecutor {
    async fn should_checkpoint(&self, step: &PlannedStep) -> bool {
        let strategy = &self.checkpoint_strategy;

        // Time-based
        if self.last_checkpoint.elapsed() > strategy.time_interval {
            return true;
        }

        // Step-based
        if self.steps_since_checkpoint >= strategy.step_interval {
            return true;
        }

        // Token-based
        if self.tokens_since_checkpoint >= strategy.token_interval {
            return true;
        }

        // Context threshold
        if self.context.usage_percent() > strategy.context_threshold {
            return true;
        }

        // Before dangerous operations
        if strategy.checkpoint_before.contains(&step.action.action_type()) {
            return true;
        }

        false
    }
}
```

---

## Failure Modes & Recovery

```
┌─────────────────────────────────────────────────────────────────────┐
│                        Failure Taxonomy                              │
└─────────────────────────────────────────────────────────────────────┘

1. GRACEFUL INTERRUPT (Ctrl+C, user pause)
   ├── Save current state immediately
   ├── Create checkpoint
   ├── Mark task as PAUSED
   └── Recovery: `mimir resume` continues seamlessly

2. CRASH (OOM, segfault, power loss)
   ├── Rely on last checkpoint
   ├── May lose work since last checkpoint
   └── Recovery: `mimir recover` replays from checkpoint

3. CONTEXT EXHAUSTION (LLM context full)
   ├── Automatic checkpoint
   ├── Compress context
   ├── Continue with fresh context + summary
   └── Recovery: Automatic, transparent to user

4. API FAILURE (network, rate limit, API error)
   ├── Retry with exponential backoff
   ├── If persistent, pause task
   └── Recovery: `mimir resume` when API available

5. EDIT CONFLICT (external file change)
   ├── Detect on next operation
   ├── Pause and notify user
   └── Recovery: User resolves, `mimir resume --merge`

6. LOGICAL ERROR (wrong edit, bad plan)
   ├── User can interrupt anytime
   ├── Rollback to checkpoint
   └── Recovery: `mimir rollback` + new instructions
```

---

## Graceful Shutdown Handler

```rust
pub struct ShutdownHandler {
    task_executor: Arc<TaskExecutor>,
    shutdown_tx: broadcast::Sender<()>,
}

impl ShutdownHandler {
    pub fn install(executor: Arc<TaskExecutor>) -> Self {
        let (shutdown_tx, _) = broadcast::channel(1);
        let handler = Self {
            task_executor: executor.clone(),
            shutdown_tx: shutdown_tx.clone(),
        };

        // Handle Ctrl+C
        let executor_clone = executor.clone();
        let tx = shutdown_tx.clone();
        ctrlc::set_handler(move || {
            eprintln!("\n⏸ Interrupt received, saving state...");

            // Signal shutdown
            let _ = tx.send(());

            // Block until saved (with timeout)
            let rt = tokio::runtime::Handle::current();
            rt.block_on(async {
                tokio::time::timeout(
                    Duration::from_secs(10),
                    executor_clone.graceful_shutdown()
                ).await
            }).ok();

            eprintln!("✓ State saved. Resume with: mimir resume");
            std::process::exit(0);
        }).expect("Error setting Ctrl-C handler");

        handler
    }
}

impl TaskExecutor {
    pub async fn graceful_shutdown(&self) -> Result<()> {
        // 1. Stop accepting new operations
        self.paused.store(true, Ordering::SeqCst);

        // 2. Wait for current operation to complete (with timeout)
        let _ = tokio::time::timeout(
            Duration::from_secs(5),
            self.current_operation_done.notified()
        ).await;

        // 3. Create checkpoint
        let checkpoint = Checkpoint::create_now(&self.task, &self.graph);
        self.save_checkpoint(&checkpoint).await?;

        // 4. Update task status
        self.task.status = TaskStatus::Paused {
            reason: PauseReason::UserRequested,
        };
        self.save_task().await?;

        // 5. Flush any pending writes
        self.graph.flush().await?;

        Ok(())
    }
}
```

---

## Resumable Summary

When resuming a task, we generate a continuation prompt:

```rust
#[derive(Serialize, Deserialize)]
pub struct ResumableSummary {
    pub task_description: String,
    pub facts: Vec<Fact>,
    pub decisions: Vec<Decision>,
    pub current_step: String,
    pub remaining_work: Vec<String>,
    pub issues: Vec<Issue>,
}

impl ResumableSummary {
    /// Convert to prompt for LLM to continue
    pub fn to_continuation_prompt(&self) -> String {
        format!(r#"
# Continuing Task

## Original Goal
{task_description}

## What We've Learned
{facts}

## Decisions Made
{decisions}

## Current Status
{current_step}

## Remaining Work
{remaining}

## Issues to Address
{issues}

Continue from where we left off.
"#,
            task_description = self.task_description,
            facts = self.format_facts(),
            decisions = self.format_decisions(),
            current_step = self.current_step,
            remaining = self.format_remaining(),
            issues = self.format_issues(),
        )
    }
}
```

---

## Task Storage Format

```
.agent/tasks/
├── active/
│   └── task_a1b2c3/
│       ├── task.json           # Task definition & state
│       ├── plan.json           # Execution plan
│       ├── context.bin         # Serialized context (compressed)
│       ├── checkpoints/
│       │   ├── cp_001.json     # Checkpoint metadata
│       │   ├── cp_001_ctx.bin  # Checkpoint context
│       │   ├── cp_002.json
│       │   └── cp_002_ctx.bin
│       └── logs/
│           ├── execution.log   # Detailed execution log
│           └── decisions.log   # Key decisions made
│
├── completed/
│   └── task_x7y8z9/
│       └── summary.json        # Completed task summary
│
└── failed/
    └── task_m4n5o6/
        ├── task.json
        └── error.log
```

### Task File Format

```json
// .agent/tasks/active/task_a1b2c3/task.json
{
  "id": "task_a1b2c3",
  "version": 1,
  "created_at": "2024-01-15T10:30:00Z",
  "updated_at": "2024-01-15T10:45:32Z",

  "request": {
    "original": "Split the auth module into separate crates",
    "interpreted": "Refactor src/auth into three crates: jwt, oauth, sessions"
  },

  "status": {
    "type": "executing",
    "step": 3,
    "substep": "moving_files"
  },

  "plan": {
    "steps": [
      {
        "id": "step_1",
        "description": "Create crates/jwt",
        "status": "completed",
        "started_at": "2024-01-15T10:31:00Z",
        "completed_at": "2024-01-15T10:33:45Z"
      }
    ],
    "estimated_total_minutes": 18,
    "actual_elapsed_minutes": 15
  },

  "knowledge": {
    "analyzed_symbols": ["auth::jwt::*", "auth::oauth::*"],
    "decisions": [
      {
        "what": "Use jsonwebtoken v9.2 for JWT crate",
        "why": "Already in use, maintains compatibility",
        "when": "2024-01-15T10:32:00Z"
      }
    ],
    "issues_encountered": []
  },

  "recovery": {
    "snapshot_before": "snap_20240115_103000",
    "checkpoints": ["cp_001", "cp_002", "cp_003"],
    "last_checkpoint": "cp_003",
    "last_checkpoint_at": "2024-01-15T10:44:00Z"
  }
}
```

---

## Next: See CONTEXT.md for LLM context management
