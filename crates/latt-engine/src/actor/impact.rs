//! What a change touches: the symbols of each changed file against those of its version in a commit,
//! compared by their text, so a line moved by an edit above them changes nothing.

use crate::actor::GraphActor;
use crate::changes::{self, ChangedFile};
use crate::error::GraphError;
use crate::scanner;
use latt_parse_core::nodes::MODULE_NAME;
use latt_protocol::{ChangedSymbol, ImpactResponse, NamedAt, RemovedSymbol};
use latt_types::{FileId, Language, QualifiedName, Span, Symbol, SymbolId, SymbolKind};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// How many levels of dependents come back unless asked otherwise.
const DEFAULT_DEPTH: u32 = 2;
/// How many places that still name a removed symbol come back.
const NAMED_AT_KEPT: usize = 20;

/// A symbol's text, without the symbols inside it: a method added to a type does not change the
/// type. Only the outer spaces of each piece are dropped, so indenting a line of Python still counts.
pub fn own_text(span: Span, others: &[Span], source: &str) -> String {
    let mut inner: Vec<Span> = others
        .iter()
        .copied()
        .filter(|other| span.start <= other.start && other.end <= span.end && (*other != span))
        .collect();
    inner.sort_by_key(|other| other.start);
    let mut pieces = Vec::new();
    let mut from = span.start;
    for other in inner {
        if other.start >= from {
            pieces.push(
                source
                    .get(from as usize..other.start as usize)
                    .unwrap_or(""),
            );
            from = other.end;
        }
    }
    pieces.push(source.get(from as usize..span.end as usize).unwrap_or(""));
    let pieces: Vec<&str> = pieces
        .into_iter()
        .map(str::trim)
        .filter(|piece| !piece.is_empty())
        .collect();
    pieces.join("\n")
}

/// What a member belongs to, as the second last part of its name. Anything else belongs to no one
/// here: where it lives is part of its name in some languages, and moving it changes that.
fn owner(name: &QualifiedName, kind: SymbolKind) -> Option<String> {
    let is_member = matches!(
        kind,
        SymbolKind::Method | SymbolKind::Field | SymbolKind::Property | SymbolKind::EnumVariant
    );
    let segments = name.segments();
    is_member
        .then(|| segments.len().checked_sub(2))
        .flatten()
        .and_then(|at| segments.get(at))
        .cloned()
}

/// A symbol of the version compared with.
struct Before {
    name: String,
    qualified_name: String,
    owner: Option<String>,
    kind: SymbolKind,
    line: u32,
    text: String,
    path: PathBuf,
}

/// What differs in a file: the symbols added or changed, each saying if it is new, and those gone.
struct Compared {
    found: Vec<(Arc<Symbol>, bool)>,
    gone: Vec<Before>,
}

impl GraphActor {
    /// What changed since the last commit, or since `base` split off, and what depends on it.
    pub async fn handle_impact(
        &mut self,
        base: Option<String>,
        depth: Option<u32>,
    ) -> Result<ImpactResponse, GraphError> {
        self.ensure_sources();
        let root = self.root.clone();
        let changes = tokio::task::spawn_blocking(move || changes::read(&root, base.as_deref()))
            .await
            .map_err(GraphError::Changes)??;
        self.take_in(&changes.files)?;
        self.ensure_linked().await;

        let mut changed: Vec<(Arc<Symbol>, bool)> = Vec::new();
        let mut removed: Vec<Before> = Vec::new();
        let mut other_files = Vec::new();
        for file in &changes.files {
            let Compared { found, gone } = self.compare(file)?;
            if found.is_empty() && gone.is_empty() {
                other_files.push(file.path.clone());
            }
            changed.extend(found);
            removed.extend(gone);
        }
        self.pair_moved(&mut changed, &mut removed);

        let roots: Vec<SymbolId> = changed.iter().map(|(symbol, _)| symbol.id).collect();
        let depth = depth.unwrap_or(DEFAULT_DEPTH) as usize;
        let dependents = self.graph.dependents(&roots, depth, &self.file_paths);
        let changed = changed
            .into_iter()
            .zip(dependents)
            .map(|((symbol, added), dependents)| ChangedSymbol {
                symbol: self.symbol_to_info(&symbol),
                added,
                dependents: self.to_trace_nodes(dependents),
            })
            .collect();
        let removed = removed
            .into_iter()
            .map(|symbol| self.still_named(symbol))
            .collect();
        Ok(ImpactResponse {
            base: changes.base,
            changed,
            removed,
            other_files,
            provisional: self.pending.is_some(),
        })
    }

    /// Bring the graph up to the files as they are: a watcher may not have told of them yet.
    fn take_in(&mut self, files: &[ChangedFile]) -> Result<(), GraphError> {
        for file in files {
            let path = self.root.join(&file.path);
            let language = Language::from_path(&path);
            if self.registry.get_by_language(language).is_none() {
                continue;
            }
            if file.exists && scanner::takes(&self.root, &path) {
                let content =
                    std::fs::read_to_string(&path).map_err(|source| GraphError::Read {
                        path: path.clone(),
                        source,
                    })?;
                self.handle_add_file(path, content, language)?;
            } else {
                self.handle_remove_path(&path);
            }
        }
        Ok(())
    }

    /// The symbols of a file added or changed since its version in the commit, and those gone.
    /// Symbols of one name and kind, as twins behind `cfg`, are paired in the order they are written.
    fn compare(&self, file: &ChangedFile) -> Result<Compared, GraphError> {
        let mut before: HashMap<(String, SymbolKind), VecDeque<Before>> = HashMap::new();
        for symbol in self.symbols_before(file)? {
            before
                .entry((symbol.qualified_name.clone(), symbol.kind))
                .or_default()
                .push_back(symbol);
        }
        let mut found = Vec::new();
        if let Some(tracked) = self.files.get(&self.root.join(&file.path))
            && let Some(project_file) = self.sources.get(&tracked.file_id)
        {
            let mut symbols = self.graph.symbols_in_file(tracked.file_id);
            symbols.sort_by_key(|symbol| (symbol.span.start, symbol.span.end));
            let spans: Vec<Span> = symbols.iter().map(|symbol| symbol.span).collect();
            // A file's own symbol is what lies outside its symbols, as comments and imports: a file
            // changed only there is reported as such in every language.
            for symbol in symbols.into_iter().filter(|s| s.name != MODULE_NAME) {
                let key = (symbol.qualified_name.to_string(), symbol.kind);
                match before.get_mut(&key).and_then(VecDeque::pop_front) {
                    None => found.push((symbol, true)),
                    Some(old) => {
                        if old.text != own_text(symbol.span, &spans, &project_file.source) {
                            found.push((symbol, false));
                        }
                    }
                }
            }
        }
        let mut gone: Vec<Before> = before.into_values().flatten().collect();
        gone.sort_by_key(|symbol| symbol.line);
        Ok(Compared { found, gone })
    }

    /// The symbols of a file's version in the commit.
    fn symbols_before(&self, file: &ChangedFile) -> Result<Vec<Before>, GraphError> {
        let Some(text) = &file.before else {
            return Ok(Vec::new());
        };
        let language = Language::from_path(&file.path);
        let Some(adapter) = self.registry.get_by_language(language) else {
            return Ok(Vec::new());
        };
        let tree = adapter.parse(text).ok_or_else(|| GraphError::ParseFailed {
            path: file.path.clone(),
        })?;
        let mut symbols = adapter.extract_symbols(&tree, FileId::new(0));
        adapter.qualify(&file.path, &mut symbols);
        let spans: Vec<Span> = symbols.iter().map(|symbol| symbol.span).collect();
        Ok(symbols
            .into_iter()
            .filter(|symbol| symbol.name != MODULE_NAME)
            .map(|symbol| Before {
                text: own_text(symbol.span, &spans, text),
                owner: owner(&symbol.qualified_name, symbol.kind),
                name: symbol.name,
                qualified_name: symbol.qualified_name.to_string(),
                kind: symbol.kind,
                line: symbol.range.start_line,
                path: file.path.clone(),
            })
            .collect())
    }

    /// A symbol gone from one place and added in another with the same text has moved: what depends
    /// on it still does, and its name still means it.
    /// A symbol gone from one place and added in another, with the same name, kind and owner, has
    /// moved: what depends on it still does, and its name still means it. With the same text nothing
    /// happened to it, and otherwise it is changed. A method of another type is another method.
    fn pair_moved(&self, changed: &mut Vec<(Arc<Symbol>, bool)>, removed: &mut Vec<Before>) {
        let mut spans_of: HashMap<FileId, Vec<Span>> = HashMap::new();
        removed.retain(|gone| {
            let mut same_text = |symbol: &Symbol| {
                self.sources.get(&symbol.file_id).is_some_and(|file| {
                    let spans = spans_of.entry(symbol.file_id).or_insert_with(|| {
                        self.graph
                            .symbols_in_file(symbol.file_id)
                            .iter()
                            .map(|symbol| symbol.span)
                            .collect()
                    });
                    own_text(symbol.span, spans, &file.source) == gone.text
                })
            };
            let is_it = |symbol: &Symbol, added: bool| {
                added
                    && symbol.name == gone.name
                    && symbol.kind == gone.kind
                    && owner(&symbol.qualified_name, symbol.kind) == gone.owner
            };
            let unchanged = changed
                .iter()
                .position(|(symbol, added)| is_it(symbol, *added) && same_text(symbol));
            if let Some(unchanged) = unchanged {
                changed.remove(unchanged);
                return false;
            }
            match changed
                .iter_mut()
                .find(|(symbol, added)| is_it(symbol, *added))
            {
                Some((_, added)) => {
                    *added = false;
                    false
                }
                None => true,
            }
        });
    }

    /// Where a removed symbol's name is still written and resolves to nothing else. This says that a
    /// place names it, not that it called it: a name that resolves to nothing may be another's.
    fn still_named(&self, removed: Before) -> RemovedSymbol {
        let family = Language::from_path(&removed.path).family();
        let mut named_at = Vec::new();
        for (id, file) in &self.sources {
            if Language::from_path(&file.path).family() != family {
                continue;
            }
            for reference in file.references.iter().filter(|reference| {
                last_name(&reference.name) == removed.name
                    && !self.resolves(
                        *id,
                        reference.range.start_line,
                        reference.span,
                        &removed.name,
                    )
            }) {
                named_at.push(NamedAt {
                    path: file.path.clone(),
                    line: reference.range.start_line,
                    symbol: self.symbol_at(*id, reference.range.start_line),
                });
            }
        }
        named_at.sort_by(|a, b| (&a.path, a.line).cmp(&(&b.path, b.line)));
        named_at.dedup_by(|a, b| a.path == b.path && a.line == b.line);
        let named_total = named_at.len();
        named_at.truncate(NAMED_AT_KEPT);
        RemovedSymbol {
            name: removed.name,
            qualified_name: removed.qualified_name,
            kind: removed.kind,
            file_path: removed.path,
            line: removed.line,
            named_at,
            named_total,
        }
    }

    /// Whether the symbol holding a place has an edge from there to a symbol of this name.
    fn resolves(&self, file: FileId, line: u32, span: Span, name: &str) -> bool {
        self.graph
            .symbols_in_file(file)
            .iter()
            .filter(|symbol| symbol.range.start_line <= line && line <= symbol.range.end_line)
            .any(|holder| {
                self.graph
                    .get_outgoing(holder.id)
                    .iter()
                    .any(|(target, edge)| edge.span == span && target.name == name)
            })
    }
}

/// The last part of a name as written, as `helper` in `a::helper` or `a.helper`.
fn last_name(written: &str) -> &str {
    let after_path = written.rsplit("::").next().unwrap_or(written);
    let name = after_path.rsplit('.').next().unwrap_or(after_path);
    Path::new(name)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(start: u32, end: u32) -> Span {
        Span { start, end }
    }

    #[test]
    fn a_member_added_does_not_change_what_holds_it() {
        let one = "struct T {\n  fn a() {}\n}";
        let two = "struct T {\n  fn a() {}\n\n  fn b() {}\n}";
        let a = span(13, 22);
        let b = span(26, 35);
        assert_eq!(
            own_text(
                span(0, one.len() as u32),
                &[span(0, one.len() as u32), a],
                one
            ),
            own_text(
                span(0, two.len() as u32),
                &[span(0, two.len() as u32), a, b],
                two
            ),
        );
    }

    #[test]
    fn indenting_a_line_of_a_symbol_changes_it() {
        let one = "def f():\n    if x:\n        y()\n";
        let two = "def f():\n    if x:\n        pass\n    y()\n";
        let whole = |text: &str| span(0, text.len() as u32);
        assert_ne!(
            own_text(whole(one), &[], one),
            own_text(whole(two), &[], two)
        );
    }

    #[test]
    fn the_last_part_of_a_written_name_is_what_it_names() {
        assert_eq!(last_name("a::b::helper"), "helper");
        assert_eq!(last_name("self.helper"), "helper");
        assert_eq!(last_name("helper"), "helper");
    }
}
