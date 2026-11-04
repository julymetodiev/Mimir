//! What Mimir answers to a coding agent's hook, in the form Claude Code reads.
//!
//! The first search or read of a project in a session is refused once, with what to use instead,
//! unless the session has used Mimir already. The same call then goes through, so an agent is never
//! stuck. A search through the shell is only reminded of it, once, since a command is not always a
//! search. It reads no graph and starts no daemon: an agent waits for it before every such tool.

use serde_json::{Value, json};
use std::fs::{self, OpenOptions};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use thiserror::Error;

/// How long a session is remembered.
const SESSION_KEPT: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// Read tools can show these, and Mimir's cannot.
const NOT_TEXT: [&str; 7] = ["png", "jpg", "jpeg", "gif", "webp", "pdf", "ipynb"];

/// Commands that search or read, when they begin a line of a command.
const SEARCHES: [&str; 9] = [
    "grep", "rg", "ag", "find", "sed", "awk", "cat", "head", "tail",
];

const REFUSAL: &str = "This project has Mimir. Use its tools here: grep instead of Grep, files \
instead of Glob, read_file or read instead of Read, and trace before changing a function. They \
answer from the compiler's call graph and say which symbol each match is in. This is said once a \
session: if you have no such tools or need this one, repeat the same call and it goes through.";

const REMINDER: &str = "This project has Mimir: its grep, files and read_file tools do this \
without the shell, and say which symbol each match is in.";

/// What can go wrong remembering a session.
#[derive(Error, Debug)]
pub enum HookError {
    #[error("Cannot use {path}: {source}")]
    State {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// The answer to one event, if there is any to give. An event it cannot read gets none: a hook must
/// not get in the way of what the agent is doing.
pub fn answer(event: &str, state: &Path) -> Result<Option<String>, HookError> {
    let Ok(event) = serde_json::from_str::<Value>(event) else {
        return Ok(None);
    };
    let text = |key: &str| event.get(key).and_then(Value::as_str).unwrap_or("");
    let session = text("session_id");
    if session.is_empty()
        || !session
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    {
        return Ok(None);
    }
    let cwd = Path::new(text("cwd"));
    let input = |key: &str| {
        event
            .get("tool_input")
            .and_then(|input| input.get(key))
            .and_then(Value::as_str)
    };
    let tool = text("tool_name");
    if tool.starts_with("mcp__") {
        remember(state, session)?;
        return Ok(None);
    }
    Ok(match tool {
        "Read" | "Grep" | "Glob"
            if is_of_a_project(input("file_path").or_else(|| input("path")), cwd)
                && remember(state, session)? =>
        {
            Some(refusal())
        }
        "Bash"
            if input("command").is_some_and(searches)
                && project_of(cwd).is_some()
                && remember(state, &format!("{session}.shell"))? =>
        {
            Some(reminder())
        }
        _ => None,
    })
}

fn refusal() -> String {
    json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "deny",
            "permissionDecisionReason": REFUSAL,
        }
    })
    .to_string()
}

fn reminder() -> String {
    json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "additionalContext": REMINDER,
        }
    })
    .to_string()
}

/// Whether a target lies in the git repository the agent works in, and Mimir's tools can show it.
fn is_of_a_project(target: Option<&str>, cwd: &Path) -> bool {
    let Some(project) = project_of(cwd) else {
        return false;
    };
    let target = target.map_or_else(|| cwd.to_path_buf(), |target| cwd.join(target));
    let shown = target
        .extension()
        .and_then(|extension| extension.to_str())
        .is_none_or(|extension| !NOT_TEXT.contains(&extension.to_ascii_lowercase().as_str()));
    shown && target.starts_with(project)
}

/// The git repository a directory lies in.
fn project_of(directory: &Path) -> Option<&Path> {
    directory
        .ancestors()
        .find(|ancestor| ancestor.join(".git").exists())
}

/// Whether a command begins any of its parts with a search.
fn searches(command: &str) -> bool {
    command
        .split(['\n', ';', '|', '&'])
        .filter_map(|part| part.split_whitespace().next())
        .any(|word| SEARCHES.contains(&word))
}

/// Note a mark, saying whether it is new. Old marks go when a new one comes, so they do not pile up.
fn remember(state: &Path, mark: &str) -> Result<bool, HookError> {
    let failed = |path: &Path| {
        let path = path.to_path_buf();
        move |source| HookError::State { path, source }
    };
    fs::create_dir_all(state).map_err(failed(state))?;
    let path = state.join(mark);
    match OpenOptions::new().write(true).create_new(true).open(&path) {
        Ok(_) => {}
        Err(source) if source.kind() == ErrorKind::AlreadyExists => return Ok(false),
        Err(source) => return Err(HookError::State { path, source }),
    }
    for entry in fs::read_dir(state).map_err(failed(state))?.flatten() {
        let old = entry
            .metadata()
            .and_then(|found| found.modified())
            .is_ok_and(|modified| {
                SystemTime::now()
                    .duration_since(modified)
                    .is_ok_and(|age| age > SESSION_KEPT)
            });
        if old {
            let entry = entry.path();
            fs::remove_file(&entry).map_err(failed(&entry))?;
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    struct Project {
        dir: TempDir,
    }

    impl Project {
        fn new() -> Self {
            let dir = TempDir::new().unwrap();
            fs::create_dir(dir.path().join(".git")).unwrap();
            Self { dir }
        }

        fn state(&self) -> PathBuf {
            self.dir.path().join(".state")
        }

        fn ask(&self, tool: &str, input: Value) -> Option<Value> {
            let event = json!({
                "session_id": "one",
                "cwd": self.dir.path(),
                "tool_name": tool,
                "tool_input": input,
            });
            answer(&event.to_string(), &self.state())
                .unwrap()
                .map(|answer| serde_json::from_str(&answer).unwrap())
        }
    }

    fn decision(answer: &Value) -> &str {
        answer["hookSpecificOutput"]["permissionDecision"]
            .as_str()
            .unwrap_or("")
    }

    #[test]
    fn the_first_read_of_a_session_is_refused_and_then_goes_through() {
        let project = Project::new();
        let input = json!({"file_path": "src/lib.rs"});
        let first = project.ask("Read", input.clone()).unwrap();
        assert_eq!(decision(&first), "deny");
        assert!(project.ask("Read", input.clone()).is_none());
        assert!(project.ask("Grep", json!({"pattern": "x"})).is_none());
    }

    #[test]
    fn a_session_that_used_mimir_is_never_refused() {
        let project = Project::new();
        assert!(
            project
                .ask("mcp__plugin_mimir_mimir__grep", json!({}))
                .is_none()
        );
        assert!(project.ask("Read", json!({"file_path": "a.rs"})).is_none());
    }

    #[test]
    fn what_mimir_cannot_show_or_lies_outside_the_project_is_not_refused() {
        let project = Project::new();
        assert!(project.ask("Read", json!({"file_path": "a.PNG"})).is_none());
        assert!(project.ask("Read", json!({"file_path": "b.pdf"})).is_none());
        assert!(
            project
                .ask("Read", json!({"file_path": "/elsewhere/a.rs"}))
                .is_none()
        );
        let refused = project.ask("Glob", json!({"pattern": "*.rs"})).unwrap();
        assert_eq!(decision(&refused), "deny");
    }

    #[test]
    fn outside_a_git_repository_nothing_is_refused() {
        let dir = TempDir::new().unwrap();
        let event = json!({
            "session_id": "one", "cwd": dir.path(), "tool_name": "Read",
            "tool_input": {"file_path": "a.rs"},
        });
        let state = dir.path().join(".state");
        assert!(answer(&event.to_string(), &state).unwrap().is_none());
    }

    #[test]
    fn a_search_through_the_shell_is_reminded_once_and_never_refused() {
        let project = Project::new();
        let search = json!({"command": "cargo fmt && grep -n x a.rs | head"});
        let first = project.ask("Bash", search.clone()).unwrap();
        assert_eq!(decision(&first), "");
        assert!(
            first["hookSpecificOutput"]["additionalContext"]
                .as_str()
                .unwrap()
                .contains("Mimir")
        );
        assert!(project.ask("Bash", search).is_none());
        assert!(
            project
                .ask("Bash", json!({"command": "cargo test"}))
                .is_none()
        );
    }

    #[test]
    fn an_event_it_cannot_read_gets_no_answer() {
        let project = Project::new();
        assert!(answer("not json", &project.state()).unwrap().is_none());
        let strange =
            json!({"session_id": "../up", "cwd": project.dir.path(), "tool_name": "Read"});
        assert!(
            answer(&strange.to_string(), &project.state())
                .unwrap()
                .is_none()
        );
    }
}
