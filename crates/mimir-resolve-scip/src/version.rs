//! Whether a tool is the version it was tested with.

use std::process::{Command, Stdio};

use tracing::warn;

/// Say so in the log if a program is not the tested version. Another one still runs.
pub fn check_version(program: &str, args: &[&str], tested: &str) {
    let output = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .output();
    if let Ok(output) = output
        && !says_version(&String::from_utf8_lossy(&output.stdout), tested)
        && !says_version(&String::from_utf8_lossy(&output.stderr), tested)
    {
        warn!("{program} is not the tested version, {tested}");
    }
}

/// Whether the text has this version, and not one that starts with it or ends with it.
fn says_version(output: &str, version: &str) -> bool {
    output.match_indices(version).any(|(at, _)| {
        let is_part = |c: char| c.is_ascii_digit() || c == '.';
        !output[at + version.len()..].starts_with(is_part) && !output[..at].ends_with(is_part)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_version_is_told_from_one_that_holds_it() {
        assert!(says_version("golang.org/x/tools/gopls v0.24.0\n", "0.24.0"));
        assert!(says_version("scip-go version 0.2.9", "0.2.9"));
        assert!(!says_version("gopls v0.24.01", "0.24.0"));
        assert!(!says_version("gopls v10.24.0", "0.24.0"));
        assert!(!says_version("gopls v0.24.0.1", "0.24.0"));
    }
}
