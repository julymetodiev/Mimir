//! Init command implementation.

use crate::app::App;
use anyhow::{Result, bail};

/// Execute the init command.
pub async fn execute(force: bool) -> Result<()> {
    let app = App::new()?;
    let config = &app.config;

    if config.is_initialized() && !force {
        bail!(
            "Mimir is already initialized in this directory.\n\
             Use --force to re-initialize."
        );
    }

    println!("Initializing Mimir in {}...", config.root.display());
    config.create_directories().await?;
    println!(
        "Created {}/ and {}/",
        config.agent_dir.display(),
        config.graph_dir.display()
    );
    println!("Mimir initialized. Run 'mimir sync' to build the code graph.");

    Ok(())
}
