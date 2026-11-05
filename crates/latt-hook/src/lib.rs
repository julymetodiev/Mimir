//! What Lantern Lattice answers to a coding agent's hook, in the form Claude Code reads.
//!
//! The first search or read of a project in a session is refused once, with what to use instead,
//! unless the session has used Lantern Lattice already. The same call then goes through, so an agent is never
//! stuck. A command that reads the project's files through the shell is reminded of it once, and
//! once the session has used Lantern Lattice, which shows that its tools work, it is refused every time: the
//! Read tool stays for what they do not show. It reads no graph and starts no daemon: an agent waits
//! for it before every such tool.

use serde_json::{Value, json};
use std::fs::{self, OpenOptions};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use thiserror::Error;

/// How long a session is remembered.
const SESSION_KEPT: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// Read tools can show these, and Lantern Lattice's cannot.
const NOT_TEXT: [&str; 7] = ["png", "jpg", "jpeg", "gif", "webp", "pdf", "ipynb"];

/// Commands that search, read or edit files, when they begin a part of a command.
const READERS: [&str; 16] = [
    "grep", "egrep", "fgrep", "rg", "ag", "ack", "find", "fd", "sed", "awk", "gawk", "cat", "bat",
    "head", "tail", "less",
];

/// Words before the program a command runs.
const WRAPPERS: [&str; 9] = [
    "{", "(", "!", "time", "command", "env", "nohup", "sudo", "timeout",
];

const REFUSAL: &str = "This project has Lantern Lattice. Use its tools here: grep instead of Grep, files \
instead of Glob, read_file or read instead of Read, and trace before changing a function. They \
answer from the compiler's call graph and say which symbol each match is in. This is said once a \
session: if you have no such tools or need this one, repeat the same call and it goes through.";

const REMINDER: &str = "This project has Lantern Lattice: its grep, files and read_file tools do this \
without the shell, and say which symbol each match is in.";

const SHELL_REFUSAL: &str = "This reads the project's files through the shell. Use Lantern Lattice's tools: \
grep to search, files to find files, read_file or read to read, replace_text or edit_symbols to \
change. Pipe filters, builds and tests are not refused. For a file Lantern Lattice does not show, use the \
Read tool.";

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
        "Bash" if input("command").is_some_and(|command| reads_project(command, cwd)) => {
            if state.join(format!("{session}.latt")).exists() {
                Some(refusal(SHELL_REFUSAL))
            } else if remember(state, &format!("{session}.shell"))? {
                Some(reminder())
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

fn reminder() -> String {
    json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "additionalContext": REMINDER,
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

/// A part of a command: its words, and whether it reads what a pipe gives it.
struct Part {
    words: Vec<String>,
    after_pipe: bool,
}

/// The parts of a command, with quotes kept in one word and a `|` or `;` inside them no separator.
/// What follows `<<` is a text and no command, so it ends here.
fn parts_of(command: &str) -> Vec<Part> {
    let mut parts = Vec::new();
    let mut words: Vec<String> = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut after_pipe = false;
    let mut chars = command.chars().peekable();
    while let Some(c) = chars.next() {
        if let Some(open) = quote {
            if c == open {
                quote = None;
            } else {
                word.push(c);
            }
            continue;
        }
        let mut separator = None;
        match c {
            '\'' | '"' => quote = Some(c),
            '\\' => word.extend(chars.next()),
            ' ' | '\t' => {}
            '<' if chars.peek() == Some(&'<') => {
                words.push(std::mem::take(&mut word));
                words.push("<<".to_string());
                break;
            }
            // `2>&1` and `&>` are redirections.
            '&' if word.ends_with('>') || chars.peek() == Some(&'>') => word.push(c),
            '|' if chars.peek() == Some(&'|') => {
                chars.next();
                separator = Some(false);
            }
            '|' => separator = Some(true),
            ';' | '\n' | '&' => {
                if c == '&' && chars.peek() == Some(&'&') {
                    chars.next();
                }
                separator = Some(false);
            }
            _ => word.push(c),
        }
        if (matches!(c, ' ' | '\t') || separator.is_some()) && !word.is_empty() {
            words.push(std::mem::take(&mut word));
        }
        if let Some(pipe) = separator {
            parts.push(Part {
                words: std::mem::take(&mut words),
                after_pipe,
            });
            after_pipe = pipe;
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    parts.push(Part { words, after_pipe });
    parts
}

/// Whether a command searches, reads or edits a file of the git repository it runs in. What comes
/// after a pipe filters the output of something else. A path outside the project, or one that only
/// the shell can say, is not refused: what is not known to be the project's must not stop an agent.
fn reads_project(command: &str, cwd: &Path) -> bool {
    let Some(project) = project_of(cwd) else {
        return false;
    };
    let mut directory = plain(cwd);
    for part in parts_of(command) {
        let mut words = part.words.as_slice();
        while let Some((first, rest)) = words.split_first() {
            let assignment = first
                .split_once('=')
                .is_some_and(|(name, _)| !name.is_empty() && !name.contains('/'));
            let number = first.starts_with(|c: char| c.is_ascii_digit());
            if !WRAPPERS.contains(&first.as_str()) && !assignment && !number {
                break;
            }
            words = rest;
        }
        let Some((first, arguments)) = words.split_first() else {
            continue;
        };
        if first == "cd" {
            if let Some(target) = arguments.first() {
                directory = plain(&directory.join(target));
            }
            continue;
        }
        let program = first.rsplit('/').next().unwrap_or(first);
        if part.after_pipe
            || !READERS.contains(&program)
            || arguments.iter().any(|word| word == "<<")
        {
            continue;
        }
        let paths = paths_in(arguments);
        let elsewhere = paths.iter().any(|path| {
            path.starts_with(['$', '~']) || !plain(&directory.join(path)).starts_with(project)
        });
        let named_here = paths
            .iter()
            .any(|path| path.starts_with('/') && Path::new(path).starts_with(project));
        if named_here || (directory.starts_with(project) && !elsewhere) {
            return true;
        }
    }
    false
}

/// The words of a command that may be paths: all but its options and where its output goes. A
/// relative one says nothing, as it may be a pattern, so only those that can lead elsewhere count.
fn paths_in(arguments: &[String]) -> Vec<&str> {
    let mut found = Vec::new();
    let mut words = arguments.iter();
    while let Some(word) = words.next() {
        if word.ends_with('>') {
            words.next();
        } else if !word.starts_with('-')
            && !word.contains('>')
            && word.starts_with(['/', '$', '~', '.'])
        {
            found.push(word.as_str());
        }
    }
    found
}

/// A path without `.` and `..`, as far as it can be told without the disk.
fn plain(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                result.pop();
            }
            other => result.push(other),
        }
    }
    result
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
    fn a_search_through_the_shell_is_reminded_once_and_never_refused() {
        let project = Project::new();
        let search = json!({"command": "cargo fmt && grep -n x a.rs | head"});
        let first = project.ask("Bash", search.clone()).unwrap();
        assert_eq!(decision(&first), "");
        assert!(
            first["hookSpecificOutput"]["additionalContext"]
                .as_str()
                .unwrap()
                .contains("Lantern Lattice")
        );
        assert!(project.ask("Bash", search).is_none());
        assert!(
            project
                .ask("Bash", json!({"command": "cargo test"}))
                .is_none()
        );
    }

    #[test]
    fn what_reads_the_project_and_what_does_not() {
        let project = Project::new();
        let cwd = project.dir.path();
        let reads = |command: &str| reads_project(command, cwd);
        // What filters an output is no search.
        assert!(!reads("gh api x | cut -c1-90 | head -5"));
        assert!(!reads("cargo test 2>&1 | grep FAILED"));
        assert!(reads("cargo test || cat out.log"));
        assert!(reads("rg x | head"));
        assert!(reads("cd src && sed -n 1,5p a.rs"));
        assert!(reads("FOO=1 time /usr/bin/grep x a.rs"));
        assert!(reads("timeout 10 find . -name x"));
        assert!(reads("{ echo start; grep x a.rs; }"));
        assert!(reads(&format!(
            "cd /elsewhere && cat {}/a.rs",
            cwd.display()
        )));
        // A pipe in quotes separates nothing.
        assert!(!reads("grep -E 'a|b' /elsewhere/log"));
        assert!(reads("grep -E 'a;b' a.rs"));
        // Elsewhere, or only the shell knows.
        assert!(!reads("grep FAILED /elsewhere/out.txt"));
        assert!(!reads("tail -4 $LOG/out.txt"));
        assert!(!reads("cat ../other/a.rs"));
        assert!(!reads("cd /elsewhere && grep x a.rs"));
        // Not a read.
        assert!(!reads("cat <<EOF\ngrep x a.rs\nEOF"));
        assert!(!reads("git commit -m 'grep x a.rs; cat b'"));
        assert!(!reads("cargo test"));
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
