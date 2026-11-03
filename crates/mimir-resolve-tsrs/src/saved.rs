//! The answers beside the graph, so a new daemon need not ask again what has not changed.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use mimir_resolve::{LiveDocument, Target};
use mimir_types::ContentHash;
use serde_json::{Value, json};
use tracing::warn;

/// A file of another version is not read.
const VERSION: u64 = 1;
const FILE: &str = "tsrs-answers.json";

/// What is known of a project.
pub type Known = HashMap<PathBuf, (ContentHash, LiveDocument)>;
/// The configuration and lock files of a project, and what they held.
pub type ProjectFiles = Vec<(PathBuf, ContentHash)>;

/// What was saved, if anything, with the project files it was asked under.
pub fn read(cache_dir: &Path) -> Option<(Known, ProjectFiles)> {
    let path = cache_dir.join(FILE);
    let bytes = std::fs::read(&path).ok()?;
    let parsed = parse(&serde_json::from_slice(&bytes).ok()?);
    if parsed.is_none() {
        warn!("Asking again: {} cannot be read", path.display());
    }
    parsed
}

fn hash(value: &Value) -> Option<ContentHash> {
    serde_json::from_value(value.clone()).ok()
}

fn parse(saved: &Value) -> Option<(Known, ProjectFiles)> {
    if saved["version"].as_u64() != Some(VERSION) {
        return None;
    }
    let project = saved["project"]
        .as_array()?
        .iter()
        .map(|entry| Some((PathBuf::from(entry[0].as_str()?), hash(&entry[1])?)))
        .collect::<Option<ProjectFiles>>()?;
    let mut known = Known::new();
    for (path, file) in saved["files"].as_object()? {
        let mut document = LiveDocument::default();
        for site in file["sites"].as_array()? {
            let targets = site[2]
                .as_array()?
                .iter()
                .map(|t| {
                    Some(Target {
                        path: PathBuf::from(t[0].as_str()?),
                        line: t[1].as_u64()? as u32,
                        name: t[2].as_str()?.to_string(),
                    })
                })
                .collect::<Option<Vec<Target>>>()?;
            let at = (site[0].as_u64()? as u32, site[1].as_u64()? as u32);
            document.all_sites.insert(at, targets);
        }
        known.insert(PathBuf::from(path), (hash(&file["hash"])?, document));
    }
    Some((known, project))
}

/// Save what is known. Failing costs only asking again, so it is reported and goes on.
pub fn write(cache_dir: &Path, known: &Known, project: &ProjectFiles) {
    let files: serde_json::Map<String, Value> = known
        .iter()
        .map(|(path, (hash, document))| {
            let sites: Vec<Value> = document
                .all_sites
                .iter()
                .map(|((line, column), targets)| {
                    let targets: Vec<Value> = targets
                        .iter()
                        .map(|t| json!([t.path, t.line, t.name]))
                        .collect();
                    json!([line, column, targets])
                })
                .collect();
            (
                path.to_string_lossy().into_owned(),
                json!({"hash": hash, "sites": sites}),
            )
        })
        .collect();
    let saved = json!({"version": VERSION, "project": project, "files": files});
    let (path, temporary) = (cache_dir.join(FILE), cache_dir.join(format!("{FILE}.tmp")));
    let written = std::fs::create_dir_all(cache_dir)
        .and_then(|()| std::fs::write(&temporary, saved.to_string()))
        .and_then(|()| std::fs::rename(&temporary, &path));
    if let Err(e) = written {
        warn!("Cannot save the answers of tsrs: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_saved_is_read_back() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut document = LiveDocument::default();
        document.all_sites.insert(
            (3, 7),
            vec![Target {
                path: PathBuf::from("a b/ü.ts"),
                line: 2,
                name: "run".to_string(),
            }],
        );
        document.all_sites.insert((4, 0), Vec::new());
        let hash = ContentHash::compute(b"x");
        let known = Known::from([(PathBuf::from("c.ts"), (hash, document))]);
        let project = vec![(PathBuf::from("tsconfig.json"), hash)];

        write(dir.path(), &known, &project);
        let (read_known, read_project) = read(dir.path()).unwrap();

        assert_eq!(read_project, project);
        let (read_hash, read_document) = &read_known[Path::new("c.ts")];
        assert_eq!(*read_hash, hash);
        assert_eq!(read_document.all_sites[&(3, 7)][0].name, "run");
        assert_eq!(read_document.all_sites[&(3, 7)][0].line, 2);
        assert!(read_document.all_sites[&(4, 0)].is_empty());
    }

    #[test]
    fn nothing_saved_or_another_version_gives_nothing() {
        let dir = tempfile::TempDir::new().unwrap();
        assert!(read(dir.path()).is_none());
        std::fs::write(dir.path().join(FILE), r#"{"version":0}"#).unwrap();
        assert!(read(dir.path()).is_none());
        std::fs::write(dir.path().join(FILE), "not json").unwrap();
        assert!(read(dir.path()).is_none());
    }
}
