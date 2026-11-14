//! The [`Backend`] trait: where encrypted bytes live.

use std::sync::Arc;

use crate::StoreError;

/// Where blobs live. Object safe and synchronous: async backends run their own
/// internal runtime, so callers never need one.
///
/// Keys are relative paths with `/` separators, checked by [`validate_key`].
/// Backends only ever see ciphertext; they know nothing about encryption.
pub trait Backend: Send + Sync {
    /// Stores `bytes` under `key`, replacing anything already there.
    fn put(&self, key: &str, bytes: &[u8]) -> Result<(), StoreError>;
    /// Fetches the bytes under `key`, or `None` if there are none.
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>, StoreError>;
    /// Deletes `key`. Deleting a key that does not exist is not an error.
    fn delete(&self, key: &str) -> Result<(), StoreError>;
    /// Lists every key under `prefix`, sorted.
    ///
    /// The prefix is matched by whole path segments: `"sessions"` and
    /// `"sessions/"` both list `sessions/a.json.age` but not
    /// `sessions-old/b`. An empty prefix lists everything.
    fn list(&self, prefix: &str) -> Result<Vec<String>, StoreError>;
    /// A short description for humans, such as `local /path` or
    /// `s3 bucket/prefix`. Never includes credentials.
    fn describe(&self) -> String;
}

impl<B: Backend + ?Sized> Backend for Arc<B> {
    fn put(&self, key: &str, bytes: &[u8]) -> Result<(), StoreError> {
        (**self).put(key, bytes)
    }
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>, StoreError> {
        (**self).get(key)
    }
    fn delete(&self, key: &str) -> Result<(), StoreError> {
        (**self).delete(key)
    }
    fn list(&self, prefix: &str) -> Result<Vec<String>, StoreError> {
        (**self).list(prefix)
    }
    fn describe(&self) -> String {
        (**self).describe()
    }
}

impl<B: Backend + ?Sized> Backend for Box<B> {
    fn put(&self, key: &str, bytes: &[u8]) -> Result<(), StoreError> {
        (**self).put(key, bytes)
    }
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>, StoreError> {
        (**self).get(key)
    }
    fn delete(&self, key: &str) -> Result<(), StoreError> {
        (**self).delete(key)
    }
    fn list(&self, prefix: &str) -> Result<Vec<String>, StoreError> {
        (**self).list(prefix)
    }
    fn describe(&self) -> String {
        (**self).describe()
    }
}

/// Checks a storage key: a relative path of segments made of ASCII letters,
/// digits, `.`, `-` and `_`, separated by single `/`. Rejects empty keys,
/// absolute paths, backslashes, empty segments and `.` or `..` segments.
pub fn validate_key(key: &str) -> Result<(), StoreError> {
    let bad = |reason| {
        Err(StoreError::InvalidKey {
            key: key.to_owned(),
            reason,
        })
    };
    if key.is_empty() {
        return bad("it is empty");
    }
    if key.len() > 512 {
        return bad("it is longer than 512 characters");
    }
    if key.starts_with('/') {
        return bad("it is an absolute path");
    }
    if key.contains('\\') {
        return bad("it contains a backslash");
    }
    for segment in key.split('/') {
        if segment.is_empty() {
            return bad("it has an empty path segment");
        }
        if segment == "." || segment == ".." {
            return bad("it has a '.' or '..' segment");
        }
        if !segment
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
        {
            return bad(
                "it contains a character other than letters, digits, '.', '-', '_' and '/'",
            );
        }
    }
    Ok(())
}

/// Normalises a list prefix: validates it (unless empty) and strips one
/// trailing `/`.
pub(crate) fn normalise_prefix(prefix: &str) -> Result<&str, StoreError> {
    let trimmed = prefix.strip_suffix('/').unwrap_or(prefix);
    if !trimmed.is_empty() {
        validate_key(trimmed)?;
    }
    Ok(trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_ordinary_keys() {
        for key in ["blobs/abc", "sessions/01HX-a_b.json.age", "a", "a/b/c"] {
            assert!(validate_key(key).is_ok(), "{key}");
        }
    }

    #[test]
    fn rejects_bad_keys() {
        for key in [
            "", "/abs", "a/../b", "..", ".", "a//b", "a/", "a\\b", "c:\\x", "a b", "a/./b", "ü",
            "a\0b",
        ] {
            assert!(
                matches!(validate_key(key), Err(StoreError::InvalidKey { .. })),
                "{key:?}"
            );
        }
    }

    #[test]
    fn normalises_prefixes() -> Result<(), StoreError> {
        assert_eq!(normalise_prefix("")?, "");
        assert_eq!(normalise_prefix("sessions/")?, "sessions");
        assert_eq!(normalise_prefix("sessions")?, "sessions");
        assert!(normalise_prefix("../x").is_err());
        Ok(())
    }
}
