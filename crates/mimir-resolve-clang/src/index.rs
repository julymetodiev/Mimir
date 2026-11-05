//! Writing an index: `clangd-indexer` over a database, and what it says of the project as a SCIP file, so
//! that all that reads and keeps an index stays as it is.

use std::fs::{self, File};
use std::io::BufReader;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use mimir_resolve_scip::{IndexFiles, ScipError, wait_for};

use mimir_resolve::ProjectFile;
use tracing::warn;

use crate::{clangd, dex, scip, tools};

/// One that takes longer keeps syntax rules.
/// LLVM's `llvm` folder alone took 665 s on 8 cores.
const TIMEOUT: Duration = Duration::from_secs(3600);
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
) -> Result<(), ScipError> {
    tools::check_clangd_indexer();
    let output = paths.other(OUTPUT);
    run(database, &output, paths)?;
    let read = File::open(&output)
        .map_err(failed)
        .and_then(|file| dex::read(BufReader::new(file), root).map_err(failed));
    let removed = fs::remove_file(&output).map_err(failed);
    let mut index = read?;
    removed?;
    // Without `clangd` the index stays, with no use of a local symbol.
    if let Err(e) = clangd::fill_local_symbols(&mut index, root, database, files) {
        warn!("No uses of local symbols in the index of C and C++: {e}");
    }
    fs::write(&paths.index, scip::encode(&index, root)).map_err(failed)
}

fn run(database: &Path, output: &Path, paths: &IndexFiles) -> Result<(), ScipError> {
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
    wait_for(&mut child, Instant::now() + TIMEOUT, tools::CLANGD_INDEXER)
}
