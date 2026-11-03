//! What a view is told (docs/ARCHITECTURE.md section 7). Announcing never waits: with nobody
//! watching a send does nothing, and one who falls behind is told so by its channel.

use crate::actor::GraphActor;
use mimir_graph::EdgeKind;
use mimir_protocol::{
    ActivityKind, GraphEvent, GraphSnapshot, SnapshotEdge, SnapshotEdgeKind, SnapshotSymbol,
};
use mimir_types::{FileId, SymbolId};
use std::collections::HashMap;
use tracing::debug;

/// The most symbols one query or edit lights up: a view shows where, not everything.
const MOST_IN_ACTIVITY: usize = 64;

impl GraphActor {
    fn announce(&self, event: GraphEvent) {
        if self.events.send(event).is_err() {
            debug!("Nobody watches the graph");
        }
    }

    /// Say that the edges were resolved anew.
    pub fn announce_new_edges(&mut self) {
        self.generation += 1;
        self.announce(GraphEvent::GraphChanged {
            generation: self.generation,
            provisional: self.pending.is_some(),
        });
    }

    /// Say once that files changed, so a watcher asks for a snapshot, which resolves them.
    pub fn announce_changed_files(&mut self) {
        if self.needs_link && !self.announced_change {
            self.announced_change = true;
            self.announce(GraphEvent::GraphChanged {
                generation: self.generation,
                provisional: true,
            });
        }
    }

    /// Say which symbols a query or an edit touched.
    pub fn announce_activity(&self, symbols: Vec<SymbolId>, kind: ActivityKind) {
        if self.events.receiver_count() > 0 && !symbols.is_empty() {
            let mut symbols = symbols;
            symbols.truncate(MOST_IN_ACTIVITY);
            self.announce(GraphEvent::Activity { symbols, kind });
        }
    }

    /// The whole graph as it is.
    pub fn handle_snapshot(&self) -> GraphSnapshot {
        let mut files = Vec::with_capacity(self.file_paths.len());
        let mut file_places: HashMap<FileId, u32> = HashMap::new();
        let mut paths: Vec<_> = self.file_paths.iter().collect();
        paths.sort_by(|a, b| a.1.cmp(b.1));
        for (id, path) in paths {
            file_places.insert(*id, files.len() as u32);
            files.push(path.clone());
        }

        let all = self.graph.symbols().all_symbols();
        let mut places: HashMap<SymbolId, u32> = HashMap::with_capacity(all.len());
        let mut symbols = Vec::with_capacity(all.len());
        for symbol in &all {
            let Some(file) = file_places.get(&symbol.file_id) else {
                continue;
            };
            places.insert(symbol.id, symbols.len() as u32);
            symbols.push(SnapshotSymbol {
                id: symbol.id,
                name: symbol.name.clone(),
                kind: symbol.kind,
                file: *file,
                line: symbol.range.start_line,
            });
        }

        let mut edges = Vec::new();
        for symbol in &all {
            let Some(from) = places.get(&symbol.id) else {
                continue;
            };
            for (target, edge) in self.graph.get_outgoing(symbol.id) {
                if let Some(to) = places.get(&target.id) {
                    edges.push(SnapshotEdge {
                        from: *from,
                        to: *to,
                        kind: snapshot_kind(edge.kind),
                    });
                }
            }
        }

        GraphSnapshot {
            generation: self.generation,
            provisional: self.needs_link || self.pending.is_some(),
            files,
            symbols,
            edges,
        }
    }
}

fn snapshot_kind(kind: EdgeKind) -> SnapshotEdgeKind {
    match kind {
        EdgeKind::Calls => SnapshotEdgeKind::Call,
        EdgeKind::UsesAsValue | EdgeKind::Implements => SnapshotEdgeKind::Possible,
        EdgeKind::UsesConstant | EdgeKind::UsesType | EdgeKind::UsesField => SnapshotEdgeKind::Use,
        EdgeKind::Imports => SnapshotEdgeKind::Import,
    }
}
