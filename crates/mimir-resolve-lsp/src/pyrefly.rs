//! What is `pyrefly`'s own, for Python (`docs/PYTHON.md`, section 4). It runs a project's
//! `.venv/bin/python` to find its packages, and `pyrefly lsp` has no option against that.

use std::path::Path;

use mimir_types::Language;

use crate::{AskedLanguage, AskedSource};

/// The version this was measured with, which `scripts/check-deps.py` compares with the newest.
/// Another one still runs, and the daemon's log says so.
const TESTED_VERSION: &str = "1.4.0";

/// `pyrefly` as a semantic source.
pub fn pyrefly() -> AskedSource {
    AskedSource::new(AskedLanguage {
        program: "pyrefly",
        args: &["lsp"],
        tested_version: TESTED_VERSION,
        family: Language::Python,
        languages: "Python",
        project_files: &[
            "pyproject.toml",
            "pyrefly.toml",
            "setup.py",
            "setup.cfg",
            "requirements.txt",
            "requirements-dev.txt",
            "uv.lock",
            "poetry.lock",
            "pdm.lock",
            "Pipfile.lock",
            ".python-version",
        ],
        answers_file: "pyrefly-answers.txt",
        language_id,
        missing_packages,
        reports_recheck: true,
    })
}

/// `.venv` is where it is looked for first. `VIRTUAL_ENV` may say otherwise, and so the note does too.
fn missing_packages(project: &Path) -> Option<&'static str> {
    let lists_packages = ["pyproject.toml", "requirements.txt", "setup.py"]
        .iter()
        .any(|name| project.join(name).exists());
    (lists_packages && !project.join(".venv").exists()).then_some(
        "no .venv here, so less is answered, unless VIRTUAL_ENV names one. Install the project's packages.",
    )
}

fn language_id(_: &Path) -> &'static str {
    "python"
}
