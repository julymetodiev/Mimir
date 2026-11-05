//! The semantic source behind `rust-analyzer`: its SCIP index and, for edited files, a running
//! language server.

mod index;
mod live;

use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use latt_lsp::Stopping;
use latt_resolve::{ProjectFile, Refresh, SemanticIndex, SemanticSource};

pub use index::load_or_generate;
pub use live::Analyzers;

/// `rust-analyzer` as a semantic source.
pub struct RustAnalyzer {
    analyzers: Mutex<Analyzers>,
    /// Apart from the analyzers, whose lock is held for as long as an index takes.
    stopping: Stopping,
}

impl Default for RustAnalyzer {
    fn default() -> Self {
        let analyzers = Analyzers::default();
        let stopping = analyzers.stopping();
        Self {
            analyzers: Mutex::new(analyzers),
            stopping,
        }
    }
}

impl SemanticSource for RustAnalyzer {
    fn load(
        &self,
        project: &Path,
        cache_dir: &Path,
        files: &[ProjectFile],
        refresh: Refresh,
    ) -> Vec<SemanticIndex> {
        // Reading what is saved uses no analyzer, so it must not wait for one that is busy.
        if refresh == Refresh::Saved {
            let mut none = Analyzers::default();
            return load_or_generate(project, cache_dir, files, &mut none, refresh);
        }
        let mut analyzers = self.analyzers.lock().unwrap_or_else(|e| e.into_inner());
        load_or_generate(project, cache_dir, files, &mut analyzers, refresh)
    }

    fn stop_if_idle(&self, idle: Duration) {
        if let Ok(mut analyzers) = self.analyzers.try_lock() {
            analyzers.stop_if_idle(idle);
        }
    }

    fn stop(&self) {
        self.stopping.begin();
    }
}
