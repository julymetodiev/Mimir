//! Writing files whole or not at all: the text goes to a temporary file beside the target, reaches
//! the disk, and only then takes the target's name. A reader or a crash sees the old file or the
//! new one, never a part.

use std::fs::{self, Permissions};
use std::io::{self, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use tempfile::{Builder, NamedTempFile};

/// Hidden, so a scan passes over a temporary file that is there for a moment.
const TEMPORARY_PREFIX: &str = ".mimir-write-";
/// What a new file asks for, as `File::create` does; the umask decides the rest.
const NEW_FILE_MODE: u32 = 0o666;

/// The text on disk under a temporary name in `target`'s directory.
fn on_disk(target: &Path, content: &str, permissions: Permissions) -> io::Result<NamedTempFile> {
    let directory = target.parent().unwrap_or(Path::new("."));
    let mut file = Builder::new()
        .prefix(TEMPORARY_PREFIX)
        .permissions(permissions)
        .tempfile_in(directory)?;
    file.write_all(content.as_bytes())?;
    file.as_file().sync_all()?;
    Ok(file)
}

/// A replacement ready to take its target's name.
fn prepared(path: &Path, content: &str) -> io::Result<(NamedTempFile, PathBuf)> {
    // Through a link to the file it leads to, or the link itself would be replaced.
    let target = path.canonicalize()?;
    let permissions = fs::metadata(&target)?.permissions();
    // Renaming needs a writable directory, not a writable file: without this a read-only file
    // would be replaced where a plain write is refused.
    if permissions.readonly() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("{} is read-only", target.display()),
        ));
    }
    let file = on_disk(&target, content, permissions.clone())?;
    // The umask may have narrowed them.
    file.as_file().set_permissions(permissions)?;
    Ok((file, target))
}

/// Replace existing files. Every text is on disk before the first file changes, so a full disk or a
/// read-only file changes none of them.
pub fn replace_all(files: &[(PathBuf, String)]) -> io::Result<()> {
    let ready = files
        .iter()
        .map(|(path, content)| prepared(path, content))
        .collect::<io::Result<Vec<_>>>()?;
    for (file, target) in ready {
        file.persist(&target).map_err(|e| e.error)?;
    }
    Ok(())
}

/// Make a file that must not exist yet: anything there, or appearing meanwhile, stays as it is.
pub fn create(path: &Path, content: &str) -> io::Result<()> {
    on_disk(path, content, Permissions::from_mode(NEW_FILE_MODE))?
        .persist_noclobber(path)
        .map_err(|e| e.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::os::unix::fs::symlink;
    use tempfile::tempdir;

    fn mode(path: &Path) -> u32 {
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn test_a_reader_already_in_the_file_keeps_the_whole_old_text() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("file.txt");
        fs::write(&path, "old\n").unwrap();
        let mut reader = fs::File::open(&path).unwrap();

        replace_all(&[(path.clone(), "new text\n".to_string())]).unwrap();

        let mut seen = String::new();
        reader.read_to_string(&mut seen).unwrap();
        assert_eq!(seen, "old\n");
        assert_eq!(fs::read_to_string(&path).unwrap(), "new text\n");
    }

    #[test]
    fn test_replacing_keeps_the_mode_and_leaves_nothing_behind() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("run.sh");
        fs::write(&path, "old\n").unwrap();
        fs::set_permissions(&path, Permissions::from_mode(0o755)).unwrap();

        replace_all(&[(path.clone(), "new\n".to_string())]).unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "new\n");
        assert_eq!(mode(&path), 0o755);
        assert_eq!(names(dir.path()), ["run.sh"]);
    }

    #[test]
    fn test_replacing_through_a_link_keeps_the_link() {
        let dir = tempdir().unwrap();
        let (real, link) = (dir.path().join("real.txt"), dir.path().join("link.txt"));
        fs::write(&real, "old\n").unwrap();
        symlink(&real, &link).unwrap();

        replace_all(&[(link.clone(), "new\n".to_string())]).unwrap();

        assert_eq!(fs::read_to_string(&real).unwrap(), "new\n");
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    #[test]
    fn test_a_read_only_file_changes_nothing_in_the_batch() {
        let dir = tempdir().unwrap();
        let (open, locked) = (dir.path().join("open.txt"), dir.path().join("locked.txt"));
        fs::write(&open, "old\n").unwrap();
        fs::write(&locked, "old\n").unwrap();
        fs::set_permissions(&locked, Permissions::from_mode(0o444)).unwrap();

        let batch = [
            (open.clone(), "new\n".to_string()),
            (locked.clone(), "new\n".to_string()),
        ];
        assert!(replace_all(&batch).is_err());

        assert_eq!(fs::read_to_string(&open).unwrap(), "old\n");
        assert_eq!(fs::read_to_string(&locked).unwrap(), "old\n");
        assert_eq!(names(dir.path()), ["locked.txt", "open.txt"]);
    }

    #[test]
    fn test_creating_leaves_what_is_there_and_gets_a_normal_mode() {
        let dir = tempdir().unwrap();
        let (plain, path) = (dir.path().join("plain.txt"), dir.path().join("new.txt"));
        fs::write(&plain, "plain\n").unwrap();

        create(&path, "first\n").unwrap();
        assert_eq!(mode(&path), mode(&plain));

        let again = create(&path, "second\n").unwrap_err();
        assert_eq!(again.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read_to_string(&path).unwrap(), "first\n");
        assert_eq!(names(dir.path()), ["new.txt", "plain.txt"]);
    }
}
