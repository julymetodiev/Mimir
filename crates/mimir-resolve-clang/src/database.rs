//! The compilation database: its place, and what a person is told when there is none.

use std::path::{Path, PathBuf};

const NAME: &str = "compile_commands.json";
const BUILD: &str = "build";

/// The database of a project, relative to it: in its root, or else in `build/`.
pub fn find(project: &Path) -> Option<PathBuf> {
    [PathBuf::from(NAME), Path::new(BUILD).join(NAME)]
        .into_iter()
        .find(|path| project.join(path).is_file())
}

/// Mimir does not run these itself, as each runs the project's code.
pub fn missing_note() -> String {
    format!(
        "C and C++: syntax rules only, as there is no {NAME} in the project or in {BUILD}/. \
         With CMake: cmake -B build -DCMAKE_EXPORT_COMPILE_COMMANDS=ON. With Make: bear -- make."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_root_goes_before_build() {
        let project = tempfile::tempdir().unwrap();
        assert_eq!(find(project.path()), None);
        std::fs::create_dir(project.path().join(BUILD)).unwrap();
        std::fs::write(project.path().join(BUILD).join(NAME), "[]").unwrap();
        assert_eq!(find(project.path()), Some(Path::new(BUILD).join(NAME)));
        std::fs::write(project.path().join(NAME), "[]").unwrap();
        assert_eq!(find(project.path()), Some(PathBuf::from(NAME)));
    }
}
