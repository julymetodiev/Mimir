# Mímir LLM Context Management

This document describes how Mímir manages LLM context windows efficiently.

---

## The Problem

```
Typical LLM context: 200k tokens (Claude), 128k (GPT-4)

Task "refactor auth module" may require:
- System prompt: ~2k tokens
- Task description: ~500 tokens
- Code analysis: ~50k tokens (reading many files)
- Conversation history: ~30k tokens (discussion, planning)
- Current operation: ~10k tokens
- Response space: ~8k tokens
─────────────────────────────
Total: ~100k tokens

Remaining ~100k for continuation... but:
- Each subsequent step adds to conversation
- Code snippets accumulate
- After ~20 steps context is full
```

**Current systems**: Context fills up → old messages deleted randomly → lose important decisions

**Mímir**: Hierarchical context with intelligent compression

---

## Hierarchical Context System

Three-layer context management:

```rust
pub struct HierarchicalContext {
    // Layer 1: Persistent (never deleted)
    core: CoreContext,

    // Layer 2: Session (periodically compressed)
    session: SessionContext,

    // Layer 3: Working (active operation, can be cleared)
    working: WorkingContext,
}
```

### Layer 1: Core Context (~5k tokens)

**Always present. Never compressed.**

```rust
pub struct CoreContext {
    /// System prompt - always present
    system_prompt: String,          // ~2k tokens

    /// Task definition - always present
    task: TaskDefinition,           // ~500 tokens

    /// Project structure (compact)
    project_map: ProjectMap,        // ~1k tokens

    /// Key decisions made (append-only log)
    decisions: Vec<Decision>,       // grows, but summarized
}
```

### Layer 2: Session Context (~50k tokens max)

**Bounded. LRU eviction + summarization.**

```rust
pub struct SessionContext {
    /// Summarized history of what we've done
    history_summary: String,        // ~2k tokens, updated periodically

    /// Currently relevant symbols (LRU cache)
    active_symbols: LruCache<SymbolId, SymbolContext>, // ~10k tokens max

    /// Files we're working with (LRU cache)
    active_files: LruCache<PathBuf, FileContext>,     // ~20k tokens max

    /// Recent conversation (sliding window)
    recent_messages: VecDeque<Message>,              // ~20k tokens max
}
```

### Layer 3: Working Context (variable)

**Cleared between steps.**

```rust
pub struct WorkingContext {
    /// Current step we're executing
    current_step: StepContext,      // varies

    /// Code being analyzed/edited right now
    working_code: Vec<CodeSnippet>, // varies

    /// Temporary reasoning
    scratch: String,                // cleared after step
}
```

---

## Context Composition

```rust
impl HierarchicalContext {
    pub fn to_messages(&self) -> Vec<Message> {
        let mut messages = vec![];

        // Always include core
        messages.push(Message::System(self.core.to_prompt()));

        // Include session summary
        messages.push(Message::System(format!(
            "## Session Context\n{}",
            self.session.to_summary()
        )));

        // Include recent conversation
        messages.extend(self.session.recent_messages.iter().cloned());

        // Include working context
        messages.push(Message::User(self.working.to_prompt()));

        messages
    }

    pub fn available_for_working(&self) -> usize {
        let used = self.core.token_count()
            + self.session.token_count();

        self.max_tokens
            .saturating_sub(used)
            .saturating_sub(self.reserved_for_response)
    }
}
```

---

## Context Compression

When session context gets too large:

```rust
impl HierarchicalContext {
    pub async fn compress_session(&mut self) -> Result<()> {
        // Summarize old messages
        let old_messages: Vec<_> = self.session.recent_messages
            .drain(..self.session.recent_messages.len() / 2)
            .collect();

        let summary = self.summarizer.summarize(&old_messages).await?;

        // Update history summary
        self.session.history_summary = format!(
            "{}\n\n## Recent:\n{}",
            self.session.history_summary,
            summary
        );

        // Evict least-used symbols and files
        self.session.active_symbols.resize(
            self.session.active_symbols.len() / 2
        );
        self.session.active_files.resize(
            self.session.active_files.len() / 2
        );

        Ok(())
    }
}
```

---

## Smart Code Loading

Load only what's needed, truncate intelligently:

```rust
pub struct SmartLoader {
    graph: Arc<CodeGraph>,
    context: Arc<Mutex<HierarchicalContext>>,
}

impl SmartLoader {
    /// Load symbol with just enough context
    pub async fn load_symbol(&self, name: &str) -> Result<SymbolContext> {
        let trace = self.graph.trace(name, TraceOptions {
            max_depth: 1,           // only direct callers/callees
            include_similar: false, // not now
            context_lines: 2,       // minimal context
            ..Default::default()
        }).await?;

        // Determine what to include based on available space
        let available = self.context.lock().await.available_for_working();

        let mut result = SymbolContext::new(trace.symbol.clone());
        let mut tokens_used = result.token_count();

        // Add definition
        result.definition = Some(self.load_definition(&trace)?);
        tokens_used += result.definition.as_ref().map_or(0, |d| d.token_count());

        // Add callers if space permits
        for caller in &trace.callers {
            let caller_context = self.load_call_site(caller)?;
            if tokens_used + caller_context.token_count() > available {
                result.truncated = true;
                break;
            }
            result.callers.push(caller_context);
            tokens_used += caller_context.token_count();
        }

        // Cache in session context
        self.context.lock().await
            .session.active_symbols
            .put(trace.symbol.id, result.clone());

        Ok(result)
    }

    /// Load file with smart truncation
    pub async fn load_file(&self, path: &Path, focus: Option<Span>) -> Result<FileContext> {
        let content = self.graph.read_file(path).await?;
        let lines: Vec<_> = content.lines().collect();

        let available = self.context.lock().await.available_for_working();
        let total_tokens = estimate_tokens(&content);

        if total_tokens <= available {
            // File fits entirely
            return Ok(FileContext::Full { content, path: path.to_path_buf() });
        }

        // Need to truncate - focus on relevant part
        if let Some(focus_span) = focus {
            let focus_line = self.span_to_line(&content, focus_span);
            let context_lines = self.calculate_context_lines(available, &lines);

            let start = focus_line.saturating_sub(context_lines / 2);
            let end = (focus_line + context_lines / 2).min(lines.len());

            Ok(FileContext::Partial {
                path: path.to_path_buf(),
                content: lines[start..end].join("\n"),
                range: (start, end),
                total_lines: lines.len(),
            })
        } else {
            // No focus - show beginning and structure
            Ok(FileContext::Summary {
                path: path.to_path_buf(),
                structure: self.extract_structure(&content),
                preview: lines[..50.min(lines.len())].join("\n"),
            })
        }
    }
}
```

---

## File Context Types

```rust
pub enum FileContext {
    /// Entire file fits
    Full {
        path: PathBuf,
        content: String,
    },

    /// Truncated around focus point
    Partial {
        path: PathBuf,
        content: String,
        range: (usize, usize),  // start_line, end_line
        total_lines: usize,
    },

    /// Just structure + preview
    Summary {
        path: PathBuf,
        structure: String,      // function signatures, class definitions
        preview: String,        // first N lines
    },
}
```

---

## Context Manager

```rust
pub struct ContextManager {
    max_tokens: usize,              // e.g., 200k for Claude
    reserved_for_response: usize,   // e.g., 8k

    // Current context
    system_prompt_tokens: usize,
    conversation: Vec<Message>,
    conversation_tokens: usize,

    // Compression
    summarizer: Summarizer,
}

impl ContextManager {
    pub fn available_tokens(&self) -> usize {
        self.max_tokens
            .saturating_sub(self.system_prompt_tokens)
            .saturating_sub(self.conversation_tokens)
            .saturating_sub(self.reserved_for_response)
    }

    /// Add message, compress if needed
    pub async fn add_message(&mut self, msg: Message) -> Result<()> {
        let msg_tokens = count_tokens(&msg);

        // Check if we need to compress
        if self.conversation_tokens + msg_tokens > self.soft_limit() {
            self.compress().await?;
        }

        self.conversation.push(msg);
        self.conversation_tokens += msg_tokens;

        Ok(())
    }

    fn soft_limit(&self) -> usize {
        // Start compressing at 70% capacity
        (self.max_tokens as f32 * 0.7) as usize
    }

    /// Compress conversation history
    async fn compress(&mut self) -> Result<()> {
        // Strategy 1: Summarize older messages
        let (old, recent) = self.conversation.split_at(self.conversation.len() / 2);

        let summary = self.summarizer.summarize(old).await?;

        // Replace old messages with summary
        let mut new_conversation = vec![Message::System(summary)];
        new_conversation.extend(recent.iter().cloned());

        self.conversation = new_conversation;
        self.conversation_tokens = self.count_all_tokens();

        Ok(())
    }

    /// Create resumable summary for checkpoint
    pub fn create_resumable_summary(&self) -> ResumableSummary {
        ResumableSummary {
            task_description: self.extract_task_description(),
            facts: self.extract_facts(),
            decisions: self.extract_decisions(),
            current_step: self.extract_current_step(),
            remaining_work: self.extract_remaining(),
            issues: self.extract_issues(),
        }
    }
}
```

---

## Context Budget Allocation

```
┌─────────────────────────────────────────────────────────────────┐
│                    200k Token Budget                             │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  ┌─────────────────────────────────────────────────┐            │
│  │  Core Context (5k)                               │            │
│  │  • System prompt                                 │  FIXED     │
│  │  • Task definition                               │            │
│  │  • Key decisions                                 │            │
│  └─────────────────────────────────────────────────┘            │
│                                                                  │
│  ┌─────────────────────────────────────────────────┐            │
│  │  Session Context (50k max)                       │            │
│  │  • History summary                               │  BOUNDED   │
│  │  • Active symbols (LRU)                          │  + LRU     │
│  │  • Active files (LRU)                            │            │
│  │  • Recent messages                               │            │
│  └─────────────────────────────────────────────────┘            │
│                                                                  │
│  ┌─────────────────────────────────────────────────┐            │
│  │  Working Context (variable, up to ~130k)         │            │
│  │  • Current step details                          │  VARIABLE  │
│  │  • Code being analyzed                           │  CLEARED   │
│  │  • Temporary reasoning                           │            │
│  └─────────────────────────────────────────────────┘            │
│                                                                  │
│  ┌─────────────────────────────────────────────────┐            │
│  │  Response Reserve (8k)                           │  FIXED     │
│  └─────────────────────────────────────────────────┘            │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

---

## Automatic Context Management Triggers

| Trigger | Action |
|---------|--------|
| Session context > 50k tokens | Compress session |
| Total context > 70% | Start compression |
| Total context > 90% | Aggressive compression |
| Step completed | Clear working context |
| Checkpoint created | Persist context summary |

---

## Integration with Tasks

Context is managed alongside task checkpoints:

```rust
impl TaskExecutor {
    async fn execute_step(&mut self, step: &PlannedStep) -> Result<StepResult> {
        // 1. Check context budget
        let available = self.context.available_for_working();
        if available < step.estimated_tokens {
            self.context.compress().await?;
        }

        // 2. Load required context
        for target in &step.analysis_targets {
            self.context.load_symbol(target).await?;
        }

        // 3. Execute step
        let result = self.run_step(step).await?;

        // 4. Clear working context
        self.context.clear_working();

        // 5. Update session with results
        self.context.session.add_step_result(&result);

        Ok(result)
    }
}
```

---

## Next: See CLI.md for usage examples
