//! Daemon command: there is rarely a need for it, since any command starts the daemon.

use anyhow::Result;
use mimir_client::Client;
use std::time::Duration;

/// How long a daemon stays without a request. Its analyzer, which holds gigabytes, stops sooner.
const DAEMON_IDLE: Duration = Duration::from_secs(30 * 60);

/// Serve this project in this process. This is what a client starts.
pub async fn run() -> Result<()> {
    mimir_daemon::run(std::env::current_dir()?, DAEMON_IDLE).await?;
    Ok(())
}

/// Start the daemon if it is not running.
pub async fn start() -> Result<()> {
    let client = Client::open(&std::env::current_dir()?).await?;
    println!("Daemon: running");
    println!("  Project: {}", client.root().display());
    Ok(())
}

/// Stop the daemon if it is running.
pub async fn stop() -> Result<()> {
    match Client::running(&std::env::current_dir()?).await? {
        Some(client) => {
            client.shutdown().await?;
            println!("Daemon stopped.");
        }
        None => println!("Daemon: not running"),
    }
    Ok(())
}

/// Show whether the daemon is running, without starting it.
pub async fn status() -> Result<()> {
    let Some(mut client) = Client::running(&std::env::current_dir()?).await? else {
        println!("Daemon: not running");
        return Ok(());
    };
    let stats = client.stats().await?;
    println!("Daemon: running");
    println!("  Project: {}", client.root().display());
    println!("  Files: {}", stats.files_count);
    println!("  Symbols: {}", stats.symbols_count);
    println!("  Edges: {}", stats.references_count);
    Ok(())
}
