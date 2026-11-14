//! [`SessionStore`]: compression and encryption over a [`Backend`].

use sha2::{Digest, Sha256};

use crate::format::{self, Kind};
use crate::{Backend, ProjectKey, StoreError};

/// Where content-addressed blobs live: `blobs/<sha256 hex>`.
pub const BLOB_PREFIX: &str = "blobs";
/// The suffix of a record's storage key: `<kind>/<id>.json.age`.
pub const RECORD_SUFFIX: &str = ".json.age";

/// The session store: compresses (zstd) and then encrypts (age, to the
/// project key) everything before it reaches the backend.
///
/// Callers must pass bytes that have already been redacted; the store does
/// not look inside them.
///
/// Blobs are content-addressed by the SHA-256 of their **ciphertext**. age
/// encryption is randomised, so putting the same plaintext twice gives two
/// different hashes and two stored copies. That is expected: it means the
/// address reveals nothing about the content, and lets anyone holding only
/// the ciphertext (a bucket, a hosted service) check a blob's integrity
/// without the key. Callers should store the returned hash rather than try
/// to recompute it.
pub struct SessionStore {
    backend: Box<dyn Backend>,
    key: ProjectKey,
}

impl std::fmt::Debug for SessionStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionStore")
            .field("backend", &self.backend.describe())
            .field("key", &self.key)
            .finish()
    }
}

impl SessionStore {
    /// A store over `backend`, encrypting to and decrypting with `key`.
    pub fn new(backend: Box<dyn Backend>, key: ProjectKey) -> SessionStore {
        SessionStore { backend, key }
    }

    /// The project key's public recipient (`age1...`).
    pub fn recipient(&self) -> String {
        self.key.recipient()
    }

    /// Compresses, encrypts and stores `plaintext` under
    /// `blobs/<sha256 hex of the encrypted bytes>`, and returns that hash.
    pub fn put_blob(&self, plaintext: &[u8]) -> Result<String, StoreError> {
        let ciphertext = self.seal(Kind::Blob, "", plaintext)?;
        let hash = hex::encode(Sha256::digest(&ciphertext));
        self.backend.put(&blob_key(&hash)?, &ciphertext)?;
        Ok(hash)
    }

    /// Fetches, verifies, decrypts and decompresses a blob. `None` if it is
    /// not in the store.
    ///
    /// The hash of the fetched bytes is checked before decrypting; a mismatch
    /// is [`StoreError::Tampered`].
    pub fn get_blob(&self, hash: &str) -> Result<Option<Vec<u8>>, StoreError> {
        let key = blob_key(hash)?;
        let Some(ciphertext) = self.backend.get(&key)? else {
            return Ok(None);
        };
        let actual = hex::encode(Sha256::digest(&ciphertext));
        if actual != hash {
            return Err(StoreError::Tampered {
                expected: hash.to_owned(),
                actual,
            });
        }
        self.open(Kind::Blob, "", &ciphertext, &key).map(Some)
    }

    /// Deletes a blob. Deleting one that is not there is not an error.
    pub fn delete_blob(&self, hash: &str) -> Result<(), StoreError> {
        self.backend.delete(&blob_key(hash)?)
    }

    /// Lists the hashes of every blob in the store, sorted.
    pub fn list_blobs(&self) -> Result<Vec<String>, StoreError> {
        let prefix = format!("{BLOB_PREFIX}/");
        Ok(self
            .backend
            .list(BLOB_PREFIX)?
            .into_iter()
            .filter_map(|key| key.strip_prefix(&prefix).map(str::to_owned))
            .filter(|hash| is_hash(hash))
            .collect())
    }

    /// Stores a small named record (such as session metadata) at
    /// `<kind>/<id>.json.age`, encrypted, replacing any previous version.
    pub fn put_record(&self, kind: &str, id: &str, plaintext: &[u8]) -> Result<(), StoreError> {
        let key = record_key(kind, id)?;
        let ciphertext = self.seal(Kind::Record, &key, plaintext)?;
        self.backend.put(&key, &ciphertext)
    }

    /// Fetches and decrypts a record. `None` if it is not in the store.
    pub fn get_record(&self, kind: &str, id: &str) -> Result<Option<Vec<u8>>, StoreError> {
        let key = record_key(kind, id)?;
        let Some(ciphertext) = self.backend.get(&key)? else {
            return Ok(None);
        };
        self.open(Kind::Record, &key, &ciphertext, &key).map(Some)
    }

    /// Deletes a record. Deleting one that is not there is not an error.
    pub fn delete_record(&self, kind: &str, id: &str) -> Result<(), StoreError> {
        self.backend.delete(&record_key(kind, id)?)
    }

    /// Lists the ids of every record of `kind`, sorted.
    pub fn list_records(&self, kind: &str) -> Result<Vec<String>, StoreError> {
        validate_kind(kind)?;
        let prefix = format!("{kind}/");
        Ok(self
            .backend
            .list(kind)?
            .into_iter()
            .filter_map(|key| {
                key.strip_prefix(&prefix)?
                    .strip_suffix(RECORD_SUFFIX)
                    .filter(|id| validate_id(id).is_ok())
                    .map(str::to_owned)
            })
            .collect())
    }

    /// The backend's description, e.g. `local /path` or `s3 bucket/prefix`.
    pub fn describe(&self) -> String {
        self.backend.describe()
    }

    fn seal(&self, kind: Kind, name: &str, plaintext: &[u8]) -> Result<Vec<u8>, StoreError> {
        let payload = format::encode(kind, name, plaintext)?;
        age::encrypt(&self.key.age_recipient(), &payload).map_err(|_| StoreError::Encrypt)
    }

    fn open(
        &self,
        kind: Kind,
        name: &str,
        ciphertext: &[u8],
        what: &str,
    ) -> Result<Vec<u8>, StoreError> {
        let payload = age::decrypt(self.key.identity(), ciphertext).map_err(|e| match e {
            age::DecryptError::NoMatchingKeys => StoreError::Decrypt,
            _ => StoreError::Corrupt {
                what: what.to_owned(),
                reason: "it could not be decrypted",
            },
        })?;
        format::decode(kind, name, &payload, what)
    }
}

fn is_hash(hash: &str) -> bool {
    hash.len() == 64 && hash.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn blob_key(hash: &str) -> Result<String, StoreError> {
    if !is_hash(hash) {
        return Err(StoreError::InvalidHash(hash.to_owned()));
    }
    Ok(format!("{BLOB_PREFIX}/{hash}"))
}

fn record_key(kind: &str, id: &str) -> Result<String, StoreError> {
    validate_kind(kind)?;
    validate_id(id)?;
    Ok(format!("{kind}/{id}{RECORD_SUFFIX}"))
}

fn valid_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && !value.starts_with('.')
        && !value.contains("..")
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
}

fn validate_kind(kind: &str) -> Result<(), StoreError> {
    if valid_name(kind) && kind != BLOB_PREFIX {
        Ok(())
    } else {
        Err(StoreError::InvalidId {
            what: "record kind",
            value: kind.to_owned(),
        })
    }
}

fn validate_id(id: &str) -> Result<(), StoreError> {
    if valid_name(id) {
        Ok(())
    } else {
        Err(StoreError::InvalidId {
            what: "record id",
            value: id.to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Instant;

    use super::*;
    use crate::LocalBackend;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    /// Runs `check` against a store on each backend: a local directory, and
    /// the object_store wrapper behind `S3Backend` over `InMemory`.
    fn on_each_backend(check: impl Fn(&SessionStore) -> TestResult) -> TestResult {
        let dir = tempfile::tempdir()?;
        let local = SessionStore::new(
            Box::new(LocalBackend::new(dir.path().join("store"))),
            ProjectKey::generate(),
        );
        check(&local)?;
        let (memory, _raw) = crate::s3::in_memory("projects/demo")?;
        let remote = SessionStore::new(Box::new(memory), ProjectKey::generate());
        check(&remote)
    }

    #[test]
    fn blob_round_trip() -> TestResult {
        on_each_backend(|store| {
            let text = b"user: hello\nassistant: hi there\n".repeat(100);
            let hash = store.put_blob(&text)?;
            assert!(is_hash(&hash));
            assert_eq!(store.get_blob(&hash)?.as_deref(), Some(&text[..]));
            assert_eq!(store.list_blobs()?, vec![hash.clone()]);

            let empty = store.put_blob(b"")?;
            assert_eq!(store.get_blob(&empty)?.as_deref(), Some(&b""[..]));

            store.delete_blob(&hash)?;
            assert_eq!(store.get_blob(&hash)?, None);
            store.delete_blob(&hash)?;
            Ok(())
        })
    }

    #[test]
    fn hash_is_of_ciphertext_and_differs_between_puts() -> TestResult {
        on_each_backend(|store| {
            let a = store.put_blob(b"same plaintext")?;
            let b = store.put_blob(b"same plaintext")?;
            assert_ne!(
                a, b,
                "age is randomised, so the same plaintext stores twice"
            );
            assert_ne!(a, hex::encode(Sha256::digest(b"same plaintext")));
            assert_eq!(store.get_blob(&a)?, store.get_blob(&b)?);
            Ok(())
        })
    }

    #[test]
    fn record_round_trip_and_listing() -> TestResult {
        on_each_backend(|store| {
            assert_eq!(store.list_records("sessions")?, Vec::<String>::new());
            store.put_record("sessions", "01HX-b", br#"{"id":"b"}"#)?;
            store.put_record("sessions", "01HX-a", br#"{"id":"a"}"#)?;
            store.put_record("summaries", "01HX-a", b"summary")?;
            store.put_blob(b"not a record")?;

            assert_eq!(
                store.get_record("sessions", "01HX-a")?.as_deref(),
                Some(&br#"{"id":"a"}"#[..])
            );
            assert_eq!(store.get_record("sessions", "missing")?, None);
            assert_eq!(store.list_records("sessions")?, vec!["01HX-a", "01HX-b"]);
            assert_eq!(store.list_records("summaries")?, vec!["01HX-a"]);

            store.put_record("sessions", "01HX-a", b"v2")?;
            assert_eq!(
                store.get_record("sessions", "01HX-a")?.as_deref(),
                Some(&b"v2"[..])
            );

            store.delete_record("sessions", "01HX-a")?;
            assert_eq!(store.get_record("sessions", "01HX-a")?, None);
            assert_eq!(store.list_records("sessions")?, vec!["01HX-b"]);
            store.delete_record("sessions", "01HX-a")?;
            Ok(())
        })
    }

    #[test]
    fn rejects_bad_ids_kinds_and_hashes() -> TestResult {
        let dir = tempfile::tempdir()?;
        let store = SessionStore::new(
            Box::new(LocalBackend::new(dir.path().into())),
            ProjectKey::generate(),
        );
        for id in [
            "",
            "a/b",
            "..",
            "../x",
            ".hidden",
            "a b",
            "a\\b",
            &"x".repeat(129),
        ] {
            assert!(
                matches!(
                    store.put_record("sessions", id, b"x"),
                    Err(StoreError::InvalidId { .. })
                ),
                "{id:?}"
            );
            assert!(store.get_record("sessions", id).is_err(), "{id:?}");
        }
        for kind in ["", "a/b", "..", "blobs"] {
            assert!(
                matches!(
                    store.put_record(kind, "x", b"x"),
                    Err(StoreError::InvalidId { .. })
                ),
                "{kind:?}"
            );
        }
        for hash in ["", "abc", &"G".repeat(64), &"A".repeat(64), "../etc/passwd"] {
            assert!(
                matches!(store.get_blob(hash), Err(StoreError::InvalidHash(_))),
                "{hash:?}"
            );
            assert!(store.delete_blob(hash).is_err(), "{hash:?}");
        }
        // UUIDs and Crockford ids are fine.
        store.put_record("sessions", "6f1c9a2e-3b4d-4e5f-8a9b-0c1d2e3f4a5b", b"x")?;
        store.put_record("sessions", "01JD3W8Z0K4N6P8R2T4V6X8Z0B", b"x")?;
        Ok(())
    }

    #[test]
    fn nothing_readable_reaches_the_backend() -> TestResult {
        let dir = tempfile::tempdir()?;
        let backend = LocalBackend::new(dir.path().to_path_buf());
        let store = SessionStore::new(Box::new(backend.clone()), ProjectKey::generate());
        let marker = b"UNMISTAKABLE-PLAINTEXT-MARKER";
        let hash = store.put_blob(&marker.repeat(10))?;
        store.put_record("sessions", "s1", marker)?;
        for key in backend.list("")? {
            let raw = std::fs::read(backend.path_for(&key)?)?;
            assert!(raw.starts_with(b"age-encryption.org/v1\n"), "{key}");
            assert!(!raw.windows(marker.len()).any(|w| w == marker), "{key}");
        }
        assert!(backend.path_for(&format!("blobs/{hash}"))?.exists());
        assert!(backend.path_for("sessions/s1.json.age")?.exists());
        Ok(())
    }

    #[test]
    fn tampered_blob_is_detected() -> TestResult {
        let dir = tempfile::tempdir()?;
        let backend = LocalBackend::new(dir.path().to_path_buf());
        let store = SessionStore::new(Box::new(backend.clone()), ProjectKey::generate());
        let hash = store.put_blob(b"original transcript")?;
        let other = store.put_blob(b"another transcript")?;
        let path = backend.path_for(&format!("blobs/{hash}"))?;

        // Flip a byte.
        let mut bytes = std::fs::read(&path)?;
        let last = bytes.len() - 1;
        bytes[last] ^= 0x01;
        std::fs::write(&path, &bytes)?;
        assert!(matches!(
            store.get_blob(&hash),
            Err(StoreError::Tampered { .. })
        ));

        // Swap in another valid blob: decrypts fine, but the address catches it.
        std::fs::copy(backend.path_for(&format!("blobs/{other}"))?, &path)?;
        let err = store.get_blob(&hash).err();
        assert!(matches!(err, Some(StoreError::Tampered { .. })), "{err:?}");
        Ok(())
    }

    #[test]
    fn swapped_record_is_detected() -> TestResult {
        let dir = tempfile::tempdir()?;
        let backend = LocalBackend::new(dir.path().to_path_buf());
        let store = SessionStore::new(Box::new(backend.clone()), ProjectKey::generate());
        store.put_record("sessions", "a", b"session a")?;
        store.put_record("sessions", "b", b"session b")?;
        std::fs::copy(
            backend.path_for("sessions/a.json.age")?,
            backend.path_for("sessions/b.json.age")?,
        )?;
        assert!(matches!(
            store.get_record("sessions", "b"),
            Err(StoreError::Corrupt { .. })
        ));

        std::fs::write(backend.path_for("sessions/a.json.age")?, b"not age at all")?;
        assert!(matches!(
            store.get_record("sessions", "a"),
            Err(StoreError::Corrupt { .. })
        ));
        Ok(())
    }

    #[test]
    fn wrong_key_gives_decrypt() -> TestResult {
        let (memory, _raw) = crate::s3::in_memory("")?;
        let shared = Arc::new(memory);
        let writer = SessionStore::new(Box::new(shared.clone()), ProjectKey::generate());
        let reader = SessionStore::new(Box::new(shared), ProjectKey::generate());
        let hash = writer.put_blob(b"secret-free transcript")?;
        writer.put_record("sessions", "s1", b"{}")?;

        let err = reader.get_blob(&hash).err();
        assert!(matches!(err, Some(StoreError::Decrypt)), "{err:?}");
        let err = reader.get_record("sessions", "s1").err();
        assert!(matches!(err, Some(StoreError::Decrypt)), "{err:?}");
        let message = StoreError::Decrypt.to_string();
        assert!(message.contains("inloop key import"), "{message}");

        // The same key, imported elsewhere, reads it.
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("k.agekey");
        let key = ProjectKey::generate();
        key.save(&path)?;
        let (memory, _raw) = crate::s3::in_memory("")?;
        let shared = Arc::new(memory);
        let a = SessionStore::new(Box::new(shared.clone()), key);
        let b = SessionStore::new(Box::new(shared), ProjectKey::load(&path)?.ok_or("missing")?);
        let hash = a.put_blob(b"shared")?;
        assert_eq!(b.get_blob(&hash)?.as_deref(), Some(&b"shared"[..]));
        Ok(())
    }

    #[test]
    fn delete_really_deletes_from_disk() -> TestResult {
        let dir = tempfile::tempdir()?;
        let backend = LocalBackend::new(dir.path().to_path_buf());
        let store = SessionStore::new(Box::new(backend.clone()), ProjectKey::generate());
        let hash = store.put_blob(b"to be purged")?;
        store.put_record("sessions", "s1", b"meta")?;
        let blob_path = backend.path_for(&format!("blobs/{hash}"))?;
        let record_path = backend.path_for("sessions/s1.json.age")?;
        assert!(blob_path.exists() && record_path.exists());

        store.delete_blob(&hash)?;
        store.delete_record("sessions", "s1")?;
        assert!(!blob_path.exists());
        assert!(!record_path.exists());
        assert!(backend.list("")?.is_empty());
        Ok(())
    }

    #[test]
    fn describes_backend_and_hides_key() -> TestResult {
        let key = ProjectKey::generate();
        let secret = key.export_secret();
        let store = SessionStore::new(Box::new(LocalBackend::new("/x/store".into())), key);
        assert_eq!(store.describe(), "local /x/store");
        let debug = format!("{store:?}");
        use secrecy::ExposeSecret;
        assert!(!debug.contains(secret.expose_secret()), "{debug}");
        Ok(())
    }

    /// A 5 MB transcript round-trips quickly. The bound is only enforced in
    /// release builds; debug builds just report the time.
    #[test]
    fn five_megabyte_transcript_round_trip() -> TestResult {
        // Varied, text-like content so compression has realistic work to do.
        let mut text = Vec::with_capacity(5 * 1024 * 1024);
        let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
        let words = [
            "the",
            "agent",
            "ran",
            "cargo",
            "test",
            "and",
            "fixed",
            "a",
            "bug",
            "in",
            "crates/store/src/lib.rs",
            "{\"type\":\"tool_use\"}",
            "\n",
        ];
        while text.len() < 5 * 1024 * 1024 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let index = usize::try_from(state % words.len() as u64)?;
            text.extend_from_slice(words[index].as_bytes());
            text.push(b' ');
            if state.is_multiple_of(97) {
                text.extend_from_slice(format!("{state:x}").as_bytes());
            }
        }
        let dir = tempfile::tempdir()?;
        let store = SessionStore::new(
            Box::new(LocalBackend::new(dir.path().into())),
            ProjectKey::generate(),
        );
        let start = Instant::now();
        let hash = store.put_blob(&text)?;
        let put = start.elapsed();
        let back = store.get_blob(&hash)?.ok_or("missing")?;
        let total = start.elapsed();
        assert_eq!(back, text);
        let stored = std::fs::metadata(dir.path().join("blobs").join(&hash))?.len();
        println!(
            "5 MB round trip: put {put:?}, total {total:?}, stored {stored} bytes ({:.1}% of original)",
            stored as f64 * 100.0 / text.len() as f64
        );
        if !cfg!(debug_assertions) {
            assert!(total.as_secs_f64() < 2.0, "5 MB round trip took {total:?}");
        }
        Ok(())
    }
}
