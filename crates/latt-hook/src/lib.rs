//! What Lantern Lattice answers to a coding agent's hook, in the form Claude Code reads.
//!
//! The first search or read of a project in a session is refused once, with what to use instead,
//! unless the session has used Lantern Lattice already. The same call then goes through, so an agent is never
//! stuck. The first edit of a project's file with Edit or Write is refused once too, and so is the
//! first command that reads the project's files through the shell. Once the session has used Lantern
//! Lattice, which shows that its tools work, such a command is refused every time: the Read tool stays
//! for what they do not show. It reads no graph and starts no daemon: an agent waits for it before
//! every such tool.

mod shell;

use serde_json::{Value, json};
use shell::reads_project;
use std::fs::{self, OpenOptions};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use thiserror::Error;

/// How long a session is remembered.
const SESSION_KEPT: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// Read tools can show these, and Lantern Lattice's cannot.
const NOT_TEXT: [&str; 7] = ["png", "jpg", "jpeg", "gif", "webp", "pdf", "ipynb"];

const REFUSAL: &str = "This project has Lantern Lattice. Use its tools here: grep instead of Grep, files \
instead of Glob, read_file or read instead of Read, and trace before changing a function. They \
answer from the compiler's call graph and say which symbol each match is in. This is said once a \
session: if you have no such tools or need this one, repeat the same call and it goes through.";

const EDIT_REFUSAL: &str = "This project has Lantern Lattice. Change its files with its tools: \
replace_text for a small change, edit_symbols to replace a symbol, insert_after to add code, \
edit_lines for a range of lines, write_file to create a file. They keep the call graph up to date \
as you edit. This is said once a session: if you have no such tools or need this one, repeat the \
same call and it goes through.";

const SHELL_REFUSAL: &str = "This reads the project's files through the shell. Use Lantern Lattice's tools: \
grep to search, files to find files, read_file or read to read, replace_text or edit_symbols to \
change. Pipe filters, builds and tests are not refused. For a file Lantern Lattice does not show, use the \
Read tool.";

const FIRST_SHELL_REFUSAL: &str = "This reads the project's files through the shell. This project has \
Lantern Lattice: use its tools, grep to search, files to find files, read_file or read to read, \
replace_text or edit_symbols to change. Pipe filters, builds and tests are not refused. This is said \
once a session: if you have no such tools, repeat the same command and it goes through.";

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
        remember(state, &format!("{session}.latt"))?;
        return Ok(None);
    }
    Ok(match tool {
        "Read" | "Grep" | "Glob"
            if is_of_a_project(input("file_path").or_else(|| input("path")), cwd)
                && remember(state, session)? =>
        {
            Some(refusal(REFUSAL))
        }
        "Edit" | "Write"
            if input("file_path").is_some_and(|file| is_of_a_project(Some(file), cwd))
                && remember(state, &format!("{session}.edit"))? =>
        {
            Some(refusal(EDIT_REFUSAL))
        }
        "Bash" if input("command").is_some_and(|command| reads_project(command, cwd)) => {
            if state.join(format!("{session}.latt")).exists() {
                Some(refusal(SHELL_REFUSAL))
            } else if remember(state, &format!("{session}.shell"))? {
                Some(refusal(FIRST_SHELL_REFUSAL))
            } else {
                None
            }
        }
        _ => None,
    })
}

fn refusal(reason: &str) -> String {
    json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "deny",
            "permissionDecisionReason": reason,
        }
    })
    .to_string()
}

/// Whether a target lies in the git repository the agent works in, and Lantern Lattice's tools can show it.
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
    fn a_session_that_used_latt_is_never_refused() {
        let project = Project::new();
        assert!(
            project
                .ask("mcp__plugin_latt_latt__grep", json!({}))
                .is_none()
        );
        assert!(project.ask("Read", json!({"file_path": "a.rs"})).is_none());
    }

    #[test]
    fn what_latt_cannot_show_or_lies_outside_the_project_is_not_refused() {
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
    fn a_search_through_the_shell_is_refused_once_and_then_goes_through() {
        let project = Project::new();
        let search = json!({"command": "cargo fmt && grep -n x a.rs | head"});
        let first = project.ask("Bash", search.clone()).unwrap();
        assert_eq!(decision(&first), "deny");
        assert!(project.ask("Bash", search).is_none());
        assert!(
            project
                .ask("Bash", json!({"command": "cargo test"}))
                .is_none()
        );
    }

    #[test]
    fn the_first_edit_of_a_session_is_refused_and_then_goes_through() {
        let project = Project::new();
        let first = project
            .ask("Edit", json!({"file_path": "src/lib.rs"}))
            .unwrap();
        assert_eq!(decision(&first), "deny");
        assert!(
            project
                .ask("Edit", json!({"file_path": "src/lib.rs"}))
                .is_none()
        );
        assert!(
            project
                .ask("Write", json!({"file_path": "src/new.rs"}))
                .is_none()
        );
    }

    #[test]
    fn an_edit_is_refused_apart_from_reads_and_after_latt_was_used() {
        let project = Project::new();
        project.ask("mcp__plugin_latt_latt__grep", json!({}));
        assert!(project.ask("Read", json!({"file_path": "a.rs"})).is_none());
        let refused = project
            .ask("Write", json!({"file_path": "src/new.rs"}))
            .unwrap();
        assert_eq!(decision(&refused), "deny");
    }

    #[test]
    fn an_edit_latt_cannot_make_or_outside_the_project_is_not_refused() {
        let project = Project::new();
        assert!(
            project
                .ask("Write", json!({"file_path": "/elsewhere/a.rs"}))
                .is_none()
        );
        assert!(
            project
                .ask("Write", json!({"file_path": "logo.png"}))
                .is_none()
        );
        assert!(project.ask("Edit", json!({})).is_none());
    }

    #[test]
    fn once_latt_was_used_a_read_through_the_shell_is_always_refused() {
        let project = Project::new();
        let search = json!({"command": "grep -n x a.rs"});
        project.ask("mcp__plugin_latt_latt__grep", json!({}));
        for _ in 0..2 {
            let refused = project.ask("Bash", search.clone()).unwrap();
            assert_eq!(decision(&refused), "deny");
        }
        assert!(
            project
                .ask("Bash", json!({"command": "cargo test | grep x"}))
                .is_none()
        );
        assert!(project.ask("Read", json!({"file_path": "a.rs"})).is_none());
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
