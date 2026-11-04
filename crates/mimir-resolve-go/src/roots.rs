//! Which directories are roots: a directory with a `go.mod`, nested ones too, and no limit on them.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use mimir_resolve::ProjectFile;
use mimir_types::Language;

/// A module with Go files of its own.
pub struct Root {
    /// Relative to the project.
    pub path: PathBuf,
    pub files: usize,
}

/// The roots of a project, and which each file belongs to.
pub struct Roots<'a> {
    /// The fewest files first.
    pub roots: Vec<Root>,
    pub owners: HashMap<&'a Path, PathBuf>,
}

/// The innermost `go.mod` above each Go file. A file with none has no module, and syntax rules.
pub fn of_files<'a>(project: &Path, files: &'a [ProjectFile]) -> Roots<'a> {
    let mut known: HashMap<&'a Path, Option<&'a Path>> = HashMap::new();
    let mut owners = HashMap::new();
    let mut counts: HashMap<&'a Path, usize> = HashMap::new();
    for file in files
        .iter()
        .filter(|f| Language::from_path(&f.path) == Language::Go)
    {
        let directory = file.path.parent().unwrap_or(Path::new(""));
        let root = *known.entry(directory).or_insert_with(|| {
            directory
                .ancestors()
                .find(|dir| project.join(dir).join("go.mod").is_file())
        });
        if let Some(root) = root {
            owners.insert(file.path.as_path(), root.to_path_buf());
            *counts.entry(root).or_default() += 1;
        }
    }
    let mut roots: Vec<Root> = counts
        .into_iter()
        .map(|(path, files)| Root {
            path: path.to_path_buf(),
            files,
        })
        .collect();
    roots.sort_by(|a, b| (a.files, &a.path).cmp(&(b.files, &b.path)));
    Roots { roots, owners }
}

/// The module's path in a `go.mod`.
pub fn module_path(go_mod: &Path) -> Option<String> {
    let text = std::fs::read_to_string(go_mod).ok()?;
    text.lines().find_map(|line| {
        let line = line.split("//").next().unwrap_or(line);
        let path = line.trim().strip_prefix("module")?;
        path.starts_with([' ', '\t'])
            .then(|| path.trim().trim_matches(['"', '`']).to_string())
    })
}

/// What a change in means a new index for this root: its own module, and a `go.work` above it.
pub fn manifests(root: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = ["go.mod", "go.sum"].map(|name| root.join(name)).to_vec();
    for directory in root.ancestors() {
        found.push(directory.join("go.work"));
        found.push(directory.join("go.work.sum"));
    }
    found
}

/// A root as a person reads it.
pub fn shown(root: &Path) -> String {
    if root.as_os_str().is_empty() {
        "the top of the project".to_string()
    } else {
        root.display().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::Arc;

    fn file(path: &str) -> ProjectFile {
        ProjectFile {
            file_id: mimir_types::FileId::new(1),
            path: PathBuf::from(path),
            source: Arc::from(""),
            references: Vec::new(),
        }
    }

    #[test]
    fn each_module_is_a_root_the_fewest_files_first() {
        let project = tempfile::tempdir().unwrap();
        for dir in ["", "nested/deep", "other"] {
            fs::create_dir_all(project.path().join(dir)).unwrap();
            fs::write(project.path().join(dir).join("go.mod"), "module x\n").unwrap();
        }
        let files = [
            file("a.go"),
            file("inner/b.go"),
            file("inner/c.go"),
            file("nested/deep/d.go"),
            file("nested/e.go"),
            file("other/f.go"),
            file("other/g.go"),
            file("h.rs"),
        ];
        let found = of_files(project.path(), &files);
        let roots: Vec<(&Path, usize)> = found
            .roots
            .iter()
            .map(|r| (r.path.as_path(), r.files))
            .collect();
        assert_eq!(
            roots,
            [
                (Path::new("nested/deep"), 1),
                (Path::new("other"), 2),
                (Path::new(""), 4)
            ]
        );
        assert_eq!(found.owners[Path::new("nested/e.go")], Path::new(""));
        assert!(!found.owners.contains_key(Path::new("h.rs")));
    }

    #[test]
    fn a_file_with_no_module_above_has_no_root() {
        let project = tempfile::tempdir().unwrap();
        let files = [file("a.go")];
        let found = of_files(project.path(), &files);
        assert!(found.roots.is_empty());
        assert!(found.owners.is_empty());
    }

    #[test]
    fn reads_the_path_of_a_module() {
        let project = tempfile::tempdir().unwrap();
        let go_mod = project.path().join("go.mod");
        for (text, path) in [
            (
                "// x\nmodule example.com/a // y\n\ngo 1.27\n",
                Some("example.com/a"),
            ),
            ("module \"example.com/b\"\n", Some("example.com/b")),
            ("modules x\n", None),
            ("", None),
        ] {
            fs::write(&go_mod, text).unwrap();
            assert_eq!(module_path(&go_mod).as_deref(), path, "{text}");
        }
    }

    #[test]
    fn a_go_work_above_a_root_is_a_manifest() {
        let manifests = manifests(Path::new("a/b"));
        for wanted in [
            "a/b/go.mod",
            "a/b/go.sum",
            "a/b/go.work",
            "a/go.work",
            "go.work",
            "go.work.sum",
        ] {
            assert!(manifests.contains(&PathBuf::from(wanted)), "{wanted}");
        }
        assert!(!manifests.contains(&PathBuf::from("a/go.mod")));
    }
}
