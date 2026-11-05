//! The programs Mimir runs for C and C++. They are found on `PATH`, never in the project. `clangd-indexer`
//! has one tested version, which `scripts/check-deps.py` compares with the newest. Another one still
//! runs, and the daemon's log says so.

use mimir_resolve_scip::check_version;

const CLANGD_INDEXER_TESTED: &str = "23.1.0";

pub const CLANGD_INDEXER: &str = "clangd-indexer";
/// Whichever is there: Apple's follows Xcode.
pub const CLANGD: &str = "clangd";

/// Say so in the log if `clangd-indexer` is not the tested version.
pub fn check_clangd_indexer() {
    check_version(CLANGD_INDEXER, &["--version"], CLANGD_INDEXER_TESTED);
}
