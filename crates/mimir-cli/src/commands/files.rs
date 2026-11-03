//! Files and cat commands: the project's files as they are on disk.

use anyhow::Result;
use mimir_client::Client;
use std::path::Path;

/// List the project's files, only those matching a glob if given.
pub async fn list(glob: Option<String>, limit: Option<u32>, json: bool) -> Result<()> {
    let mut client = Client::open(&std::env::current_dir()?).await?;
    let found = client.files(glob, limit).await?;
    if json {
        println!("{}", serde_json::to_string_pretty(&found)?);
        return Ok(());
    }
    for file in &found.files {
        println!("{}", file.display());
    }
    if found.files.len() < found.total {
        println!("Showing {} of {} files", found.files.len(), found.total);
    }
    Ok(())
}

/// Show lines of a file of the project, with their numbers.
pub async fn cat(file: &Path, first: Option<u32>, last: Option<u32>, json: bool) -> Result<()> {
    let mut client = Client::open(&std::env::current_dir()?).await?;
    let text = client
        .read_file(std::path::absolute(file)?, first, last)
        .await?;
    if json {
        println!("{}", serde_json::to_string_pretty(&text)?);
        return Ok(());
    }
    for (offset, line) in text.lines.iter().enumerate() {
        println!("{:5}│ {line}", text.first_line as usize + offset);
    }
    let shown = text.lines.len() as u32;
    if shown < text.total_lines {
        println!("Showing {shown} of {} lines", text.total_lines);
    }
    Ok(())
}
