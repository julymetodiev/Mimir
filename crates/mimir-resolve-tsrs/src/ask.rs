//! Asking `tsrs` about the sites of a file, and which files to ask about after a change.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use mimir_lsp::{Location, LspError, Server};
use mimir_parse_core::ExtractedReference;
use mimir_resolve::{LiveDocument, ProjectFile, Target, resolve_import};
use mimir_types::{FileId, Language, ReferenceKind};
use tracing::debug;

use crate::saved::Known;

/// How many times an answer on an import is asked again.
const MAX_STEPS: usize = 8;

/// The changed files, and each file that imports one of them, directly or not, or has an answer in
/// one. A changed type can change what `x.y()` means further away than the next import.
pub fn with_importers(
    scripts: &[&ProjectFile],
    changed: &HashSet<&Path>,
    known: &Known,
) -> Vec<PathBuf> {
    let by_path: HashMap<&Path, FileId> = scripts
        .iter()
        .map(|f| (f.path.as_path(), f.file_id))
        .collect();
    let by_id: HashMap<FileId, &Path> = scripts
        .iter()
        .map(|f| (f.file_id, f.path.as_path()))
        .collect();
    let mut users: HashMap<&Path, HashSet<&Path>> = HashMap::new();
    for file in scripts {
        for reference in &file.references {
            if reference.kind == ReferenceKind::Import
                && let Some(path) = &reference.context
                && let Some(id) = resolve_import(&file.path, path, &by_path)
                && let Some(used) = by_id.get(&id)
            {
                users.entry(*used).or_default().insert(file.path.as_path());
            }
        }
        if let Some((_, document)) = known.get(&file.path) {
            for target in document.all_sites.values().flatten() {
                if let Some((used, _)) = by_path.get_key_value(target.path.as_path()) {
                    users.entry(*used).or_default().insert(file.path.as_path());
                }
            }
        }
    }
    let mut stale: HashSet<&Path> = changed.clone();
    let mut queue: Vec<&Path> = changed.iter().copied().collect();
    while let Some(path) = queue.pop() {
        for user in users.get(path).into_iter().flatten() {
            if stale.insert(user) {
                queue.push(user);
            }
        }
    }
    let mut stale: Vec<PathBuf> = stale.into_iter().map(Path::to_path_buf).collect();
    // The order is fixed, and the edited ones first, for the first answer.
    stale.sort_by_key(|path| (!changed.contains(path.as_path()), path.clone()));
    stale
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

/// The answers for these files, each opened only while it is asked about.
pub fn answers(
    server: &mut Server,
    project: &Path,
    files: &[ProjectFile],
    stale: &[PathBuf],
) -> Result<HashMap<PathBuf, LiveDocument>, LspError> {
    let by_path: HashMap<&Path, &ProjectFile> =
        files.iter().map(|f| (f.path.as_path(), f)).collect();
    let import_lines: HashMap<&Path, HashSet<u32>> = files
        .iter()
        .map(|f| {
            let lines = f
                .references
                .iter()
                .filter(|r| r.kind == ReferenceKind::Import)
                .map(|r| r.range.start_line)
                .collect();
            (f.path.as_path(), lines)
        })
        .collect();
    let asking = Asking {
        project,
        by_path: &by_path,
        import_lines: &import_lines,
    };
    let mut documents = HashMap::new();
    for path in stale {
        let Some(file) = by_path.get(path.as_path()) else {
            continue;
        };
        let absolute = project.join(path);
        server.update(&absolute, language_id(path), &file.source)?;
        let document = asking.document(server, &absolute, file)?;
        server.close(&absolute)?;
        documents.insert(path.clone(), document);
    }
    Ok(documents)
}

fn is_asked(reference: &ExtractedReference) -> bool {
    matches!(
        reference.kind,
        ReferenceKind::Call
            | ReferenceKind::FunctionValue
            | ReferenceKind::TypeUse
            | ReferenceKind::FieldUse
            | ReferenceKind::FieldWrite
    )
}

struct Asking<'a> {
    project: &'a Path,
    by_path: &'a HashMap<&'a Path, &'a ProjectFile>,
    import_lines: &'a HashMap<&'a Path, HashSet<u32>>,
}

impl Asking<'_> {
    /// The answers for each site of an open file.
    fn document(
        &self,
        server: &mut Server,
        path: &Path,
        file: &ProjectFile,
    ) -> Result<LiveDocument, LspError> {
        let mut seen = HashSet::new();
        let asked: Vec<&ExtractedReference> = file
            .references
            .iter()
            .filter(|r| is_asked(r) && seen.insert((r.range.start_line, r.range.start_col)))
            .collect();
        let zero_based = |r: &ExtractedReference| (r.range.start_line - 1, r.range.start_col);
        let sites: Vec<(u32, u32)> = asked.iter().map(|r| zero_based(r)).collect();
        let mut found = server.many("textDocument/definition", path, &sites)?;

        // On `Circle | Square` a definition is one member's alone, and each may run.
        let with_receiver: Vec<(usize, (u32, u32))> = asked
            .iter()
            .enumerate()
            .filter_map(|(i, r)| r.receiver_at.map(|(line, column)| (i, (line - 1, column))))
            .collect();
        let receivers: Vec<(u32, u32)> = with_receiver.iter().map(|(_, at)| *at).collect();
        let types = server.many("textDocument/typeDefinition", path, &receivers)?;
        let several: Vec<usize> = with_receiver
            .iter()
            .zip(&types)
            // `Promise` is declared in several files of the standard library, and none of that
            // gives an edge. Finding what runs is what takes time, here 10 ms a site.
            .filter(|(_, types)| types.len() > 1 && types.iter().any(|t| self.is_inside(t)))
            .map(|((i, _), _)| *i)
            .collect();
        let at_several: Vec<(u32, u32)> = several.iter().map(|i| sites[*i]).collect();
        let members = server.many("textDocument/implementation", path, &at_several)?;
        for (i, members) in several.into_iter().zip(members) {
            found[i].extend(members);
        }

        debug!(
            "{}: {} sites, {} receivers, {} with several types",
            file.path.display(),
            sites.len(),
            receivers.len(),
            at_several.len()
        );
        let mut document = LiveDocument::default();
        for (reference, locations) in asked.iter().zip(found) {
            let mut targets = Vec::new();
            for location in locations {
                self.follow(server, location, &mut targets)?;
            }
            let site = (reference.range.start_line, reference.range.start_col);
            document.all_sites.insert(site, targets);
        }
        Ok(document)
    }

    /// Where an answer leads: an answer on an import is asked again from there, since the edge
    /// must go to the definition. One that does not settle is left out.
    fn follow(
        &self,
        server: &mut Server,
        mut location: Location,
        targets: &mut Vec<Target>,
    ) -> Result<(), LspError> {
        for _ in 0..MAX_STEPS {
            let Ok(relative) = location.path.strip_prefix(self.project) else {
                return Ok(());
            };
            let on_import = self
                .import_lines
                .get(relative)
                .is_some_and(|lines| lines.contains(&(location.line + 1)));
            if !on_import {
                if let Some(target) = self.target(&location)
                    && !targets.contains(&target)
                {
                    targets.push(target);
                }
                return Ok(());
            }
            let next = server
                .definition(&location.path, location.line, location.column)?
                .into_iter()
                .next();
            match next {
                Some(next) if (&next.path, next.line) != (&location.path, location.line) => {
                    location = next;
                }
                _ => return Ok(()),
            }
        }
        Ok(())
    }

    fn is_inside(&self, found: &Location) -> bool {
        found
            .path
            .strip_prefix(self.project)
            .is_ok_and(|path| self.by_path.contains_key(path))
    }

    fn target(&self, found: &Location) -> Option<Target> {
        let path = found.path.strip_prefix(self.project).ok()?;
        let file = self.by_path.get(path)?;
        let name = file
            .source
            .split('\n')
            .nth(found.line as usize)?
            .get(found.column as usize..found.end_column as usize)?
            .to_string();
        Some(Target {
            path: path.to_path_buf(),
            line: found.line + 1,
            name,
        })
    }
}
