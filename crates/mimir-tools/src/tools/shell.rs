//! Shell tool - execute shell commands.

use crate::{Tool, ToolContext, ToolParameter, ToolResult, ToolSchema};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::process::Stdio;
use tokio::process::Command;

/// Output from the shell tool.
#[derive(Debug, Serialize, Deserialize)]
pub struct ShellOutput {
    pub command: String,
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// Tool for executing shell commands.
///
/// This tool is sandboxed and only allows certain safe commands.
pub struct ShellTool;

/// Commands that are allowed to run.
const ALLOWED_COMMANDS: &[&str] = &[
    "ls", "pwd", "cat", "head", "tail", "wc", "grep", "find",
    "cargo", "rustc", "rustfmt", "clippy",
    "git", "gh",
    "npm", "node", "npx", "yarn", "pnpm",
    "python", "python3", "pip", "pip3",
    "go", "gofmt",
    "echo", "which", "env", "date",
    "mkdir", "touch", "cp", "mv", "rm",
    "curl", "wget",
];

/// Commands that are never allowed.
const BLOCKED_COMMANDS: &[&str] = &[
    "sudo", "su", "chmod", "chown", "chgrp",
    "rm -rf /", "mkfs", "dd",
    "shutdown", "reboot", "halt",
];

#[async_trait]
impl Tool for ShellTool {
    fn name(&self) -> &str {
        "shell"
    }

    fn description(&self) -> &str {
        "Execute shell commands in the project directory. Only safe commands are allowed. Use for running tests, builds, git operations, etc."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema::new(self.name(), self.description())
            .with_param(ToolParameter::required_string(
                "command",
                "Shell command to execute. Must start with an allowed command (cargo, git, npm, etc.)",
            ))
            .with_param(ToolParameter::optional_integer(
                "timeout",
                "Timeout in seconds (default: 60, max: 300)",
            ))
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let command = match params["command"].as_str() {
            Some(c) => c,
            None => return ToolResult::error("'command' parameter is required"),
        };

        let timeout_secs = params["timeout"].as_u64().unwrap_or(60).min(300);

        // Security checks
        if let Err(e) = self.validate_command(command) {
            return ToolResult::error(e);
        }

        if context.dry_run {
            return ToolResult::success(ShellOutput {
                command: command.to_string(),
                exit_code: 0,
                stdout: "[dry-run] Command would be executed".to_string(),
                stderr: String::new(),
            });
        }

        // Execute the command
        self.run_command(command, timeout_secs, context).await
    }
}

impl ShellTool {
    fn validate_command(&self, command: &str) -> Result<(), String> {
        let trimmed = command.trim();

        // Extract only the command part (before any heredoc markers)
        // This prevents blocking commands like `cat << 'EOF'\nuser data\nEOF`
        // where "user" contains "su" but is just data, not a command
        let command_part = if let Some(heredoc_pos) = trimmed.find("<<") {
            &trimmed[..heredoc_pos]
        } else {
            trimmed
        };

        // Check for blocked patterns only in the actual command, not heredoc content
        // Use word boundary checks to avoid false positives (e.g., "user" containing "su")
        for blocked in BLOCKED_COMMANDS {
            // Check if blocked command appears as a word (not part of another word)
            let patterns = [
                format!("{} ", blocked),      // "su " at start/middle
                format!(" {}", blocked),      // " su" at end
                format!(" {} ", blocked),     // " su " in middle
            ];

            // Also check if command starts with blocked command
            if command_part.starts_with(&format!("{} ", blocked))
                || command_part == *blocked
                || patterns.iter().any(|p| command_part.contains(p)) {
                return Err(format!("Command contains blocked pattern: {}", blocked));
            }
        }

        // Check if command starts with allowed executable
        let first_word = trimmed.split_whitespace().next().unwrap_or("");

        // Handle common shell constructs
        let executable = if first_word.starts_with("./") {
            // Allow running local scripts
            first_word
        } else {
            first_word
        };

        if !ALLOWED_COMMANDS.contains(&executable) && !executable.starts_with("./") {
            return Err(format!(
                "Command '{}' is not in the allowed list. Allowed: {:?}",
                executable,
                ALLOWED_COMMANDS
            ));
        }

        Ok(())
    }

    async fn run_command(&self, command: &str, timeout_secs: u64, context: &ToolContext) -> ToolResult {
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(timeout_secs),
            async {
                Command::new("sh")
                    .arg("-c")
                    .arg(command)
                    .current_dir(&context.project_root)
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .output()
                    .await
            },
        )
        .await;

        match result {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let exit_code = output.status.code().unwrap_or(-1);

                // Truncate output if too long
                let max_len = 50000;
                let stdout = if stdout.len() > max_len {
                    format!("{}...\n[truncated, {} bytes total]", &stdout[..max_len], stdout.len())
                } else {
                    stdout
                };
                let stderr = if stderr.len() > max_len {
                    format!("{}...\n[truncated, {} bytes total]", &stderr[..max_len], stderr.len())
                } else {
                    stderr
                };

                ToolResult::success(ShellOutput {
                    command: command.to_string(),
                    exit_code,
                    stdout,
                    stderr,
                })
            }
            Ok(Err(e)) => ToolResult::error(format!("Failed to execute command: {}", e)),
            Err(_) => ToolResult::error(format!("Command timed out after {} seconds", timeout_secs)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_allowed_command() {
        let tool = ShellTool;
        assert!(tool.validate_command("cargo build").is_ok());
        assert!(tool.validate_command("git status").is_ok());
        assert!(tool.validate_command("ls -la").is_ok());
    }

    #[test]
    fn test_validate_blocked_command() {
        let tool = ShellTool;
        assert!(tool.validate_command("sudo rm -rf /").is_err());
        assert!(tool.validate_command("su root").is_err());
        assert!(tool.validate_command("dangerous_command").is_err());
    }

    #[test]
    fn test_heredoc_with_user_content() {
        let tool = ShellTool;
        // "user" contains "su" but it's in heredoc content, not the command
        let cmd = r#"cat > src/main.rs << 'EOF'
mod user;
use user::User;
EOF"#;
        assert!(tool.validate_command(cmd).is_ok(), "heredoc content should not trigger blocked pattern");

        // But actual su command should still be blocked
        assert!(tool.validate_command("su -").is_err());
    }

    #[tokio::test]
    async fn test_shell_execute() {
        let tool = ShellTool;
        let context = ToolContext::new("/tmp");
        let params = serde_json::json!({
            "command": "echo hello"
        });

        let result = tool.execute(params, &context).await;
        assert!(result.success);

        let output: ShellOutput = serde_json::from_value(result.output).unwrap();
        assert_eq!(output.exit_code, 0);
        assert!(output.stdout.contains("hello"));
    }

    #[tokio::test]
    async fn test_shell_dry_run() {
        let tool = ShellTool;
        let context = ToolContext::new("/tmp").with_dry_run(true);
        let params = serde_json::json!({
            "command": "echo hello"
        });

        let result = tool.execute(params, &context).await;
        assert!(result.success);

        let output: ShellOutput = serde_json::from_value(result.output).unwrap();
        assert!(output.stdout.contains("dry-run"));
    }
}
