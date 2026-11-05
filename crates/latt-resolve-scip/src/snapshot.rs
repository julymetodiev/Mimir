//! What an index was made from.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use latt_resolve::ProjectFile;
use latt_types::ContentHash;

use crate::IndexLanguage;

/// The hash of the manifests, and of each source by path.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub manifests: String,
    pub sources: HashMap<PathBuf, String>,
}

impl Snapshot {
    pub fn text(&self) -> String {
        let mut sources: Vec<_> = self.sources.iter().collect();
        sources.sort();
        let mut text = format!("{}\n", self.manifests);
        for (path, hash) in sources {
            text.push_str(&format!("{hash} {}\n", path.display()));
        }
        text
    }

    pub fn parse(text: &str) -> Self {
        let mut lines = text.lines();
        let manifests = lines.next().unwrap_or("").to_string();
        let sources = lines
            .filter_map(|line| line.split_once(' '))
            .map(|(hash, path)| (PathBuf::from(path), hash.to_string()))
            .collect();
        Self { manifests, sources }
    }

    pub fn of<L: IndexLanguage + ?Sized>(
        language: &L,
        project: &Path,
        root: &Path,
        files: &[ProjectFile],
    ) -> Self {
        let mut sources: Vec<(&Path, ContentHash)> = files
            .iter()
            .filter(|f| f.path.starts_with(root) && language.owns(&f.path))
            .map(|f| (f.path.as_path(), ContentHash::compute(f.source.as_bytes())))
            .collect();
        let paths: Vec<&Path> = sources.iter().map(|(path, _)| *path).collect();
        let mut manifests = language.manifests(root, &paths);
        manifests.sort();
        let mut text = String::new();
        for manifest in manifests {
            if let Ok(bytes) = fs::read(project.join(&manifest)) {
                text.push_str(&format!(
                    "{} {}\n",
                    manifest.display(),
                    ContentHash::compute(&bytes).to_hex()
                ));
            }
        }
        sources.sort_by_key(|(path, _)| *path);
        Self {
            manifests: ContentHash::compute(text.as_bytes()).to_hex(),
            sources: sources
                .into_iter()
                .map(|(path, hash)| (path.to_path_buf(), hash.to_hex()))
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_snapshot_survives_being_saved() {
        let mut snapshot = Snapshot {
            manifests: "m".to_string(),
            ..Snapshot::default()
        };
        snapshot
            .sources
            .insert(PathBuf::from("a b/c.rs"), "h".to_string());
        assert_eq!(Snapshot::parse(&snapshot.text()), snapshot);
    }
}
