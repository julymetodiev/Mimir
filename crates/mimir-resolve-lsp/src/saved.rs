//! The answers beside the graph, so a new daemon need not ask again what has not changed. One line
//! at a time, in and out, so a big project needs no second copy in memory to read or write them.
//!
//! ```text
//! mimir-lsp-answers 1
//! P <hash> <name>            a configuration or lock file
//! T <path>                   a path targets name by their number, in order
//! F <hash> <path>            a file, followed by its sites
//! S <line> <column>          a site, followed by its targets
//! A <path number> <line> <name>   a target; its name is what the server marked, spaces and all
//! ```

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use mimir_resolve::{LiveDocument, Target};
use mimir_types::ContentHash;
use tracing::warn;

/// A file of another version is not read.
const HEADER: &str = "mimir-lsp-answers 1";

/// What is known of a project.
pub type Known = HashMap<PathBuf, (ContentHash, Arc<LiveDocument>)>;
/// The configuration and lock files of a project, and what they held.
pub type ProjectFiles = Vec<(PathBuf, ContentHash)>;

/// One copy of each path and name. Most targets repeat the ones before.
#[derive(Default)]
pub struct Shared {
    paths: HashMap<PathBuf, Arc<Path>>,
    names: HashSet<Arc<str>>,
}

impl Shared {
    pub fn path(&mut self, path: &Path) -> Arc<Path> {
        if let Some(known) = self.paths.get(path) {
            return known.clone();
        }
        let shared: Arc<Path> = Arc::from(path);
        self.paths.insert(path.to_path_buf(), shared.clone());
        shared
    }

    pub fn name(&mut self, name: &str) -> Arc<str> {
        if let Some(known) = self.names.get(name) {
            return known.clone();
        }
        let shared: Arc<str> = Arc::from(name);
        self.names.insert(shared.clone());
        shared
    }
}

/// What was saved, if anything, with the project files it was asked under.
pub fn read(cache_dir: &Path, name: &str) -> Option<(Known, ProjectFiles)> {
    let path = cache_dir.join(name);
    let file = File::open(&path).ok()?;
    let parsed = parse(BufReader::new(file));
    if parsed.is_none() {
        warn!("Asking again: {} cannot be read", path.display());
    }
    parsed
}

fn parse(reader: impl BufRead) -> Option<(Known, ProjectFiles)> {
    let mut lines = reader.lines();
    if lines.next()?.ok()? != HEADER {
        return None;
    }
    let (mut known, mut project) = (Known::new(), ProjectFiles::new());
    let mut paths: Vec<Arc<Path>> = Vec::new();
    let mut shared = Shared::default();
    let mut current: Option<(PathBuf, ContentHash, LiveDocument)> = None;
    let mut site: Option<(u32, u32)> = None;
    let mut finish = |current: &mut Option<(PathBuf, ContentHash, LiveDocument)>| {
        if let Some((path, hash, document)) = current.take() {
            known.insert(path, (hash, Arc::new(document)));
        }
    };
    for line in lines {
        let line = line.ok()?;
        let (kind, rest) = line.split_once(' ')?;
        match kind {
            "P" => {
                let (hash, name) = rest.split_once(' ')?;
                project.push((PathBuf::from(name), ContentHash::from_hex(hash)?));
            }
            "T" => paths.push(shared.path(Path::new(rest))),
            "F" => {
                finish(&mut current);
                let (hash, path) = rest.split_once(' ')?;
                current = Some((
                    PathBuf::from(path),
                    ContentHash::from_hex(hash)?,
                    LiveDocument::default(),
                ));
            }
            "S" => {
                let (line, column) = rest.split_once(' ')?;
                let at = (line.parse().ok()?, column.parse().ok()?);
                current.as_mut()?.2.all_sites.insert(at, Vec::new());
                site = Some(at);
            }
            "A" => {
                let mut parts = rest.splitn(3, ' ');
                let number = parts.next()?.parse::<usize>().ok()?;
                let path = paths.get(number)?.clone();
                let line = parts.next()?.parse().ok()?;
                let name = shared.name(parts.next()?);
                let targets = current.as_mut()?.2.all_sites.get_mut(&site?)?;
                targets.push(Target { path, line, name });
            }
            _ => return None,
        }
    }
    finish(&mut current);
    Some((known, project))
}

/// Whether a path can go on a line of its own, with spaces only after what comes first.
fn fits(path: &Path) -> Option<&str> {
    path.to_str().filter(|text| !text.contains('\n'))
}

/// Save what is known. Failing costs only asking again, so it is reported and goes on. A file with a
/// line break in its path is left out, and so asked about by the next daemon.
pub fn write(cache_dir: &Path, name: &str, known: &Known, project: &ProjectFiles) {
    let (path, temporary) = (cache_dir.join(name), cache_dir.join(format!("{name}.tmp")));
    let written = std::fs::create_dir_all(cache_dir)
        .and_then(|()| File::create(&temporary))
        .and_then(|file| write_to(BufWriter::new(file), known, project))
        .and_then(|()| std::fs::rename(&temporary, &path));
    if let Err(e) = written {
        warn!("Cannot save {name}: {e}");
    }
}

fn write_to(mut out: impl Write, known: &Known, project: &ProjectFiles) -> std::io::Result<()> {
    writeln!(out, "{HEADER}")?;
    for (path, hash) in project {
        if let Some(path) = fits(path) {
            writeln!(out, "P {} {path}", hash.to_hex())?;
        }
    }
    let mut numbers: HashMap<&Path, usize> = HashMap::new();
    for (_, document) in known.values() {
        for target in document.all_sites.values().flatten() {
            if !numbers.contains_key(&*target.path)
                && let Some(text) = fits(&target.path)
            {
                writeln!(out, "T {text}")?;
                numbers.insert(&target.path, numbers.len());
            }
        }
    }
    for (path, (hash, document)) in known {
        let Some(text) = fits(path) else {
            continue;
        };
        writeln!(out, "F {} {text}", hash.to_hex())?;
        for ((line, column), targets) in &document.all_sites {
            writeln!(out, "S {line} {column}")?;
            for target in targets {
                if let Some(number) = numbers.get(&*target.path) {
                    writeln!(out, "A {number} {} {}", target.line, target.name)?;
                }
            }
        }
    }
    out.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "answers.txt";

    #[test]
    fn what_is_saved_is_read_back() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut document = LiveDocument::default();
        document.all_sites.insert(
            (3, 7),
            vec![
                Target {
                    path: Arc::from(Path::new("a b/ü.ts")),
                    line: 2,
                    name: Arc::from("#run"),
                },
                // A server may mark a whole declaration when it fits on the line.
                Target {
                    path: Arc::from(Path::new("a b/ü.ts")),
                    line: 3,
                    name: Arc::from("constructor(a: string, b: number);"),
                },
            ],
        );
        let hash = ContentHash::compute(b"x");
        let known = Known::from([(PathBuf::from("c d.ts"), (hash, Arc::new(document)))]);
        let project = vec![(PathBuf::from("/a b/tsconfig.json"), hash)];

        write(dir.path(), FILE, &known, &project);
        let (read_known, read_project) = read(dir.path(), FILE).unwrap();

        assert_eq!(read_project, project);
        let (read_hash, read_document) = &read_known[Path::new("c d.ts")];
        assert_eq!(*read_hash, hash);
        let targets = &read_document.all_sites[&(3, 7)];
        assert_eq!(targets.len(), 2);
        assert_eq!(&*targets[0].name, "#run");
        assert_eq!(targets[0].line, 2);
        assert_eq!(&*targets[0].path, Path::new("a b/ü.ts"));
        assert_eq!(&*targets[1].name, "constructor(a: string, b: number);");
    }

    #[test]
    fn nothing_saved_or_another_version_gives_nothing() {
        let dir = tempfile::TempDir::new().unwrap();
        assert!(read(dir.path(), FILE).is_none());
        std::fs::write(dir.path().join(FILE), "mimir-lsp-answers 0\n").unwrap();
        assert!(read(dir.path(), FILE).is_none());
        std::fs::write(dir.path().join(FILE), format!("{HEADER}\nA 0 1 x\n")).unwrap();
        assert!(read(dir.path(), FILE).is_none());
    }

    #[test]
    fn the_same_path_and_name_are_one_copy() {
        let mut shared = Shared::default();
        let (a, b) = (shared.path(Path::new("x")), shared.path(Path::new("x")));
        assert!(Arc::ptr_eq(&a, &b));
        let (a, b) = (shared.name("x"), shared.name("x"));
        assert!(Arc::ptr_eq(&a, &b));
    }
}
