//! Daemon command: there is rarely a need for it, since any command starts the daemon.

use anyhow::Result;
use latt_client::Client;
use latt_parse_c::CFamilyAdapter;
use latt_parse_core::LanguageAdapter;
use latt_parse_go::GoAdapter;
use latt_parse_python::PythonAdapter;
use latt_parse_rust::RustAdapter;
use latt_parse_ts::ScriptAdapter;
use latt_resolve::{NoSemanticSource, SemanticSource, SemanticSources};
use latt_resolve_clang::ClangSource;
use latt_resolve_go::GoSource;
use latt_resolve_lsp::{pyrefly, tsrs};
use latt_resolve_ra::RustAnalyzer;
use std::sync::Arc;
use std::time::Duration;

/// How long a daemon stays without a request. Its analyzer, which holds gigabytes, stops sooner.
const DAEMON_IDLE: Duration = Duration::from_secs(30 * 60);

/// Serve this project in this process. This is what a client starts. `LATT_NO_SCIP` leaves it to
/// syntax rules alone.
pub async fn run() -> Result<()> {
    let semantics: Arc<dyn SemanticSource> = if std::env::var_os("LATT_NO_SCIP").is_some() {
        Arc::new(NoSemanticSource)
    } else {
        Arc::new(SemanticSources::new(vec![
            Arc::new(RustAnalyzer::default()),
            Arc::new(tsrs()),
            Arc::new(pyrefly()),
            Arc::new(GoSource::default()),
            Arc::new(ClangSource::default()),
        ]))
    };
    let languages: Vec<Arc<dyn LanguageAdapter>> = vec![
        Arc::new(RustAdapter::new()),
        Arc::new(ScriptAdapter::typescript()),
        Arc::new(ScriptAdapter::tsx()),
        Arc::new(ScriptAdapter::javascript()),
        Arc::new(PythonAdapter),
        Arc::new(GoAdapter),
        Arc::new(CFamilyAdapter::c()),
        Arc::new(CFamilyAdapter::cpp()),
    ];
    latt_daemon::run(std::env::current_dir()?, DAEMON_IDLE, languages, semantics).await?;
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
