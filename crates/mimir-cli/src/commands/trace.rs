//! Trace command - trace call chains for a symbol.

use anyhow::{Context, Result};
use mimir_graph::{CodeGraph, TraceOptions};
use mimir_parse_core::AdapterRegistry;
use mimir_parse_rust::RustAdapter;
use mimir_types::{FileId, IdGenerator, Language, Symbol};
use std::path::Path;
use std::sync::Arc;

/// Execute the trace command.
pub async fn execute(file: &Path, symbol_name: &str, depth: usize, json: bool) -> Result<()> {
    // Verify file exists
    if !file.exists() {
        anyhow::bail!("File not found: {}", file.display());
    }

    // Detect language
    let language = Language::from_path(file);
    if !language.is_known() {
        anyhow::bail!("Unsupported file type: {}", file.display());
    }

    // Setup adapter registry
    let registry = AdapterRegistry::new();
    registry.register(Arc::new(RustAdapter::new()));

    let adapter = registry
        .get_by_language(language)
        .context("No adapter for language")?;

    // Read and parse the file
    let source = std::fs::read_to_string(file)
        .with_context(|| format!("Failed to read {}", file.display()))?;

    let parsed = adapter
        .parse(&source)
        .context("Failed to parse file")?;

    // Create ID generator and graph
    let ids = IdGenerator::new();
    let file_id: FileId = ids.next();
    let graph = CodeGraph::new();

    // Extract symbols
    let extracted = adapter.extract_symbols(&parsed, file_id);

    // Add symbols to graph
    for ext in &extracted {
        let symbol = Symbol {
            id: ids.next(),
            name: ext.name.clone(),
            qualified_name: ext.qualified_name.clone(),
            kind: ext.kind,
            language: ext.language,
            file_id,
            range: ext.range,
            span: ext.span,
            signature: ext.signature.clone(),
            visibility: ext.visibility,
            doc: ext.doc.clone(),
        };
        graph.add_symbol(symbol);
    }

    // Extract and add references
    let references = adapter.extract_references(&parsed, file_id);
    for reference in &references {
        // Find source symbol (containing function)
        if let Some(from_symbol) = find_containing_symbol(&graph, file_id, reference.span.start) {
            // Find target symbol by name
            let targets = graph.find_symbol(&reference.name);
            if let Some(to_symbol) = targets.first() {
                graph.add_call(from_symbol.id, to_symbol.id, reference.span, reference.confidence);
            }
        }
    }

    // Find the target symbol
    let symbols = graph.find_symbol(symbol_name);
    if symbols.is_empty() {
        anyhow::bail!("Symbol '{}' not found in {}", symbol_name, file.display());
    }

    let target = &symbols[0];

    // Trace the symbol
    let options = TraceOptions::new().with_max_depth(depth);
    let result = graph
        .trace(target.id, options)
        .context("Failed to trace symbol")?;

    // Output results
    if json {
        let output = serde_json::to_string_pretty(&result)?;
        println!("{}", output);
    } else {
        println!("Trace for: {} ({})", result.root.name, result.root.kind);
        println!("  File: {}:{}", file.display(), result.root.range.start_line);
        println!();

        if !result.callers.is_empty() {
            println!("Callers ({}):", result.callers.len());
            for node in &result.callers {
                let indent = "  ".repeat(node.depth);
                println!("{}← {} ({})", indent, node.symbol.name, node.symbol.kind);
            }
            println!();
        }

        if !result.callees.is_empty() {
            println!("Callees ({}):", result.callees.len());
            for node in &result.callees {
                let indent = "  ".repeat(node.depth);
                println!("{}→ {} ({})", indent, node.symbol.name, node.symbol.kind);
            }
            println!();
        }

        println!(
            "Total: {} symbols, max depth: {}",
            result.total_count(),
            result.max_depth_reached
        );
    }

    Ok(())
}

/// Find the symbol containing a given byte offset.
fn find_containing_symbol(graph: &CodeGraph, file_id: FileId, offset: u32) -> Option<Arc<Symbol>> {
    let symbols = graph.symbols_in_file(file_id);
    symbols
        .into_iter()
        .filter(|s| s.span.start <= offset && offset <= s.span.end)
        .min_by_key(|s| s.span.end - s.span.start) // Smallest containing span
}
