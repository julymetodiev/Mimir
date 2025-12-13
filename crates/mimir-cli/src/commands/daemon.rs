//! Daemon command - manage the Mimir daemon.

use anyhow::{Context, Result};
use std::process::Command;

use crate::app::{find_project_root, Config};
use crate::daemon::{check_daemon_running, DaemonClient, DaemonServer};

/// Start the daemon in foreground mode.
pub async fn execute_start(foreground: bool) -> Result<()> {
    let root = find_project_root().context("Not in a Mimir project. Run 'mimir init' first.")?;
    let config = Config::new(&root);

    if !config.is_initialized() {
        anyhow::bail!("Project not initialized. Run 'mimir init' first.");
    }

    // Check if already running
    if check_daemon_running(&config.agent_dir) {
        anyhow::bail!("Daemon is already running. Use 'mimir daemon stop' first.");
    }

    if foreground {
        // Run in foreground
        println!("Starting daemon in foreground mode...");
        let server = DaemonServer::start(config).await?;
        server.run().await?;
    } else {
        // Fork to background
        println!("Starting daemon in background...");

        // Get the current executable path
        let exe = std::env::current_exe()?;

        // Spawn daemon process
        let child = Command::new(&exe)
            .args(["daemon", "start", "--foreground"])
            .current_dir(&root)
            .spawn()
            .context("Failed to spawn daemon process")?;

        println!("Daemon started with PID: {}", child.id());
        println!("Socket: {}", config.agent_dir.join("daemon.sock").display());
    }

    Ok(())
}

/// Stop the running daemon.
pub async fn execute_stop() -> Result<()> {
    let root = find_project_root().context("Not in a Mimir project. Run 'mimir init' first.")?;
    let config = Config::new(&root);

    if !config.is_initialized() {
        anyhow::bail!("Project not initialized.");
    }

    if !check_daemon_running(&config.agent_dir) {
        println!("Daemon is not running.");
        return Ok(());
    }

    // Connect and send shutdown
    let mut client = DaemonClient::connect(&config.agent_dir).await?;
    client.shutdown().await?;

    println!("Daemon stopped.");
    Ok(())
}

/// Show daemon status.
pub async fn execute_status() -> Result<()> {
    let root = find_project_root().context("Not in a Mimir project. Run 'mimir init' first.")?;
    let config = Config::new(&root);

    if !config.is_initialized() {
        anyhow::bail!("Project not initialized.");
    }

    if !check_daemon_running(&config.agent_dir) {
        println!("Daemon: not running");
        println!("  Start with: mimir daemon start");
        return Ok(());
    }

    // Connect and get stats
    let mut client = DaemonClient::connect(&config.agent_dir).await?;

    // Ping to verify
    if !client.ping().await? {
        println!("Daemon: not responding");
        return Ok(());
    }

    let stats = client.stats().await?;

    println!("Daemon: running");
    println!("  Project: {}", config.root.display());
    println!("  Socket: {}", config.agent_dir.join("daemon.sock").display());
    println!("  Files indexed: {}", stats.files_count);
    println!("  Symbols indexed: {}", stats.symbols_count);
    println!("  References: {}", stats.references_count);

    Ok(())
}

/// Trigger a manual sync via daemon.
#[allow(dead_code)]
pub async fn execute_sync_via_daemon() -> Result<()> {
    let root = find_project_root().context("Not in a Mimir project. Run 'mimir init' first.")?;
    let config = Config::new(&root);

    if !check_daemon_running(&config.agent_dir) {
        anyhow::bail!("Daemon is not running. Start with 'mimir daemon start'.");
    }

    let mut client = DaemonClient::connect(&config.agent_dir).await?;
    let result = client.sync().await?;

    println!("Sync complete:");
    println!("  Files: +{} ~{} -{}",
        result.files_added, result.files_updated, result.files_removed);
    println!("  Symbols: +{} -{}",
        result.symbols_added, result.symbols_removed);
    println!("  Duration: {}ms", result.duration_ms);

    Ok(())
}
