//! CLI for the Mimir semantic code intelligence engine.

mod app;
mod commands;

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
        commands::Commands::Init { force } => commands::init::execute(force).await?,
        commands::Commands::Status => commands::status::execute().await?,
    }

    Ok(())
}
