//! Trace command: who calls a symbol and what it calls.

use anyhow::Result;
use mimir_client::Client;
use mimir_protocol::{TraceNode, TraceOptions, TraceRelation};
use std::path::{Path, PathBuf};

/// What to trace and how to show it.
pub struct TraceRequest<'a> {
    pub file: Option<&'a Path>,
    pub symbol: &'a str,
    pub depth: usize,
    pub json: bool,
    pub show_code: bool,
    pub callers_only: bool,
    pub callees_only: bool,
}

/// Execute the trace command.
pub async fn execute(request: TraceRequest<'_>) -> Result<()> {
    let mut client = Client::open(&std::env::current_dir()?).await?;
    let file: Option<PathBuf> = request.file.map(Path::canonicalize).transpose()?;
    let options = TraceOptions {
        max_depth: Some(request.depth as u32),
        callers_only: request.callers_only,
        callees_only: request.callees_only,
        file,
    };
    let trace = client.trace(request.symbol, options).await?;

    if request.json {
        println!("{}", serde_json::to_string_pretty(&trace)?);
        return Ok(());
    }

    if trace.matches > 1 {
        eprintln!(
            "note: {} symbols are named '{}'; tracing the most likely. Narrow it with -f <file>.",
            trace.matches, request.symbol
        );
    }
    println!("Trace for: {} ({})", trace.root.name, trace.root.kind);
    println!(
        "  File: {}:{}-{}",
        trace.root.file_path.display(),
        trace.root.start_line,
        trace.root.end_line
    );

    if request.show_code {
        let source = client.source(trace.root.id, None).await?;
        println!();
        println!("```");
        for (i, line) in source.lines.iter().enumerate() {
            println!("{:4}│ {}", source.first_line as usize + i, line);
        }
        println!("```");
    }
    println!();

    if !trace.callers.is_empty() {
        println!("Callers ({}):", trace.callers.len());
        trace.callers.iter().for_each(|node| show(node, "←"));
        println!();
    }
    if !trace.callees.is_empty() {
        println!("Callees ({}):", trace.callees.len());
        trace.callees.iter().for_each(|node| show(node, "→"));
        println!();
    }
    println!(
        "Total: {} symbols, max depth: {}",
        trace.total_symbols, trace.max_depth_reached
    );
    if trace.provisional {
        println!(
            "Partial: analysis is still running, so some calls may be missing. Run this again shortly."
        );
    }
    Ok(())
}

/// One line of a trace. Which implementation a trait call reaches is decided at runtime, and a
/// function named as a value may run later or never, so these are marked as possible.
pub fn show(node: &TraceNode, arrow: &str) {
    let indent = "  ".repeat(node.depth as usize);
    let symbol = &node.symbol;
    let name = match node.relationship {
        TraceRelation::Implementation => symbol.qualified_name.as_deref().unwrap_or(&symbol.name),
        _ => &symbol.name,
    };
    let mark = match node.relationship {
        TraceRelation::CallerViaTrait => " [via trait, possible]",
        TraceRelation::Implementation => " [implementation, possible]",
        TraceRelation::CallerByValue | TraceRelation::CalleeByValue => " [as value, possible]",
        TraceRelation::Reader => " [reads it]",
        TraceRelation::User => " [uses it]",
        _ => "",
    };
    println!("{indent}{arrow} {name} ({}){mark}", symbol.kind);
}
