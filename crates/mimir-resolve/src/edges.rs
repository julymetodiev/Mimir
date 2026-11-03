//! The edges being collected, and what the resolvers share to collect them.

use std::collections::{HashMap, HashSet};

use mimir_graph::{EdgeData, EdgeKind};
use mimir_types::{Confidence, Span, SymbolId};

use crate::ProjectFile;

/// A symbol using a constant or static.
pub struct ConstantUse {
    pub from: SymbolId,
    pub constant: SymbolId,
    pub span: Span,
    pub confidence: Confidence,
}

/// Edges with no repeats: one per (from, to, kind), at the first site found.
#[derive(Default)]
pub struct EdgeSet {
    seen: HashSet<(SymbolId, SymbolId, EdgeKind)>,
    edges: Vec<(SymbolId, SymbolId, EdgeData)>,
}

impl EdgeSet {
    pub fn add(
        &mut self,
        from: SymbolId,
        to: SymbolId,
        kind: EdgeKind,
        span: Span,
        confidence: Confidence,
    ) {
        if self.seen.insert((from, to, kind)) {
            self.edges
                .push((from, to, EdgeData::new(kind, span, confidence)));
        }
    }

    /// Every edge so far.
    pub fn iter(&self) -> impl Iterator<Item = &(SymbolId, SymbolId, EdgeData)> {
        self.edges.iter()
    }

    pub fn len(&self) -> usize {
        self.edges.len()
    }

    pub fn count(&self, kind: EdgeKind) -> usize {
        self.edges
            .iter()
            .filter(|(_, _, data)| data.kind == kind)
            .count()
    }

    pub fn into_vec(self) -> Vec<(SymbolId, SymbolId, EdgeData)> {
        self.edges
    }
}

/// File ids by project-relative path.
pub fn files_by_path(files: &[ProjectFile]) -> HashMap<&std::path::Path, &ProjectFile> {
    files.iter().map(|f| (f.path.as_path(), f)).collect()
}
