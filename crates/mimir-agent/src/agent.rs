//! Core agent implementation.

use crate::{AgentConfig, AgentError, AgentResult, DEFAULT_SYSTEM_PROMPT, ToolExecutor};
use futures::future::join_all;
use mimir_llm::{Completion, CompletionRequest, ContentBlock, LlmProvider, Message, Role};
use mimir_tasks::{Checkpoint, CheckpointManager, ConversationMessage, Task};
use mimir_tools::{GraphClient, ToolContext, ToolRegistry};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, info, warn};

/// Maximum size for tool results before compression (in chars).
const MAX_RESULT_SIZE: usize = 8000;

/// Compress large tool results to save tokens.
fn compress_result(content: &str) -> String {
    if content.len() <= MAX_RESULT_SIZE {
        return content.to_string();
    }

    // Try to parse as JSON (handle both arrays and objects with arrays)
    if let Ok(mut json) = serde_json::from_str::<serde_json::Value>(content) {
        let mut modified = false;

        // Helper to truncate array
        let truncate_array = |val: &mut serde_json::Value, limit: usize| -> bool {
            if let Some(arr) = val.as_array_mut() {
                if arr.len() > limit {
                    arr.truncate(limit);
                    return true;
                }
            }
            false
        };

        if json.is_array() {
            if truncate_array(&mut json, 10) {
                 // For root arrays, we can't easily add a "note", but keeping it valid JSON is better
                 modified = true;
            }
        } else if let Some(obj) = json.as_object_mut() {
            for field in ["matches", "symbols", "callers", "callees", "references"] {
                if let Some(val) = obj.get_mut(field) {
                    if truncate_array(val, 10) {
                        modified = true;
                    }
                }
            }
            
            if modified {
                // Add a warning field to indicate truncation
                obj.insert("_warning".to_string(), serde_json::Value::String("Output truncated for brevity".to_string()));
            }
        }

        if modified {
            return json.to_string();
        }
    }


    // Fallback: smart truncation keeping start and end (crucial for error logs)
    let keep_len = MAX_RESULT_SIZE / 2;
    if content.len() > MAX_RESULT_SIZE {
        let start = &content[..keep_len];
        let end = &content[content.len() - keep_len..];
        return format!(
            "{}\n\n... ({} chars truncated) ...\n\n{}",
            start,
            content.len() - MAX_RESULT_SIZE,
            end
        );
    }

    content.to_string()
}

/// Cache for tool call results to avoid duplicate executions.
#[derive(Debug, Default)]
pub struct ToolCache {
    /// Cached results: (tool_name, input_json) -> (output, is_error)
    entries: HashMap<String, CacheEntry>,
    /// Tools that are safe to cache (read-only operations)
    cacheable_tools: Vec<String>,
    /// Number of cache hits
    hits: usize,
}

#[derive(Debug, Clone)]
struct CacheEntry {
    content: String,
    is_error: bool,
}

impl ToolCache {
    /// Create a new tool cache with default cacheable tools.
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            cacheable_tools: vec![
                "search".to_string(),
                "read".to_string(),
                "trace".to_string(),
            ],
            hits: 0,
        }
    }

    /// Check if a tool is cacheable.
    fn is_cacheable(&self, tool_name: &str) -> bool {
        self.cacheable_tools.iter().any(|t| t == tool_name)
    }

    /// Generate cache key from tool name and input.
    fn cache_key(tool_name: &str, input: &serde_json::Value) -> String {
        format!("{}:{}", tool_name, input.to_string())
    }

    /// Get cached result if available.
    pub fn get(&mut self, tool_name: &str, input: &serde_json::Value) -> Option<(String, bool)> {
        if !self.is_cacheable(tool_name) {
            return None;
        }
        let key = Self::cache_key(tool_name, input);
        self.entries.get(&key).map(|e| {
            self.hits += 1;
            (e.content.clone(), e.is_error)
        })
    }

    /// Store result in cache.
    pub fn store(
        &mut self,
        tool_name: &str,
        input: &serde_json::Value,
        content: String,
        is_error: bool,
    ) {
        if !self.is_cacheable(tool_name) {
            return;
        }
        let key = Self::cache_key(tool_name, input);
        self.entries.insert(key, CacheEntry { content, is_error });
    }

    /// Get number of cache hits.
    pub fn hits(&self) -> usize {
        self.hits
    }

    /// Get number of cached entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if cache is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Invalidate cache entries for a specific file.
    /// Called after edit operations to only clear affected entries.
    pub fn invalidate_file(&mut self, file_path: &str) {
        // Extract just the filename for matching
        let filename = file_path.rsplit('/').next().unwrap_or(file_path);
        self.entries.retain(|key, _| !key.contains(filename));
    }

    /// Invalidate all cache entries.
    /// Called after shell commands which may affect any file.
    pub fn invalidate_all(&mut self) {
        self.entries.clear();
    }

    /// Check if a tool is a mutating operation that should invalidate the cache.
    pub fn is_mutating(tool_name: &str) -> bool {
        matches!(tool_name, "edit" | "edit_batch" | "edit_file" | "shell")
    }

    /// Check if tool is an edit operation (not shell).
    pub fn is_edit(tool_name: &str) -> bool {
        matches!(tool_name, "edit" | "edit_batch" | "edit_file")
    }

    /// Extract file path from tool input if present.
    pub fn extract_file(input: &serde_json::Value) -> Option<String> {
        input
            .get("file")
            .or_else(|| input.get("path"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }
}

/// Callback for agent events.
pub trait AgentCallback: Send + Sync {
    /// Called when the agent starts processing.
    fn on_start(&self, _task: &Task) {}

    /// Called before each LLM request.
    fn on_llm_request(&self, _messages: &[Message]) {}

    /// Called after LLM response.
    fn on_llm_response(&self, _completion: &Completion) {}

    /// Called before tool execution.
    fn on_tool_start(&self, _name: &str, _input: &serde_json::Value) {}

    /// Called after tool execution.
    fn on_tool_result(&self, _name: &str, _result: &str, _is_error: bool) {}

    /// Called when a tool result is served from cache.
    fn on_tool_cache_hit(&self, _name: &str, _input: &serde_json::Value) {}

    /// Called when a checkpoint is created.
    fn on_checkpoint(&self, _checkpoint: &Checkpoint) {}

    /// Called when the agent completes.
    fn on_complete(&self, _task: &Task) {}

    /// Called on error.
    fn on_error(&self, _error: &AgentError) {}

    /// Request approval for tool execution. Returns true if approved.
    fn approve_tool(&self, _name: &str, _input: &serde_json::Value) -> bool {
        true
    }
}

/// Default callback that does nothing.
pub struct NoOpCallback;
impl AgentCallback for NoOpCallback {}

/// The main agent that orchestrates LLM and tools.
pub struct Agent<P: LlmProvider> {
    provider: P,
    config: AgentConfig,
    tool_registry: Arc<ToolRegistry>,
    callback: Arc<dyn AgentCallback>,
    graph_client: Option<Arc<dyn GraphClient>>,
}

impl<P: LlmProvider> Agent<P> {
    /// Create a new agent.
    pub fn new(provider: P, config: AgentConfig, tool_registry: ToolRegistry) -> Self {
        Self {
            provider,
            config,
            tool_registry: Arc::new(tool_registry),
            callback: Arc::new(NoOpCallback),
            graph_client: None,
        }
    }

    /// Set a callback for agent events.
    pub fn with_callback(mut self, callback: Arc<dyn AgentCallback>) -> Self {
        self.callback = callback;
        self
    }

    /// Set the graph client for semantic operations.
    pub fn with_graph_client(mut self, client: Arc<dyn GraphClient>) -> Self {
        self.graph_client = Some(client);
        self
    }

    /// Run the agent on a task.
    pub async fn run(&self, task: &mut Task) -> AgentResult<String> {
        debug!("Starting agent for task: {}", task.goal);
        self.callback.on_start(task);

        task.start();

        // Build initial message
        let initial_message = if let Some(context) = &task.context {
            Message::user(format!("{}\n\nContext: {}", task.goal, context))
        } else {
            Message::user(&task.goal)
        };

        self.run_with_messages(task, vec![initial_message], 0).await
    }

    /// Internal method to run agent with given initial messages.
    async fn run_with_messages(
        &self,
        task: &mut Task,
        initial_messages: Vec<Message>,
        start_iteration: u32,
    ) -> AgentResult<String> {
        let working_dir = task
            .working_dir
            .clone()
            .or_else(|| self.config.working_dir.clone())
            .unwrap_or_else(|| ".".to_string());

        let mut tool_context = ToolContext::new(&working_dir);
        if let Some(client) = &self.graph_client {
            tool_context = tool_context.with_graph_client(client.clone());
        }
        let executor = ToolExecutor::new(self.tool_registry.clone(), tool_context);

        let mut checkpoint_manager = CheckpointManager::new(task.id.clone());
        let mut messages = initial_messages;
        let mut tool_cache = ToolCache::new();
        let mut consecutive_errors = 0;

        let system_prompt = self
            .config
            .system_prompt
            .clone()
            .unwrap_or_else(|| DEFAULT_SYSTEM_PROMPT.to_string());

        let mut iteration = start_iteration;
        let mut final_response = String::new();

        loop {
            iteration += 1;
            if iteration > self.config.max_iterations {
                warn!("Max iterations reached: {}", self.config.max_iterations);
                task.fail(format!(
                    "Max iterations reached: {}",
                    self.config.max_iterations
                ));
                return Err(AgentError::MaxIterations(self.config.max_iterations));
            }

            debug!(
                "Iteration {}: sending {} messages",
                iteration,
                messages.len()
            );
            self.callback.on_llm_request(&messages);

            // Build request
            let mut request = CompletionRequest::new(&self.config.model, messages.clone())
                .with_system(&system_prompt)
                .with_max_tokens(self.config.max_tokens)
                .with_tools(executor.tool_schemas());

            if let Some(temp) = self.config.temperature {
                request = request.with_temperature(temp);
            }

            // Call LLM
            let completion = self.provider.complete(request).await?;
            self.callback.on_llm_response(&completion);

            // Record usage
            task.record_tool_call(completion.usage.total_tokens());

            // Get text response
            let response_text = completion.text();
            if !response_text.is_empty() {
                final_response = response_text.clone();
            }

            // Add assistant message to history
            messages.push(completion.message.clone());

            // Check if we need to execute tools
            if completion.wants_tool_use() {
                let tool_uses: Vec<(String, String, serde_json::Value)> = completion
                    .message
                    .content
                    .iter()
                    .filter_map(|c| {
                        if let ContentBlock::ToolUse { id, name, input } = c {
                            Some((id.clone(), name.clone(), input.clone()))
                        } else {
                            None
                        }
                    })
                    .collect();

                // Separate into read-only (parallelizable) and mutating (sequential)
                let (read_only, mutating): (Vec<_>, Vec<_>) = tool_uses
                    .iter()
                    .partition(|(_, name, _)| !ToolCache::is_mutating(name));

                let mut tool_results = vec![];

                // Execute read-only tools in parallel
                if !read_only.is_empty() {
                    let parallel_futures: Vec<_> = read_only
                        .iter()
                        .filter_map(|(id, name, input)| {
                            // Check cache first
                            if let Some((cached_content, cached_is_error)) =
                                tool_cache.get(name, input)
                            {
                                debug!("Cache hit for tool {}", name);
                                self.callback.on_tool_cache_hit(name, input);
                                self.callback
                                    .on_tool_result(name, &cached_content, cached_is_error);
                                tool_results.push(ContentBlock::ToolResult {
                                    tool_use_id: id.clone(),
                                    content: cached_content,
                                    is_error: cached_is_error,
                                });
                                return None;
                            }

                            self.callback.on_tool_start(name, input);

                            // Check approval
                            if !self.config.auto_approve && !self.callback.approve_tool(name, input)
                            {
                                warn!("Tool {} not approved", name);
                                tool_results.push(ContentBlock::ToolResult {
                                    tool_use_id: id.clone(),
                                    content: "Tool execution not approved by user".to_string(),
                                    is_error: true,
                                });
                                return None;
                            }

                            let id = id.clone();
                            let name = name.clone();
                            let input = input.clone();
                            let exec = &executor;

                            Some(async move {
                                let result = exec.execute_tool(&name, input.clone()).await;
                                (id, name, input, result)
                            })
                        })
                        .collect();

                    // Execute in parallel
                    let parallel_results = join_all(parallel_futures).await;

                    for (id, name, input, result) in parallel_results {
                        let is_error = !result.success;
                        let raw_content = if result.success {
                            result.output.to_string()
                        } else {
                            result.error.clone().unwrap_or_else(|| "Unknown error".to_string())
                        };
                        let content = compress_result(&raw_content);

                        tool_cache.store(&name, &input, content.clone(), is_error);
                        self.callback.on_tool_result(&name, &content, is_error);

                        tool_results.push(ContentBlock::ToolResult {
                            tool_use_id: id,
                            content,
                            is_error,
                        });
                    }
                }

                // Execute mutating tools sequentially
                for (id, name, input) in &mutating {
                    // Check cache (unlikely for mutating, but check anyway)
                    if let Some((cached_content, cached_is_error)) = tool_cache.get(name, input) {
                        debug!("Cache hit for tool {}", name);
                        self.callback.on_tool_cache_hit(name, input);
                        self.callback
                            .on_tool_result(name, &cached_content, cached_is_error);
                        tool_results.push(ContentBlock::ToolResult {
                            tool_use_id: id.clone(),
                            content: cached_content,
                            is_error: cached_is_error,
                        });
                        continue;
                    }

                    self.callback.on_tool_start(name, input);

                    if !self.config.auto_approve && !self.callback.approve_tool(name, input) {
                        warn!("Tool {} not approved", name);
                        tool_results.push(ContentBlock::ToolResult {
                            tool_use_id: id.clone(),
                            content: "Tool execution not approved by user".to_string(),
                            is_error: true,
                        });
                        continue;
                    }

                    let result = executor.execute_tool(name, input.clone()).await;
                    let is_error = !result.success;
                    let raw_content = if result.success {
                        result.output.to_string()
                    } else {
                        result.error.clone().unwrap_or_else(|| "Unknown error".to_string())
                    };
                    let content = compress_result(&raw_content);

                    // Smart cache invalidation
                    if ToolCache::is_edit(name) {
                        if let Some(file) = ToolCache::extract_file(input) {
                            tool_cache.invalidate_file(&file);
                        }
                    } else {
                        tool_cache.invalidate_all();
                    }

                    self.callback.on_tool_result(name, &content, is_error);

                    tool_results.push(ContentBlock::ToolResult {
                        tool_use_id: id.clone(),
                        content,
                        is_error,
                    });
                }

                // Add tool results as user message
                messages.push(Message {
                    role: Role::User,
                    content: tool_results.clone(),
                });

                // Check for consecutive errors
                let has_success = tool_results.iter().any(|block| {
                    if let ContentBlock::ToolResult { is_error, .. } = block {
                        !*is_error
                    } else {
                        false
                    }
                });

                if !tool_results.is_empty() {
                    if has_success {
                        consecutive_errors = 0;
                    } else {
                        consecutive_errors += 1;
                    }
                }

                if consecutive_errors >= 3 {
                    messages.push(Message::user("SYSTEM WARNING: You have failed 3 times in a row. Stop and reflect. Read the file again. Verify your parameters. Do NOT repeat the same mistake."));
                    consecutive_errors = 0;
                }

                // Create checkpoint if enabled
                if self.config.checkpoint_enabled {
                    let checkpoint = self.create_checkpoint(
                        &mut checkpoint_manager,
                        &messages,
                        &format!("After iteration {}", iteration),
                    );
                    self.callback.on_checkpoint(&checkpoint);
                }
            } else {
                // No tool use, we're done
                break;
            }
        }

        task.complete();
        self.callback.on_complete(task);

        if tool_cache.hits() > 0 {
            info!(
                "Agent completed task in {} iterations ({} cache hits, {} cached entries)",
                iteration,
                tool_cache.hits(),
                tool_cache.len()
            );
        } else {
            debug!("Agent completed task in {} iterations", iteration);
        }
        Ok(final_response)
    }

    /// Resume a task from a checkpoint.
    pub async fn resume(&self, task: &mut Task, checkpoint: Checkpoint) -> AgentResult<String> {
        info!(
            "Resuming task {} from checkpoint {} (sequence {})",
            task.id, checkpoint.id, checkpoint.sequence
        );
        self.callback.on_start(task);

        task.start();

        // Restore messages from checkpoint
        let messages: Vec<Message> = checkpoint
            .messages
            .iter()
            .filter_map(|m| {
                let role = match m.role.as_str() {
                    "user" => Role::User,
                    "assistant" => Role::Assistant,
                    "system" => return None, // System messages are handled separately
                    _ => Role::User,
                };

                // Try to restore full content structure
                let content = if let Ok(blocks) =
                    serde_json::from_value::<Vec<ContentBlock>>(m.content.clone())
                {
                    blocks
                } else if let Some(text) = m.content.as_str() {
                    vec![ContentBlock::Text {
                        text: text.to_string(),
                    }]
                } else {
                    vec![ContentBlock::Text {
                        text: m.content.to_string(),
                    }]
                };

                Some(Message { role, content })
            })
            .collect();

        if messages.is_empty() {
            // No messages to restore, start fresh
            warn!("No messages in checkpoint, starting fresh");
            return self.run(task).await;
        }

        info!("Restored {} messages from checkpoint", messages.len());

        // Continue from where we left off
        self.run_with_messages(task, messages, checkpoint.sequence)
            .await
    }

    fn create_checkpoint(
        &self,
        manager: &mut CheckpointManager,
        messages: &[Message],
        description: &str,
    ) -> Checkpoint {
        let mut checkpoint = manager.create_checkpoint();

        // Convert messages to checkpoint format
        for msg in messages {
            let role = match msg.role {
                Role::User => "user",
                Role::Assistant => "assistant",
                Role::System => "system",
            };
            checkpoint.messages.push(ConversationMessage::with_content(
                role,
                serde_json::to_value(&msg.content).unwrap_or_default(),
            ));
        }

        checkpoint.description = Some(description.to_string());
        manager.save(checkpoint.clone());
        checkpoint
    }
}

/// Builder for creating agents.
pub struct AgentBuilder<P: LlmProvider> {
    provider: P,
    config: AgentConfig,
    tool_registry: Option<ToolRegistry>,
    callback: Option<Arc<dyn AgentCallback>>,
}

impl<P: LlmProvider> AgentBuilder<P> {
    /// Create a new agent builder.
    pub fn new(provider: P) -> Self {
        Self {
            provider,
            config: AgentConfig::default(),
            tool_registry: None,
            callback: None,
        }
    }

    /// Set the configuration.
    pub fn config(mut self, config: AgentConfig) -> Self {
        self.config = config;
        self
    }

    /// Set the model.
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.config.model = model.into();
        self
    }

    /// Set max iterations.
    pub fn max_iterations(mut self, max: u32) -> Self {
        self.config.max_iterations = max;
        self
    }

    /// Set the tool registry.
    pub fn tools(mut self, registry: ToolRegistry) -> Self {
        self.tool_registry = Some(registry);
        self
    }

    /// Set the callback.
    pub fn callback(mut self, callback: Arc<dyn AgentCallback>) -> Self {
        self.callback = Some(callback);
        self
    }

    /// Build the agent.
    pub fn build(self) -> Agent<P> {
        let registry = self.tool_registry.unwrap_or_default();
        let mut agent = Agent::new(self.provider, self.config, registry);
        if let Some(callback) = self.callback {
            agent = agent.with_callback(callback);
        }
        agent
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mimir_tasks::Task;

    #[test]
    fn test_agent_config() {
        let config = AgentConfig::new("test-model")
            .with_max_iterations(10)
            .with_auto_approve(true);

        assert_eq!(config.model, "test-model");
        assert_eq!(config.max_iterations, 10);
        assert!(config.auto_approve);
    }

    #[test]
    fn test_task_creation() {
        let task = Task::new("Test task");
        assert_eq!(task.goal, "Test task");
    }
}
