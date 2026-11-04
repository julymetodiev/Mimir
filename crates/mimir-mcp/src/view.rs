//! What a model reads: only what it can use, in as few tokens as possible. Ids, repeated names and
//! a language per symbol are for programs, which get them through the CLI's `--json`.

use mimir_protocol::{
    ChangedSymbol, FileText, FilesResponse, GrepMatch, GrepResponse, ImpactResponse, NamedAt,
    RemovedSymbol, SearchResponse, SymbolInfo, SymbolSource, TraceNode, TraceRelation,
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
    /// Why this may not happen at runtime. Without it or `not_a_call`, this is an exact call.
    #[serde(skip_serializing_if = "Option::is_none")]
    possible: Option<&'static str>,
    /// What this does instead of a call.
    #[serde(skip_serializing_if = "Option::is_none")]
    not_a_call: Option<&'static str>,
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
                | TraceRelation::Reader
                | TraceRelation::User => None,
            },
            not_a_call: match node.relationship {
                TraceRelation::Reader => Some("reads it"),
                TraceRelation::User => Some("uses it"),
                _ => None,
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

/// A symbol added or changed.
#[derive(Serialize)]
struct Changed {
    #[serde(flatten)]
    symbol: Found,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    added: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    dependents: Vec<Step>,
}

impl From<&ChangedSymbol> for Changed {
    fn from(changed: &ChangedSymbol) -> Self {
        Self {
            symbol: Found::from(&changed.symbol),
            added: changed.added,
            dependents: changed.dependents.iter().map(Step::from).collect(),
        }
    }
}

/// A symbol gone, and where its name is still written and means nothing else.
#[derive(Serialize)]
struct Removed {
    name: String,
    kind: String,
    was_at: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    still_named_at: Vec<String>,
    /// All such places, of which `still_named_at` may be the first few.
    #[serde(skip_serializing_if = "is_zero")]
    still_named_total: usize,
}

fn named_place(named: &NamedAt) -> String {
    let place = format!("{}:{}", named.path.display(), named.line);
    match &named.symbol {
        Some(symbol) => format!("{place} in {symbol}"),
        None => place,
    }
}

impl From<&RemovedSymbol> for Removed {
    fn from(removed: &RemovedSymbol) -> Self {
        Self {
            name: removed.qualified_name.clone(),
            kind: removed.kind.to_string(),
            was_at: format!("{}:{}", removed.file_path.display(), removed.line),
            still_named_at: removed.named_at.iter().map(named_place).collect(),
            still_named_total: removed.named_total,
        }
    }
}

/// What a change touches.
#[derive(Serialize)]
pub struct Impact {
    base: String,
    changed: Vec<Changed>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    removed: Vec<Removed>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    other_files: Vec<std::path::PathBuf>,
    /// Analysis is still running, so dependents may be missing.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    provisional: bool,
}

impl From<ImpactResponse> for Impact {
    fn from(impact: ImpactResponse) -> Self {
        Self {
            base: impact.base,
            changed: impact.changed.iter().map(Changed::from).collect(),
            removed: impact.removed.iter().map(Removed::from).collect(),
            other_files: impact.other_files,
            provisional: impact.provisional,
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
    #[serde(skip_serializing_if = "Vec::is_empty")]
    notes: Vec<String>,
}

impl From<mimir_protocol::StatsResponse> for Status {
    fn from(stats: mimir_protocol::StatsResponse) -> Self {
        Self {
            files: stats.files_count,
            symbols: stats.symbols_count,
            edges: stats.references_count,
            notes: stats.notes,
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
    /// Files passed over for their size: a match there is not among these.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    too_large: Vec<std::path::PathBuf>,
}

impl From<GrepResponse> for Grep {
    fn from(found: GrepResponse) -> Self {
        Self {
            matches: found.matches.into_iter().map(Line::from).collect(),
            total: found.total,
            too_large: found.too_large,
        }
    }
}

/// Files of the project.
#[derive(Serialize)]
pub struct Files {
    files: Vec<std::path::PathBuf>,
    /// All that match, of which `files` may be the first few.
    total: usize,
}

impl From<FilesResponse> for Files {
    fn from(found: FilesResponse) -> Self {
        Self {
            files: found.files,
            total: found.total,
        }
    }
}

/// Lines of a file, each with its number, as `edit_lines` needs them.
#[derive(Serialize)]
pub struct Text {
    file: std::path::PathBuf,
    total_lines: u32,
    text: String,
}

impl From<FileText> for Text {
    fn from(file: FileText) -> Self {
        let text = file
            .lines
            .iter()
            .enumerate()
            .map(|(offset, line)| format!("{}\t{line}", file.first_line as usize + offset))
            .collect::<Vec<_>>()
            .join("\n");
        Self {
            file: file.path,
            total_lines: file.total_lines,
            text,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(relationship: TraceRelation) -> serde_json::Value {
        let symbol = serde_json::from_value(serde_json::json!({
            "id": 1, "name": "f", "qualified_name": null, "kind": "function", "file_id": 1,
            "file_path": "a.py", "start_line": 1, "end_line": 2, "language": "python",
        }))
        .unwrap();
        let node = TraceNode {
            symbol,
            depth: 1,
            relationship,
        };
        serde_json::to_value(Step::from(&node)).unwrap()
    }

    #[test]
    fn a_removed_symbol_says_where_it_is_still_named_and_nothing_empty_is_shown() {
        let answer: ImpactResponse = serde_json::from_value(serde_json::json!({
            "base": "abc", "changed": [], "other_files": [], "provisional": false,
            "removed": [{
                "name": "f", "qualified_name": "m::f", "kind": "function", "file_path": "a.rs",
                "line": 3, "named_total": 1,
                "named_at": [{"path": "b.rs", "line": 7, "symbol": "g"}],
            }],
        }))
        .unwrap();
        let shown = serde_json::to_value(Impact::from(answer)).unwrap();
        assert_eq!(shown["removed"][0]["name"], "m::f");
        assert_eq!(shown["removed"][0]["was_at"], "a.rs:3");
        assert_eq!(shown["removed"][0]["still_named_at"][0], "b.rs:7 in g");
        assert!(shown.get("other_files").is_none() && shown.get("provisional").is_none());
    }

    #[test]
    fn what_reads_or_uses_a_symbol_is_not_shown_as_a_call() {
        assert_eq!(step(TraceRelation::User)["not_a_call"], "uses it");
        assert_eq!(step(TraceRelation::Reader)["not_a_call"], "reads it");
        let call = step(TraceRelation::Caller);
        assert!(call.get("not_a_call").is_none() && call.get("possible").is_none());
    }
}
