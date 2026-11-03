//! CLI for the Mimir semantic code intelligence engine.

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
    let serving = matches!(
        cli.command,
        commands::Commands::Daemon {
            command: commands::DaemonCommands::Run
        }
    );
    let filter = match (cli.verbose, serving) {
        (true, _) => "debug",
        (false, true) => "info",
        (false, false) => "warn",
    };
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_writer(std::io::stderr)
        .init();

    match cli.command {
        commands::Commands::Daemon { command } => match command {
            commands::DaemonCommands::Start => commands::daemon::start().await?,
            commands::DaemonCommands::Run => commands::daemon::run().await?,
            commands::DaemonCommands::Stop => commands::daemon::stop().await?,
            commands::DaemonCommands::Status => commands::daemon::status().await?,
        },
        commands::Commands::Mcp => mimir_mcp::serve(std::env::current_dir()?).await?,
        commands::Commands::Init => commands::init::execute()?,
        commands::Commands::Status => commands::status::execute().await?,
        commands::Commands::Sync { full } => commands::sync::execute(full).await?,
        commands::Commands::Trace {
            file,
            symbol,
            depth,
            json,
            code,
            callers,
            callees,
        } => {
            commands::trace::execute(commands::trace::TraceRequest {
                file: file.as_deref(),
                symbol: &symbol,
                depth,
                json,
                show_code: code,
                callers_only: callers,
                callees_only: callees,
            })
            .await?
        }
        commands::Commands::Edit {
            file,
            symbol,
            mode,
            start,
            end,
            content,
            preview,
            backup,
        } => {
            let edit_mode: commands::edit::EditMode =
                mode.parse().map_err(|e: String| anyhow::anyhow!(e))?;
            commands::edit::execute(commands::edit::EditRequest {
                file,
                symbol: symbol.as_deref(),
                mode: edit_mode,
                start,
                end,
                content,
                preview,
                backup,
            })
            .await?
        }
        commands::Commands::EditSymbols {
            json,
            preview,
            backup,
        } => {
            let json_content = if json == "-" {
                use std::io::Read;
                let mut buffer = String::new();
                std::io::stdin().read_to_string(&mut buffer)?;
                buffer
            } else {
                json
            };
            commands::edit::execute_symbols(&json_content, preview, backup).await?
        }
        commands::Commands::Search {
            pattern,
            kind,
            limit,
            json,
            code,
        } => commands::search::execute(&pattern, kind.as_deref(), limit, json, code).await?,
        commands::Commands::Read {
            symbol,
            before,
            after,
            numbers,
            json,
        } => commands::read::execute(&symbol, before, after, numbers, json).await?,
    }

    Ok(())
}
