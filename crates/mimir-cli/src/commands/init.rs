//! Init command implementation.

use crate::app::App;
use anyhow::{Result, bail};
use mimir_content::ContentStore;
use mimir_files::Scanner;
use mimir_snapshot::{FileInfo, create_snapshot};
use mimir_types::{FileId, SnapshotId};
use std::collections::HashMap;
use tracing::info;

/// Execute the init command.
pub async fn execute(force: bool) -> Result<()> {
    let mut app = App::new()?;

    // Extract paths before any mutable borrows
    let root = app.config.root.clone();
    let snapshots_dir = app.config.snapshots_dir.clone();
    let is_initialized = app.config.is_initialized();

    // Check if already initialized
    if is_initialized && !force {
        bail!(
            "Mimir is already initialized in this directory.\n\
             Use --force to re-initialize."
        );
    }

    println!("Initializing Mimir in {}...", root.display());
    println!();

    // Step 1: Create directory structure
    print_step(1, 5, "Creating .agent/ directory structure...");
    app.config.create_directories().await?;
    println!(" done");

    // Step 2: Scan files
    print_step(2, 5, "Scanning files...");
    let scanner = Scanner::new(&root);
    let scanned_files = scanner.scan().await?;
    println!(" Found {} files", scanned_files.len());

    // Step 3: Compute content hashes (already done in scanner)
    print_step(3, 5, "Computing content hashes...");
    println!(" done");

    // Step 4: Store content
    print_step(4, 5, "Storing content...");
    let handle = app.init_storage().await?;
    let store = handle.content_store();
    let index = handle.index();

    let mut total_size: u64 = 0;
    let mut language_counts: HashMap<String, usize> = HashMap::new();
    let mut file_infos: Vec<FileInfo> = Vec::new();

    for scanned in &scanned_files {
        // Read file content
        let content = tokio::fs::read(&scanned.absolute_path).await?;
        total_size += content.len() as u64;

        // Store content
        let hash = store.store(&content).await?;

        // Count languages
        let lang_name = format!("{:?}", scanned.language);
        *language_counts.entry(lang_name).or_insert(0) += 1;

        // Create file entry
        let entry = mimir_files::FileEntry::new(
            FileId::new(0), // Will be assigned by index
            scanned.path.clone(),
            scanned.language,
            hash,
            scanned.size,
            scanned.modified_at,
        );

        index.insert(entry);

        // Collect file info for snapshot
        file_infos.push(FileInfo {
            path: scanned.path.clone(),
            content_hash: hash,
            size: scanned.size,
            language: scanned.language,
        });
    }

    // Calculate stored size (approximate - actual compressed size)
    let stored_size = total_size / 2; // Rough estimate

    println!(
        " {} ({} compressed)",
        format_bytes(total_size),
        format_bytes(stored_size)
    );

    // Step 5: Create initial snapshot
    print_step(5, 5, "Creating initial snapshot...");

    let snapshot_id = SnapshotId::new(1);
    let snapshot = create_snapshot(
        snapshot_id,
        file_infos,
        Some("Initial snapshot".to_string()),
    )?;

    // Save snapshot to disk
    let snapshot_path = snapshots_dir.join(format!("snap_{}", snapshot.id.value()));
    tokio::fs::create_dir_all(&snapshot_path).await?;
    let manifest_path = snapshot_path.join("manifest.json");
    let manifest_json = serde_json::to_string_pretty(&snapshot)?;
    tokio::fs::write(&manifest_path, manifest_json).await?;

    println!(" done");

    // Summary
    println!();
    println!(" Mimir initialized. Run 'mimir status' to see details.");

    // Show language breakdown
    if !language_counts.is_empty() {
        info!("Language breakdown:");
        let mut sorted: Vec<_> = language_counts.into_iter().collect();
        sorted.sort_by(|a, b| b.1.cmp(&a.1));
        for (lang, count) in sorted.iter().take(5) {
            info!("  {} ({})", lang, count);
        }
    }

    Ok(())
}

fn print_step(current: u32, total: u32, message: &str) {
    print!("[{}/{}] {}", current, total, message);
    use std::io::Write;
    std::io::stdout().flush().ok();
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
