//! The call graph from your compiler or type checker, for humans and coding agents.

/// A reader gone, as `| head` leaves behind, ends this as it ends any other program. Rust ignores the
/// signal that would do that, and its own `println!` panics instead.
macro_rules! println {
    () => {
        crate::write_line(format_args!(""))
    };
    ($($arg:tt)*) => {
        crate::write_line(format_args!($($arg)*))
    };
}

mod commands;

use clap::Parser;
use std::io::Write;

/// What a shell says of a program a closed pipe ended: 128 and the signal's number.
const READER_GONE: i32 = 141;

fn write_line(line: std::fmt::Arguments<'_>) {
    let mut out = std::io::stdout().lock();
    match out.write_fmt(line).and_then(|()| out.write_all(b"\n")) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => std::process::exit(READER_GONE),
        Err(e) => panic!("failed printing to stdout: {e}"),
    }
}

#[derive(Parser)]
#[command(name = "latt")]
#[command(author, version, about = "The call graph from your compiler or type checker, for humans and coding agents", long_about = None)]
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
        commands::Commands::Mcp => latt_mcp::serve(std::env::current_dir()?).await?,
        commands::Commands::View { no_open, export } => {
            let here = std::env::current_dir()?;
            match export {
                Some(directory) => {
                    let symbols = latt_view::export(&here, &directory).await?;
                    println!("Wrote {symbols} symbols to {}", directory.display());
                }
                None => latt_view::serve(&here, !no_open).await?,
            }
        }
        commands::Commands::Init => commands::init::execute()?,
        commands::Commands::Status => commands::status::execute().await?,
        commands::Commands::Sync { full, json } => commands::sync::execute(full, json).await?,
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
        commands::Commands::Impact { base, depth, json } => {
            commands::impact::execute(base, depth, json).await?
        }
        commands::Commands::Hook { state } => {
            use std::io::Read;
            let mut event = String::new();
            std::io::stdin().read_to_string(&mut event)?;
            // A hook that fails must not get in the way of what the agent is doing.
            match latt_hook::answer(&event, &state) {
                Ok(Some(answer)) => println!("{answer}"),
                Ok(None) => {}
                Err(e) => eprintln!("latt hook: {e}"),
            }
        }
        commands::Commands::Edit {
            file,
            symbol,
            mode,
            start,
            end,
            content,
            preview,
            old,
            all,
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
                old,
                all,
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
        commands::Commands::Grep {
            pattern,
            literal,
            ignore_case,
            path,
            context,
            limit,
            json,
        } => {
            let options = latt_protocol::GrepOptions {
                pattern,
                literal,
                ignore_case,
                path,
                context,
                limit,
            };
            commands::grep::execute(options, json).await?
        }
        commands::Commands::Files { glob, limit, json } => {
            commands::files::list(glob, limit, json).await?
        }
        commands::Commands::Write {
            file,
            content,
            overwrite,
        } => commands::files::write(&file, content, overwrite).await?,
        commands::Commands::Cat {
            file,
            first,
            last,
            json,
        } => commands::files::cat(&file, first, last, json).await?,
        commands::Commands::Read {
            symbol,
            file,
            before,
            after,
            numbers,
            json,
        } => {
            commands::read::execute(&symbol, file.as_deref(), before, after, numbers, json).await?
        }
    }

    Ok(())
}
