//! The semantic source behind `rust-analyzer`: its SCIP index and, for edited files, a running
//! language server.

mod decode;
mod index;
mod live;

use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use mimir_resolve::{ProjectFile, Refresh, ScipIndex, SemanticSource};

pub use index::load_or_generate;
pub use live::Analyzers;

/// `rust-analyzer` as a semantic source.
#[derive(Default)]
pub struct RustAnalyzer {
    analyzers: Mutex<Analyzers>,
}

impl SemanticSource for RustAnalyzer {
    fn load(
        &self,
        project: &Path,
        cache_dir: &Path,
        files: &[ProjectFile],
        refresh: Refresh,
    ) -> Vec<ScipIndex> {
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
}
