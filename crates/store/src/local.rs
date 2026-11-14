//! [`LocalBackend`]: a directory on disk, the zero-setup default.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::StoreError;
use crate::backend::{Backend, normalise_prefix, validate_key};

/// Prefix of in-flight temporary files. Listing skips anything starting with
/// a dot, which valid keys never produce at the top of a segment we write.
const TEMP_PREFIX: &str = ".inloop-tmp-";

/// Stores each key as a file under a root directory.
///
/// Writes are atomic (a temporary file in the same directory, synced, then
/// renamed over the target), so a crash never leaves a half-written blob. On
/// unix, directories are created `0700` and files `0600`.
#[derive(Debug, Clone)]
pub struct LocalBackend {
    root: PathBuf,
}

impl LocalBackend {
    /// A backend rooted at `dir`. The directory is created on first write.
    pub fn new(dir: PathBuf) -> LocalBackend {
        LocalBackend { root: dir }
    }

    /// The root directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The file a key maps to, after validating the key.
    pub fn path_for(&self, key: &str) -> Result<PathBuf, StoreError> {
        validate_key(key)?;
        let mut path = self.root.clone();
        for segment in key.split('/') {
            path.push(segment);
        }
        Ok(path)
    }
}

/// Creates `dir` and any missing parents, `0700` on unix.
pub(crate) fn create_private_dir(dir: &Path) -> Result<(), StoreError> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(dir)
        .map_err(|e| StoreError::io("create the directory", dir, e))
}

impl Backend for LocalBackend {
    fn put(&self, key: &str, bytes: &[u8]) -> Result<(), StoreError> {
        let path = self.path_for(key)?;
        let dir = path.parent().unwrap_or(&self.root).to_path_buf();
        create_private_dir(&dir)?;
        // tempfile creates the file 0600 on unix.
        let mut tmp = tempfile::Builder::new()
            .prefix(TEMP_PREFIX)
            .tempfile_in(&dir)
            .map_err(|e| StoreError::io("create a temporary file in", &dir, e))?;
        tmp.write_all(bytes)
            .and_then(|()| tmp.as_file().sync_all())
            .map_err(|e| StoreError::io("write", tmp.path().to_path_buf(), e))?;
        tmp.persist(&path)
            .map_err(|e| StoreError::io("move into place", &path, e.error))?;
        Ok(())
    }

    fn get(&self, key: &str) -> Result<Option<Vec<u8>>, StoreError> {
        let path = self.path_for(key)?;
        match fs::read(&path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(StoreError::io("read", path, e)),
        }
    }

    fn delete(&self, key: &str) -> Result<(), StoreError> {
        let path = self.path_for(key)?;
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(StoreError::io("delete", path, e)),
        }
    }

    fn list(&self, prefix: &str) -> Result<Vec<String>, StoreError> {
        let prefix = normalise_prefix(prefix)?;
        let start = if prefix.is_empty() {
            self.root.clone()
        } else {
            self.path_for(prefix)?
        };
        let mut keys = Vec::new();
        walk(&start, prefix, &mut keys)?;
        keys.sort();
        Ok(keys)
    }

    fn describe(&self) -> String {
        format!("local {}", self.root.display())
    }
}

/// Collects every file under `dir` as a key starting with `key_prefix`.
fn walk(dir: &Path, key_prefix: &str, keys: &mut Vec<String>) -> Result<(), StoreError> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        // A missing directory, or a prefix that names a file, lists nothing.
        Err(e)
            if matches!(
                e.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
            ) =>
        {
            return Ok(());
        }
        Err(e) => return Err(StoreError::io("list", dir, e)),
    };
    for entry in entries {
        let entry = entry.map_err(|e| StoreError::io("list", dir, e))?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue; // Not UTF-8, so not something we wrote.
        };
        if name.starts_with('.') {
            continue; // Temporary files and other hidden entries.
        }
        let key = if key_prefix.is_empty() {
            name
        } else {
            format!("{key_prefix}/{name}")
        };
        if validate_key(&key).is_err() {
            continue; // Something else put a file here; not one of ours.
        }
        let file_type = entry
            .file_type()
            .map_err(|e| StoreError::io("inspect", entry.path(), e))?;
        if file_type.is_dir() {
            walk(&entry.path(), &key, keys)?;
        } else if file_type.is_file() {
            keys.push(key);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn round_trip_delete_and_list() -> TestResult {
        let dir = tempfile::tempdir()?;
        let backend = LocalBackend::new(dir.path().join("store"));
        assert_eq!(backend.get("a/b")?, None);
        assert_eq!(backend.list("")?, Vec::<String>::new());

        backend.put("a/b", b"one")?;
        backend.put("a/c/d", b"two")?;
        backend.put("ab", b"three")?;
        assert_eq!(backend.get("a/b")?.as_deref(), Some(&b"one"[..]));

        backend.put("a/b", b"replaced")?;
        assert_eq!(backend.get("a/b")?.as_deref(), Some(&b"replaced"[..]));

        assert_eq!(backend.list("")?, vec!["a/b", "a/c/d", "ab"]);
        assert_eq!(backend.list("a")?, vec!["a/b", "a/c/d"]);
        assert_eq!(backend.list("a/")?, vec!["a/b", "a/c/d"]);
        assert_eq!(backend.list("a/c")?, vec!["a/c/d"]);
        assert_eq!(backend.list("missing")?, Vec::<String>::new());
        assert_eq!(backend.list("ab")?, Vec::<String>::new());

        let file = backend.path_for("a/b")?;
        assert!(file.exists());
        backend.delete("a/b")?;
        assert!(!file.exists());
        assert_eq!(backend.get("a/b")?, None);
        backend.delete("a/b")?; // Missing is fine.
        Ok(())
    }

    #[test]
    fn rejects_escaping_keys() {
        let backend = LocalBackend::new(PathBuf::from("/nonexistent"));
        for key in ["../x", "/etc/passwd", "a\\b", "a/../../b"] {
            assert!(backend.put(key, b"x").is_err(), "{key}");
            assert!(backend.get(key).is_err(), "{key}");
            assert!(backend.delete(key).is_err(), "{key}");
        }
        assert!(backend.list("../").is_err());
    }

    #[test]
    fn skips_temporary_and_foreign_files() -> TestResult {
        let dir = tempfile::tempdir()?;
        let backend = LocalBackend::new(dir.path().to_path_buf());
        backend.put("blobs/x", b"x")?;
        fs::write(dir.path().join("blobs").join(".inloop-tmp-abc"), b"partial")?;
        fs::write(dir.path().join("blobs").join("has space"), b"foreign")?;
        assert_eq!(backend.list("")?, vec!["blobs/x"]);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn private_permissions() -> TestResult {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir()?;
        let root = dir.path().join("store");
        let backend = LocalBackend::new(root.clone());
        backend.put("blobs/x", b"x")?;
        let mode =
            |p: &Path| -> io::Result<u32> { Ok(fs::metadata(p)?.permissions().mode() & 0o777) };
        assert_eq!(mode(&root)?, 0o700);
        assert_eq!(mode(&root.join("blobs"))?, 0o700);
        assert_eq!(mode(&root.join("blobs/x"))?, 0o600);
        Ok(())
    }

    #[test]
    fn describes_itself() {
        let backend = LocalBackend::new(PathBuf::from("/tmp/store"));
        assert_eq!(backend.describe(), "local /tmp/store");
    }
}
