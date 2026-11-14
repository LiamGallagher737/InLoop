//! [`ProjectKey`]: a project's age X25519 key.

use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use age::x25519;
use secrecy::{ExposeSecret, SecretString};

use crate::StoreError;
use crate::local::create_private_dir;

/// A project's age key (X25519). Sessions are encrypted to its recipient and
/// decrypted with its identity.
///
/// The identity never appears in `Debug`, logs or errors; `Debug` shows only
/// the public recipient. The only way to get the secret out is
/// [`ProjectKey::export_secret`], which returns a [`SecretString`].
pub struct ProjectKey {
    identity: x25519::Identity,
}

impl fmt::Debug for ProjectKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProjectKey")
            .field("recipient", &self.recipient())
            .finish_non_exhaustive()
    }
}

impl ProjectKey {
    /// Generates a fresh key from the operating system's random source.
    pub fn generate() -> ProjectKey {
        ProjectKey {
            identity: x25519::Identity::generate(),
        }
    }

    /// The public recipient, `age1...`. Safe to show and share.
    pub fn recipient(&self) -> String {
        self.identity.to_public().to_string()
    }

    pub(crate) fn identity(&self) -> &x25519::Identity {
        &self.identity
    }

    pub(crate) fn age_recipient(&self) -> x25519::Recipient {
        self.identity.to_public()
    }

    /// Loads the key saved at `path`, or `None` if there is no file there.
    ///
    /// The file is in age-keygen's format: `#` comment lines and one
    /// `AGE-SECRET-KEY-1...` line.
    pub fn load(path: &Path) -> Result<Option<ProjectKey>, StoreError> {
        let contents = match fs::read_to_string(path) {
            Ok(contents) => SecretString::from(contents),
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) if e.kind() == io::ErrorKind::InvalidData => {
                return Err(StoreError::KeyFile(path.to_path_buf()));
            }
            Err(e) => return Err(StoreError::io("read the project key", path, e)),
        };
        let mut lines = contents
            .expose_secret()
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'));
        let (Some(line), None) = (lines.next(), lines.next()) else {
            return Err(StoreError::KeyFile(path.to_path_buf()));
        };
        let identity = x25519::Identity::from_str(line)
            .map_err(|_| StoreError::KeyFile(path.to_path_buf()))?;
        Ok(Some(ProjectKey { identity }))
    }

    /// Saves the key to `path`, creating parent directories (`0700` on unix).
    ///
    /// The file is created new with mode `0600`; if anything already exists at
    /// `path` this fails with [`StoreError::KeyExists`] and leaves it alone.
    pub fn save(&self, path: &Path) -> Result<(), StoreError> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            create_private_dir(parent)?;
        }
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = match options.open(path) {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                return Err(StoreError::KeyExists(path.to_path_buf()));
            }
            Err(e) => return Err(StoreError::io("create the project key file", path, e)),
        };
        let secret = self.export_secret();
        let contents = SecretString::from(format!(
            "# InLoop project key. Keep it secret; share it only with people who may read this project's sessions.\n# public key: {}\n{}\n",
            self.recipient(),
            secret.expose_secret()
        ));
        let written = file
            .write_all(contents.expose_secret().as_bytes())
            .and_then(|()| file.sync_all());
        if let Err(e) = written {
            drop(file);
            // Don't leave a truncated key behind for `load` to choke on.
            let _ = fs::remove_file(path);
            return Err(StoreError::io("write the project key file", path, e));
        }
        Ok(())
    }

    /// Loads the key at `path`, or generates and saves a new one if there is
    /// none. Returns the key and whether it was created.
    pub fn load_or_create(path: &Path) -> Result<(ProjectKey, bool), StoreError> {
        if let Some(key) = ProjectKey::load(path)? {
            return Ok((key, false));
        }
        let key = ProjectKey::generate();
        match key.save(path) {
            Ok(()) => Ok((key, true)),
            // Another process created it between our load and save: use theirs.
            Err(StoreError::KeyExists(_)) => match ProjectKey::load(path)? {
                Some(key) => Ok((key, false)),
                None => Err(StoreError::KeyFile(path.to_path_buf())),
            },
            Err(e) => Err(e),
        }
    }

    /// The `AGE-SECRET-KEY-1...` line, for `inloop key export`.
    pub fn export_secret(&self) -> SecretString {
        self.identity.to_string()
    }

    /// Parses a key from an `AGE-SECRET-KEY-1...` line, as printed by
    /// [`ProjectKey::export_secret`]. Surrounding whitespace and `#` comment
    /// lines (a whole key file pasted in) are ignored. The input never
    /// appears in the error.
    pub fn import(secret: &str) -> Result<ProjectKey, StoreError> {
        let mut lines = secret
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'));
        let (Some(line), None) = (lines.next(), lines.next()) else {
            return Err(StoreError::KeyImport);
        };
        let identity = x25519::Identity::from_str(line).map_err(|_| StoreError::KeyImport)?;
        Ok(ProjectKey { identity })
    }
}

/// Where a project's key lives: `<config_dir>/keys/<sanitised project>.agekey`.
///
/// The project name is sanitised to letters, digits, `.`, `-` and `_` (other
/// characters become `_`, a leading `.` becomes `_`), so it can never escape
/// the `keys` directory. An empty name becomes `default`.
pub fn key_path(config_dir: &Path, project: &str) -> PathBuf {
    let mut name: String = project
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if name.starts_with('.') {
        name.replace_range(0..1, "_");
    }
    if name.is_empty() {
        name.push_str("default");
    }
    config_dir.join("keys").join(format!("{name}.agekey"))
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn export_import_round_trip() -> TestResult {
        let key = ProjectKey::generate();
        assert!(key.recipient().starts_with("age1"));
        let secret = key.export_secret();
        assert!(secret.expose_secret().starts_with("AGE-SECRET-KEY-1"));
        let again = ProjectKey::import(&format!("  {}\n", secret.expose_secret()))?;
        assert_eq!(again.recipient(), key.recipient());
        Ok(())
    }

    #[test]
    fn import_rejects_garbage_without_echoing_it() {
        for bad in ["", "nope", "AGE-SECRET-KEY-1NOTREAL", "age1abc", "a\nb"] {
            let err = ProjectKey::import(bad).err();
            assert!(matches!(err, Some(StoreError::KeyImport)), "{bad:?}");
        }
        let err = ProjectKey::import("AGE-SECRET-KEY-1SHOULDNOTAPPEAR").err();
        let shown = format!(
            "{err:?} {}",
            err.as_ref().map(ToString::to_string).unwrap_or_default()
        );
        assert!(!shown.contains("SHOULDNOTAPPEAR"), "{shown}");
    }

    #[test]
    fn debug_shows_no_secret() {
        let key = ProjectKey::generate();
        let secret = key.export_secret();
        let debug = format!("{key:?}");
        assert!(!debug.contains(secret.expose_secret()), "{debug}");
        assert!(!debug.to_uppercase().contains("AGE-SECRET-KEY"), "{debug}");
        assert!(debug.contains(&key.recipient()), "{debug}");
    }

    #[test]
    fn save_load_and_refuse_to_overwrite() -> TestResult {
        let dir = tempfile::tempdir()?;
        let path = key_path(dir.path(), "demo");
        assert!(ProjectKey::load(&path)?.is_none());

        let key = ProjectKey::generate();
        key.save(&path)?;
        let loaded = ProjectKey::load(&path)?.ok_or("key missing")?;
        assert_eq!(loaded.recipient(), key.recipient());

        let before = fs::read(&path)?;
        let other = ProjectKey::generate();
        assert!(matches!(other.save(&path), Err(StoreError::KeyExists(_))));
        assert_eq!(fs::read(&path)?, before, "existing key was changed");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(&path)?.permissions().mode() & 0o777, 0o600);
            let keys_dir = path.parent().ok_or("no parent")?;
            assert_eq!(fs::metadata(keys_dir)?.permissions().mode() & 0o777, 0o700);
        }
        Ok(())
    }

    #[test]
    fn load_or_create_creates_once() -> TestResult {
        let dir = tempfile::tempdir()?;
        let path = key_path(dir.path(), "demo");
        let (first, created) = ProjectKey::load_or_create(&path)?;
        assert!(created);
        let (second, created) = ProjectKey::load_or_create(&path)?;
        assert!(!created);
        assert_eq!(first.recipient(), second.recipient());
        Ok(())
    }

    #[test]
    fn load_rejects_bad_files() -> TestResult {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("bad.agekey");
        for contents in [
            "",
            "# only a comment\n",
            "garbage\n",
            "AGE-SECRET-KEY-1X\nAGE-SECRET-KEY-1Y\n",
        ] {
            fs::write(&path, contents)?;
            assert!(
                matches!(ProjectKey::load(&path), Err(StoreError::KeyFile(_))),
                "{contents:?}"
            );
        }
        // A file written by age-keygen (comments plus the key line) loads.
        let key = ProjectKey::generate();
        fs::write(
            &path,
            format!(
                "# created: 2026-11-14T00:00:00Z\n# public key: {}\n{}\n",
                key.recipient(),
                key.export_secret().expose_secret()
            ),
        )?;
        assert_eq!(
            ProjectKey::load(&path)?.ok_or("missing")?.recipient(),
            key.recipient()
        );
        Ok(())
    }

    #[test]
    fn key_paths_are_sanitised() {
        let base = Path::new("/cfg");
        assert_eq!(
            key_path(base, "adpatch"),
            Path::new("/cfg/keys/adpatch.agekey")
        );
        assert_eq!(
            key_path(base, "My Project"),
            Path::new("/cfg/keys/My_Project.agekey")
        );
        assert_eq!(
            key_path(base, "../../etc/passwd"),
            Path::new("/cfg/keys/_._.._etc_passwd.agekey")
        );
        assert_eq!(
            key_path(base, ".hidden"),
            Path::new("/cfg/keys/_hidden.agekey")
        );
        assert_eq!(key_path(base, ".."), Path::new("/cfg/keys/_..agekey"));
        assert_eq!(key_path(base, ""), Path::new("/cfg/keys/default.agekey"));
        assert_eq!(
            key_path(base, "a/b\\c"),
            Path::new("/cfg/keys/a_b_c.agekey")
        );
        for p in ["../x", "..", "/abs", "a/../../b"] {
            let path = key_path(base, p);
            assert_eq!(path.parent(), Some(Path::new("/cfg/keys")), "{p}");
        }
    }
}
