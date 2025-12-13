//! CLI for the Mimir semantic code intelligence engine.

mod app;
mod commands;
mod daemon;

use clap::Parser;

#[derive(Parser)]
#[command(name = "mimir")]
#[command(author, version, about = "Semantic Code Intelligence Engine", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: commands::Commands,

    /// Enable verbose output
    #[arg(short, long, global = true)]
    verbose: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Initialize tracing
    let filter = if cli.verbose { "debug" } else { "info" };
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();

    match cli.command {
        commands::Commands::Daemon { command } => match command {
            commands::DaemonCommands::Start { foreground } => {
                commands::daemon::execute_start(foreground).await?
            }
            commands::DaemonCommands::Stop => commands::daemon::execute_stop().await?,
            commands::DaemonCommands::Status => commands::daemon::execute_status().await?,
        },
        commands::Commands::Init { force } => commands::init::execute(force).await?,
        commands::Commands::Status => commands::status::execute().await?,
        commands::Commands::Sync => commands::sync::execute().await?,
        commands::Commands::Trace {
            file,
            symbol,
            depth,
            json,
            code,
        } => commands::trace::execute(file.as_deref(), &symbol, depth, json, code).await?,
        commands::Commands::Edit {
            file,
            mode,
            start,
            end,
            content,
            preview,
            backup,
        } => {
            let edit_mode: commands::edit::EditMode =
                mode.parse().map_err(|e: String| anyhow::anyhow!(e))?;
            commands::edit::execute(file, edit_mode, start, end, content, preview, backup).await?
        }
        commands::Commands::EditBatch {
            json,
            preview,
            backup,
        } => commands::edit::execute_batch(&json, preview, backup).await?,
        commands::Commands::Search {
            pattern,
            kind,
            limit,
            json,
            code,
        } => commands::search::execute(&pattern, kind.as_deref(), limit, json, code).await?,
    }

    Ok(())
}
