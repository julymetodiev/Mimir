//! Status command implementation.

use crate::app::{App, find_project_root};
use anyhow::{Result, bail};
use mimir_snapshot::list_snapshots;
use std::collections::HashMap;
use std::path::PathBuf;

/// Execute the status command.
pub async fn execute() -> Result<()> {
    // Find project root
    let root = match find_project_root() {
        Some(r) => r,
        None => bail!("Not in a Mimir project. Run 'mimir init' first."),
    };

    let mut app = App::with_root(&root)?;

    // Extract paths before borrowing mutably
    let objects_dir = app.config.objects_dir.clone();
    let snapshots_dir = app.config.snapshots_dir.clone();
    let is_initialized = app.config.is_initialized();

    if !is_initialized {
        bail!("Mimir is not initialized. Run 'mimir init' first.");
    }

    // Get project name from directory
    let project_name = root
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "Unknown".to_string());

    println!("Project: {}", project_name);
    println!("Root: {}", root.display());
    println!();

    // Initialize storage to read index
    let handle = app.init_storage().await?;
    let index = handle.index();

    // Count files by language
    let mut language_counts: HashMap<String, usize> = HashMap::new();
    for entry in index.iter() {
        let lang_name = format!("{:?}", entry.language);
        *language_counts.entry(lang_name).or_insert(0) += 1;
    }

    // Files section
    println!("Files:");
    println!("  Indexed: {}", index.len());

    if !language_counts.is_empty() {
        let mut sorted: Vec<_> = language_counts.into_iter().collect();
        sorted.sort_by(|a, b| b.1.cmp(&a.1));
        let langs: Vec<_> = sorted
            .iter()
            .take(5)
            .map(|(lang, count)| format!("{} ({})", lang, count))
            .collect();
        println!("  Languages: {}", langs.join(", "));
    }
    println!();

    // Storage section
    println!("Storage:");

    // Count objects
    let objects_count = count_objects(&objects_dir).await;
    let objects_size = dir_size(&objects_dir).await;
    println!(
        "  Objects: {} ({} files)",
        format_bytes(objects_size),
        objects_count
    );

    // Count snapshots
    let snapshots = list_snapshots(&snapshots_dir).await.unwrap_or_default();
    println!("  Snapshots: {}", snapshots.len());
    println!();

    // Last indexed time (from most recent file modification)
    if let Some(latest) = index.iter().map(|e| e.modified_at).max() {
        let now = chrono::Utc::now();
        let duration = now.signed_duration_since(latest);
        let ago = format_duration(duration);
        println!("Last indexed: {}", ago);
    }

    Ok(())
}

async fn count_objects(path: &PathBuf) -> usize {
    let mut count = 0;
    if let Ok(mut entries) = tokio::fs::read_dir(path).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            if entry.path().is_dir()
                && let Ok(mut sub_entries) = tokio::fs::read_dir(entry.path()).await
            {
                while let Ok(Some(_)) = sub_entries.next_entry().await {
                    count += 1;
                }
            }
        }
    }
    count
}

async fn dir_size(path: &PathBuf) -> u64 {
    let mut size = 0;
    if let Ok(mut entries) = tokio::fs::read_dir(path).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            if let Ok(metadata) = entry.metadata().await {
                if metadata.is_file() {
                    size += metadata.len();
                } else if metadata.is_dir() {
                    size += Box::pin(dir_size(&entry.path().to_path_buf())).await;
                }
            }
        }
    }
    size
}

fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} bytes", bytes)
    }
}

fn format_duration(duration: chrono::Duration) -> String {
    let seconds = duration.num_seconds();

    if seconds < 60 {
        "just now".to_string()
    } else if seconds < 3600 {
        let minutes = seconds / 60;
        if minutes == 1 {
            "1 minute ago".to_string()
        } else {
            format!("{} minutes ago", minutes)
        }
    } else if seconds < 86400 {
        let hours = seconds / 3600;
        if hours == 1 {
            "1 hour ago".to_string()
        } else {
            format!("{} hours ago", hours)
        }
    } else {
        let days = seconds / 86400;
        if days == 1 {
            "1 day ago".to_string()
        } else {
            format!("{} days ago", days)
        }
    }
}
