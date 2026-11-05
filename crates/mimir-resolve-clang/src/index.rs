//! Writing an index: `clangd-indexer` over a database, and what it says of the project as a SCIP file, so
//! that all that reads and keeps an index stays as it is.

use std::fs::{self, File};
use std::io::BufReader;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use mimir_lsp::{LspError, Stopping};
use mimir_resolve_scip::{IndexFiles, ScipError, wait_for};

use mimir_resolve::ProjectFile;
use tracing::{debug, warn};

use crate::{clangd, dex, scip, tools};

/// One that takes longer keeps syntax rules.
/// LLVM's `llvm` folder alone took 665 s on 8 cores.
const TIMEOUT: Duration = Duration::from_secs(3600);
/// What the log of `clangd-indexer` says of each command that failed.
const FAILED_COMMAND: &str = "Failed to run action on ";
/// How the files of this index start in the cache directory.
pub const STEM_PREFIX: &str = "clangd";
const OUTPUT: &str = "yaml";

fn failed(e: impl std::fmt::Display) -> ScipError {
    ScipError::Failed(e.to_string())
}

/// Write the index of the project at this absolute path.
pub fn write(
    root: &Path,
    database: &Path,
    files: &[ProjectFile],
    paths: &IndexFiles,
    stopping: &Stopping,
) -> Result<(), ScipError> {
    tools::check_clangd_indexer();
    let output = paths.other(OUTPUT);
    if let Err(e) = run(database, &output, paths, stopping) {
        // It can be gigabytes, and what is in it is of no use.
        if let Err(gone) = fs::remove_file(&output) {
            debug!("No output of a failed index to remove: {gone}");
        }
        return Err(e);
    }
    let read = File::open(&output)
        .map_err(failed)
        .and_then(|file| dex::read(BufReader::new(file), root).map_err(failed));
    let removed = fs::remove_file(&output).map_err(failed);
    let mut index = read?;
    removed?;
    // Without `clangd` the index stays, with no use of a local symbol.
    match clangd::fill_local_symbols(&mut index, root, database, files, stopping) {
        Ok(()) => {}
        // Kept, this would pass for a whole index.
        Err(e @ LspError::Stopping) => return Err(failed(e)),
        Err(e) => warn!("No uses of local symbols in the index of C and C++: {e}"),
    }
    fs::write(&paths.index, scip::encode(&index, root)).map_err(failed)
}

/// What a person should know if commands of the database failed: an index can look whole with most
/// of a project missing.
pub fn failure_note(log: &Path, database: &Path) -> Option<String> {
    let failed = failed_commands(&fs::read_to_string(log).ok()?);
    if failed == 0 {
        return None;
    }
    let commands = fs::read(database)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Vec<serde_json::Value>>(&bytes).ok())
        .map_or_else(|| "?".to_string(), |commands| commands.len().to_string());
    Some(format!(
        "C and C++: {failed} of {commands} compile commands failed in {}, so what only they read may \
         have syntax rules. See {}.",
        tools::CLANGD_INDEXER,
        log.display()
    ))
}

fn failed_commands(log: &str) -> usize {
    log.lines()
        .filter(|line| line.starts_with(FAILED_COMMAND))
        .count()
}

fn run(
    database: &Path,
    output: &Path,
    paths: &IndexFiles,
    stopping: &Stopping,
) -> Result<(), ScipError> {
    let directory = database
        .parent()
        .ok_or_else(|| failed("a database has no directory"))?;
    let mut child = Command::new(tools::CLANGD_INDEXER)
        .args(["--executor=all-TUs", "--format=yaml"])
        .arg(database)
        .current_dir(directory)
        .stdin(Stdio::null())
        .stdout(File::create(output).map_err(failed)?)
        .stderr(File::create(&paths.log).map_err(failed)?)
        .spawn()
        .map_err(|e| {
            ScipError::ToolMissing(format!("cannot run {}: {e}", tools::CLANGD_INDEXER))
        })?;
    wait_for(
        &mut child,
        Instant::now() + TIMEOUT,
        tools::CLANGD_INDEXER,
        stopping,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_person_is_told_of_commands_that_failed() {
        let dir = tempfile::tempdir().unwrap();
        let (log, database) = (dir.path().join("log"), dir.path().join("db.json"));
        fs::write(&database, r#"[{"file":"a.c"},{"file":"b.c"}]"#).unwrap();

        fs::write(&log, "warning: something\n").unwrap();
        assert_eq!(failure_note(&log, &database), None);

        fs::write(&log, "error: no a.h\nFailed to run action on /p/a.c\n").unwrap();
        let note = failure_note(&log, &database).unwrap();
        assert!(note.contains("1 of 2 compile commands"), "{note}");
    }
}
