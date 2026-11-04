//! What is `tsrs`'s own, for TypeScript and JavaScript (`docs/TYPESCRIPT.md`, section 4).

use std::path::Path;

use mimir_types::Language;

use crate::{AskedLanguage, AskedSource};

/// The version this was measured with, which `scripts/check-deps.py` compares with the newest.
/// Another one still runs, and the daemon's log says so.
const TESTED_VERSION: &str = "0.21.0";

/// `tsrs` as a semantic source.
pub fn tsrs() -> AskedSource {
    AskedSource::new(AskedLanguage {
        program: "tsrs",
        args: &["--lsp", "-stdio"],
        tested_version: TESTED_VERSION,
        family: Language::TypeScript,
        languages: "TypeScript and JavaScript",
        project_files: &[
            "tsconfig.json",
            "jsconfig.json",
            "package.json",
            "package-lock.json",
            "pnpm-lock.yaml",
            "yarn.lock",
        ],
        answers_file: "tsrs-answers.txt",
        language_id,
        missing_packages,
        reports_recheck: false,
    })
}

fn missing_packages(project: &Path) -> Option<&'static str> {
    (project.join("package.json").exists() && !project.join("node_modules").exists())
        .then_some("no node_modules here, so less is answered. Install the project's packages.")
}

fn language_id(path: &Path) -> &'static str {
    match (
        Language::from_path(path),
        path.extension().and_then(|e| e.to_str()),
    ) {
        (Language::Tsx, _) => "typescriptreact",
        (_, Some("jsx")) => "javascriptreact",
        (Language::JavaScript, _) => "javascript",
        _ => "typescript",
    }
}
