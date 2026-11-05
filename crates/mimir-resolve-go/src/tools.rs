//! The programs Mimir runs for Go. They are found on `PATH`, never in the project. Each has one tested
//! version, which `scripts/check-deps.py` compares with the newest. Another one still runs, and the
//! daemon's log says so.

use mimir_resolve_scip::check_version;

const SCIP_GO_TESTED: &str = "0.2.9";
const GOPLS_TESTED: &str = "0.24.0";

pub const GO: &str = "go";
pub const SCIP_GO: &str = "scip-go";
pub const GOPLS: &str = "gopls";

/// Say so in the log if `scip-go` is not the tested version.
pub fn check_scip_go() {
    check_version(SCIP_GO, &["--version"], SCIP_GO_TESTED);
}

/// Say so in the log if `gopls` is not the tested version.
pub fn check_gopls() {
    check_version(GOPLS, &["version"], GOPLS_TESTED);
}
