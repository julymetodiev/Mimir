//! Edited files, resolved by a running `rust-analyzer` instead of a new index.
//!
//! An index costs a whole run after any edit. A running analyzer recomputes what the edit
//! changed, so it answers for one file in well under a second. It answers one position at a
//! time and has no symbol names to give, so only the sites the parser found are asked about:
//! a function named as a value among a macro's tokens waits for the next index.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use mimir_parse_core::ExtractedReference;
use mimir_types::ReferenceKind;
use tracing::info;

use mimir_resolve::{LiveDocument, ProjectFile, Target, type_name};

mod lsp;

pub use crate::live::lsp::stop;
use crate::live::lsp::{Analyzer, Location};

/// Going to the definition of these lands on what they use (`fmt`, `from_str`, `from`), not on
/// them. Those are not the calls written here, and an index does not report them.
const FORWARDING_METHODS: [&str; 4] = ["into", "try_into", "parse", "to_string"];

/// The running analyzers of a project, one per Cargo root.
#[derive(Default)]
pub struct Analyzers {
    running: HashMap<PathBuf, Analyzer>,
    last_used: Option<Instant>,
}

impl Analyzers {
    /// Stop every analyzer if none was used for `idle`: each holds gigabytes.
    pub fn stop_if_idle(&mut self, idle: Duration) {
        if self.last_used.is_some_and(|used| used.elapsed() >= idle) && !self.running.is_empty() {
            info!("Stopping {} idle rust-analyzer", self.running.len());
            self.running.clear();
            self.last_used = None;
        }
    }

    /// Ask about every reference in `stale`, and the trait of every impl, from the analyzer
    /// of the Cargo root at `root`, an absolute path. `files` are all the project's files.
    pub fn resolve(
        &mut self,
        project: &Path,
        root: &Path,
        files: &[ProjectFile],
        stale: &[&ProjectFile],
    ) -> Result<HashMap<PathBuf, LiveDocument>, String> {
        self.last_used = Some(Instant::now());
        if !self.running.contains_key(root) {
            let started = Instant::now();
            let analyzer = Analyzer::start(root)?;
            info!(
                "rust-analyzer ready for {:?} in {:?}",
                root,
                started.elapsed()
            );
            self.running.insert(root.to_path_buf(), analyzer);
        }
        let analyzer = self.running.get_mut(root).ok_or("no analyzer")?;
        let result = resolve_documents(analyzer, project, files, stale);
        if result.is_err() {
            self.running.remove(root);
        }
        self.last_used = Some(Instant::now());
        result
    }
}

fn resolve_documents(
    analyzer: &mut Analyzer,
    project: &Path,
    files: &[ProjectFile],
    stale: &[&ProjectFile],
) -> Result<HashMap<PathBuf, LiveDocument>, String> {
    let project = project.canonicalize().map_err(|e| e.to_string())?;
    let sources: HashMap<&Path, &str> = files
        .iter()
        .map(|f| (f.path.as_path(), &*f.source))
        .collect();
    for file in stale {
        analyzer.update(&project.join(&file.path), &file.source)?;
    }
    let target = |found: Location| {
        let path = found.path.strip_prefix(&project).ok()?.to_path_buf();
        let line = sources
            .get(path.as_path())?
            .split('\n')
            .nth(found.line as usize)?;
        let name = line
            .get(found.column as usize..found.end_column as usize)?
            .to_string();
        Some(Target {
            path,
            line: found.line + 1,
            name,
        })
    };

    let mut documents = HashMap::new();
    for file in stale {
        let path = project.join(&file.path);
        let mut document = LiveDocument::default();
        for reference in file.references.iter().filter(|r| {
            matches!(
                r.kind,
                ReferenceKind::Call
                    | ReferenceKind::FunctionValue
                    | ReferenceKind::Read
                    | ReferenceKind::TypeUse
                    | ReferenceKind::FieldUse
            )
        }) {
            let (line, column) = site_of(reference);
            let found = analyzer
                .definition(&path, line - 1, column)?
                .into_iter()
                .filter_map(target)
                .collect();
            if reference.kind == ReferenceKind::FieldUse {
                document.field_sites.insert((line, column), found);
                continue;
            }
            if let Some(found) = choose(found, &reference.name)
                && (found.name == reference.name
                    || !FORWARDING_METHODS.contains(&reference.name.as_str()))
            {
                document.sites.insert((line, column), found);
            }
        }
        for (line, column) in impl_trait_names(&file.source) {
            let found = analyzer
                .definition(&path, line - 1, column)?
                .into_iter()
                .find_map(target);
            if let Some(found) = found {
                document.traits.insert(line, found);
            }
        }
        documents.insert(file.path.clone(), document);
    }
    Ok(documents)
}

/// The answer meant by a site. Inside a macro the server answers once per expansion, and those
/// can include whatever the macro calls; a different name only fits a renaming import.
fn choose(found: Vec<Target>, name: &str) -> Option<Target> {
    let mut found = found.into_iter();
    let first = found.next()?;
    if first.name == name {
        return Some(first);
    }
    found.find(|t| t.name == name).or(Some(first))
}

fn site_of(reference: &ExtractedReference) -> (u32, u32) {
    (reference.range.start_line, reference.range.start_col)
}

/// Where the trait is named in each single-line `impl Tr for T`: the `impl`'s line, 1-based,
/// and the name's column. An impl with a header across lines waits for the next index.
fn impl_trait_names(source: &str) -> Vec<(u32, u32)> {
    let mut found = Vec::new();
    for (i, line) in source.split('\n').enumerate() {
        let text = line.trim_start();
        let text = text.strip_prefix("unsafe ").unwrap_or(text);
        let Some(rest) = text
            .strip_prefix("impl")
            .filter(|r| r.starts_with([' ', '<']))
        else {
            continue;
        };
        let rest = skip_generics(rest).trim_start();
        let Some((implemented, _)) = rest.split_once(" for ") else {
            continue;
        };
        let name = type_name(implemented);
        let at = rest.as_ptr() as usize - line.as_ptr() as usize;
        if let Some(offset) = implemented.find(name).filter(|_| !name.is_empty()) {
            // The last segment of the path, not an earlier one that contains its text.
            let offset = implemented
                .split('<')
                .next()
                .unwrap_or(implemented)
                .rfind(name)
                .unwrap_or(offset);
            found.push((i as u32 + 1, (at + offset) as u32));
        }
    }
    found
}

/// The text after a leading `<...>`.
fn skip_generics(text: &str) -> &str {
    let trimmed = text.trim_start();
    if !trimmed.starts_with('<') {
        return text;
    }
    let (mut depth, mut previous) = (0, ' ');
    for (at, c) in trimmed.char_indices() {
        match c {
            '<' => depth += 1,
            '>' if previous != '-' => depth -= 1,
            _ => {}
        }
        if depth == 0 {
            return &trimmed[at + 1..];
        }
        previous = c;
    }
    ""
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chooses_the_answer_with_the_name_at_the_site() {
        let target = |name: &str| Target {
            path: PathBuf::from("a.rs"),
            line: 1,
            name: name.to_string(),
        };
        let chosen = |found: &[&str], name| {
            choose(found.iter().map(|n| target(n)).collect(), name).map(|t| t.name)
        };

        assert_eq!(
            chosen(&["block_on", "helper"], "helper").as_deref(),
            Some("helper")
        );
        assert_eq!(
            chosen(&["original"], "renamed").as_deref(),
            Some("original")
        );
        assert_eq!(chosen(&[], "helper"), None);
    }

    #[test]
    fn finds_the_trait_in_an_impl_header() {
        let source = "impl Foo {}\nimpl Tr for Foo {}\n    impl<T> a::Tr<T> for Foo<T> {}\nunsafe impl<'a, F: Fn() -> u32> Tr for F {}\nimplement();\n";
        let lines: Vec<&str> = source.split('\n').collect();
        let names: Vec<(u32, &str)> = impl_trait_names(source)
            .into_iter()
            .map(|(line, column)| {
                (
                    line,
                    &lines[line as usize - 1][column as usize..column as usize + 2],
                )
            })
            .collect();
        assert_eq!(names, [(2, "Tr"), (3, "Tr"), (4, "Tr")]);
    }
}
