//! Daemon command: there is rarely a need for it, since any command starts the daemon.

use anyhow::Result;
use mimir_client::Client;
use mimir_parse_core::LanguageAdapter;
use mimir_parse_go::GoAdapter;
use mimir_parse_python::PythonAdapter;
use mimir_parse_rust::RustAdapter;
use mimir_parse_ts::ScriptAdapter;
use mimir_resolve::{NoSemanticSource, SemanticSource, SemanticSources};
use mimir_resolve_go::GoSource;
use mimir_resolve_lsp::{pyrefly, tsrs};
use mimir_resolve_ra::RustAnalyzer;
use std::sync::Arc;
use std::time::Duration;

/// How long a daemon stays without a request. Its analyzer, which holds gigabytes, stops sooner.
const DAEMON_IDLE: Duration = Duration::from_secs(30 * 60);

/// Serve this project in this process. This is what a client starts. `MIMIR_NO_SCIP` leaves it to
/// syntax rules alone.
pub async fn run() -> Result<()> {
    let semantics: Arc<dyn SemanticSource> = if std::env::var_os("MIMIR_NO_SCIP").is_some() {
        Arc::new(NoSemanticSource)
    } else {
        Arc::new(SemanticSources::new(vec![
            Arc::new(RustAnalyzer::default()),
            Arc::new(tsrs()),
            Arc::new(pyrefly()),
            Arc::new(GoSource::default()),
        ]))
    };
    let languages: Vec<Arc<dyn LanguageAdapter>> = vec![
        Arc::new(RustAdapter::new()),
        Arc::new(ScriptAdapter::typescript()),
        Arc::new(ScriptAdapter::tsx()),
        Arc::new(ScriptAdapter::javascript()),
        Arc::new(PythonAdapter),
        Arc::new(GoAdapter),
    ];
    mimir_daemon::run(std::env::current_dir()?, DAEMON_IDLE, languages, semantics).await?;
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
