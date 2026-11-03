//! Init command: make the current directory a project, when no `.git` already says so.

use anyhow::Result;

/// Execute the init command.
pub fn execute() -> Result<()> {
    let dir = mimir_client::init(&std::env::current_dir()?)?;
    println!("Initialized {}", dir.display());
    Ok(())
}
