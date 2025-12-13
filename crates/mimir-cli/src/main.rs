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
            callers,
            callees,
        } => commands::trace::execute(file.as_deref(), &symbol, depth, json, code, callers, callees).await?,
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
            commands::edit::execute(file, symbol.as_deref(), edit_mode, start, end, content, preview, backup).await?
        }
        commands::Commands::EditBatch {
            json,
            preview,
            backup,
        } => commands::edit::execute_batch(&json, preview, backup).await?,
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
            commands::edit::execute_batch_symbols(&json_content, preview, backup).await?
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
        commands::Commands::Agent {
            goal,
            provider,
            model,
            max_iterations,
            verbose,
            checkpoint,
        } => {
            commands::agent::run_agent(&goal, &provider, model.as_deref(), max_iterations, verbose, checkpoint)
                .await?
        }
        commands::Commands::Tasks { status } => {
            let task_dir = app::find_project_root()
                .unwrap_or_else(|| std::env::current_dir().unwrap())
                .join(".mimir")
                .join("tasks");
            let status = status.and_then(|s| match s.as_str() {
                "pending" => Some(mimir_tasks::TaskStatus::Pending),
                "running" => Some(mimir_tasks::TaskStatus::Running),
                "paused" => Some(mimir_tasks::TaskStatus::Paused),
                "completed" | "done" => Some(mimir_tasks::TaskStatus::Completed),
                "failed" => Some(mimir_tasks::TaskStatus::Failed),
                "cancelled" => Some(mimir_tasks::TaskStatus::Cancelled),
                _ => None,
            });
            commands::agent::list_tasks(&task_dir, status).await?
        }
        commands::Commands::Resume { task_id, verbose } => {
            let task_dir = app::find_project_root()
                .unwrap_or_else(|| std::env::current_dir().unwrap())
                .join(".mimir")
                .join("tasks");
            commands::agent::resume_task(&task_id, &task_dir, verbose).await?
        }
    }

    Ok(())
}
