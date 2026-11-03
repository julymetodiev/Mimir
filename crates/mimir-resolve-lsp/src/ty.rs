//! What is `ty`'s own, for Python (`docs/PYTHON.md`, section 4).

use std::path::Path;

use mimir_types::Language;

use crate::{AskedLanguage, AskedSource};

/// The version this was measured with, which `scripts/check-deps.py` compares with the newest.
/// Another one still runs, and the daemon's log says so.
const TESTED_VERSION: &str = "0.0.96";

/// `ty` as a semantic source.
pub fn ty() -> AskedSource {
    AskedSource::new(AskedLanguage {
        program: "ty",
        args: &["server"],
        tested_version: TESTED_VERSION,
        family: Language::Python,
        languages: "Python",
        project_files: &[
            "pyproject.toml",
            "ty.toml",
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
        answers_file: "ty-answers.txt",
        language_id,
        missing_packages,
    })
}

/// `.venv` is where `ty` looks by itself. `VIRTUAL_ENV` may say otherwise, and so the note does too.
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
