//! Whether a shell command reads the files of the project it runs in, told from its words alone.

use crate::project_of;
use std::path::{Path, PathBuf};

/// Commands that search, read or edit files, when they begin a part of a command.
const READERS: [&str; 16] = [
    "grep", "egrep", "fgrep", "rg", "ag", "ack", "find", "fd", "sed", "awk", "gawk", "cat", "bat",
    "head", "tail", "less",
];

/// Words before the program a command runs.
const WRAPPERS: [&str; 9] = [
    "{", "(", "!", "time", "command", "env", "nohup", "sudo", "timeout",
];

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
pub fn reads_project(command: &str, cwd: &Path) -> bool {
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
                // Where `$X` or `~` leads only the shell can say: nowhere known to be the project.
                directory = if target.starts_with(['$', '~']) {
                    PathBuf::new()
                } else {
                    plain(&directory.join(target))
                };
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn what_reads_the_project_and_what_does_not() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        let cwd = dir.path();
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
        assert!(!reads("cd $SITE && grep -rn x ."));
        assert!(!reads("cd ~/other && cat a.rs"));
        assert!(reads(&format!("cd $SITE && cat {}/a.rs", cwd.display())));
        // Not a read.
        assert!(!reads("cat <<EOF\ngrep x a.rs\nEOF"));
        assert!(!reads("git commit -m 'grep x a.rs; cat b'"));
        assert!(!reads("cargo test"));
    }
}
