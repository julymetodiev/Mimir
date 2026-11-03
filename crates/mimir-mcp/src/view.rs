//! What a model reads: only what it can use, in as few tokens as possible. Ids, repeated names and
//! a language per symbol are for programs, which get them through the CLI's `--json`.

use mimir_protocol::{
    GrepMatch, GrepResponse, SearchResponse, SymbolInfo, SymbolSource, TraceNode, TraceRelation,
    TraceResponse,
};
use serde::Serialize;

/// Where a symbol is: `path:first-last`.
fn place(symbol: &SymbolInfo) -> String {
    format!(
        "{}:{}-{}",
        symbol.file_path.display(),
        symbol.start_line,
        symbol.end_line
    )
}

/// The fullest name of a symbol, which says what it belongs to.
fn full_name(symbol: &SymbolInfo) -> String {
    symbol
        .qualified_name
        .clone()
        .unwrap_or_else(|| symbol.name.clone())
}

/// A symbol.
#[derive(Serialize)]
pub struct Found {
    name: String,
    kind: String,
    at: String,
}

impl From<&SymbolInfo> for Found {
    fn from(symbol: &SymbolInfo) -> Self {
        Self {
            name: full_name(symbol),
            kind: symbol.kind.to_string(),
            at: place(symbol),
        }
    }
}

/// A symbol in a trace.
#[derive(Serialize)]
pub struct Step {
    #[serde(flatten)]
    symbol: Found,
    /// How far from the root, when more than one step.
    #[serde(skip_serializing_if = "is_first_level")]
    depth: u32,
    /// Why this may not happen at runtime. Without it, this is an exact call.
    #[serde(skip_serializing_if = "Option::is_none")]
    possible: Option<&'static str>,
}

fn is_first_level(depth: &u32) -> bool {
    *depth == 1
}

impl From<&TraceNode> for Step {
    fn from(node: &TraceNode) -> Self {
        Self {
            symbol: Found::from(&node.symbol),
            depth: node.depth,
            possible: match node.relationship {
                TraceRelation::CallerViaTrait => Some("via trait"),
                TraceRelation::Implementation => Some("implementation"),
                TraceRelation::CallerByValue | TraceRelation::CalleeByValue => Some("as value"),
                TraceRelation::Root
                | TraceRelation::Caller
                | TraceRelation::Callee
                | TraceRelation::Reader => None,
            },
        }
    }
}

/// A trace.
#[derive(Serialize)]
pub struct Trace {
    #[serde(flatten)]
    root: Found,
    callers: Vec<Step>,
    callees: Vec<Step>,
    /// Analysis is still running, so calls may be missing.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    provisional: bool,
    /// How many other symbols share the name. `file` chooses among them.
    #[serde(skip_serializing_if = "is_zero")]
    others_with_this_name: usize,
}

fn is_zero(count: &usize) -> bool {
    *count == 0
}

impl From<TraceResponse> for Trace {
    fn from(trace: TraceResponse) -> Self {
        Self {
            root: Found::from(&trace.root),
            callers: trace.callers.iter().map(Step::from).collect(),
            callees: trace.callees.iter().map(Step::from).collect(),
            provisional: trace.provisional,
            others_with_this_name: trace.matches.saturating_sub(1),
        }
    }
}

/// A search.
#[derive(Serialize)]
pub struct Search {
    symbols: Vec<Found>,
    /// All that match, of which `symbols` may be the first few.
    total: usize,
}

impl From<SearchResponse> for Search {
    fn from(found: SearchResponse) -> Self {
        Self {
            symbols: found.symbols.iter().map(Found::from).collect(),
            total: found.total_count,
        }
    }
}

/// A symbol's source.
#[derive(Serialize)]
pub struct Source {
    #[serde(flatten)]
    symbol: Found,
    first_line: u32,
    code: String,
    /// How many other symbols share the name. `file` chooses among them.
    #[serde(skip_serializing_if = "is_zero")]
    others_with_this_name: usize,
}

impl From<SymbolSource> for Source {
    fn from(source: SymbolSource) -> Self {
        Self {
            symbol: Found::from(&source.symbol),
            first_line: source.first_line,
            code: source.lines.join("\n"),
            others_with_this_name: source.matches.saturating_sub(1),
        }
    }
}

/// The size of the graph.
#[derive(Serialize)]
pub struct Status {
    files: usize,
    symbols: usize,
    edges: usize,
}

impl From<mimir_protocol::StatsResponse> for Status {
    fn from(stats: mimir_protocol::StatsResponse) -> Self {
        Self {
            files: stats.files_count,
            symbols: stats.symbols_count,
            edges: stats.references_count,
        }
    }
}

/// A line of text that matched.
#[derive(Serialize)]
pub struct Line {
    at: String,
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    symbol: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    before: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    after: Vec<String>,
}

impl From<GrepMatch> for Line {
    fn from(found: GrepMatch) -> Self {
        Self {
            at: format!("{}:{}", found.path.display(), found.line),
            text: found.text,
            symbol: found.symbol,
            before: found.before,
            after: found.after,
        }
    }
}

/// A text search.
#[derive(Serialize)]
pub struct Grep {
    matches: Vec<Line>,
    /// All that match, of which `matches` may be the first few.
    total: usize,
}

impl From<GrepResponse> for Grep {
    fn from(found: GrepResponse) -> Self {
        Self {
            matches: found.matches.into_iter().map(Line::from).collect(),
            total: found.total,
        }
    }
}
