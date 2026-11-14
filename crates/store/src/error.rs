//! Errors from the session store.
//!
//! No variant carries key material or plaintext. Paths, backend descriptions,
//! storage keys, record ids and blob hashes are fine to show: none of them is
//! secret.

use std::path::PathBuf;

/// Everything that can go wrong in the session store. Each message says what
/// to do next.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum StoreError {
    /// A storage key was malformed. This is a bug in the caller, not bad data.
    #[error(
        "invalid storage key {key:?}: {reason}; keys are relative paths of letters, digits, '.', '-' and '_' separated by '/'"
    )]
    InvalidKey {
        /// The rejected key.
        key: String,
        /// Why it was rejected.
        reason: &'static str,
    },

    /// A record kind or id was malformed.
    #[error(
        "invalid {what} {value:?}: use letters, digits, '.', '-' and '_' only, with no '/' or '..', at most 128 characters"
    )]
    InvalidId {
        /// "record kind" or "record id".
        what: &'static str,
        /// The rejected value.
        value: String,
    },

    /// A blob hash was not 64 lowercase hex characters.
    #[error("invalid blob hash {0:?}: expected 64 lowercase hex characters (a SHA-256)")]
    InvalidHash(String),

    /// A local file operation failed.
    #[error("could not {action} {path}: {source}")]
    Io {
        /// What we were doing, e.g. "write".
        action: &'static str,
        /// The file or directory involved.
        path: PathBuf,
        /// The underlying error.
        #[source]
        source: std::io::Error,
    },

    /// A remote (object store) operation failed.
    #[error(
        "the session store ({backend}) could not {action} {key:?}: {source}; check the bucket, endpoint and credentials, then try again"
    )]
    Remote {
        /// The backend's description, without credentials.
        backend: String,
        /// What we were doing, e.g. "put".
        action: &'static str,
        /// The storage key involved (empty for listing the whole store).
        key: String,
        /// The underlying error.
        #[source]
        source: Box<object_store::Error>,
    },

    /// The S3 backend could not be configured.
    #[error("could not set up the S3 session store: {0}")]
    S3Config(String),

    /// The internal async runtime used by remote backends could not start.
    #[error("could not start the runtime for the remote session store: {0}")]
    Runtime(#[source] std::io::Error),

    /// Encryption failed. Nothing was written.
    #[error("could not encrypt data for the session store; nothing was written")]
    Encrypt,

    /// The data was encrypted to a different project key.
    #[error(
        "this session was encrypted with a different project key; import the right key with `inloop key import`"
    )]
    Decrypt,

    /// The stored data decrypted (or failed to) in a way that means it is
    /// damaged or not something InLoop wrote.
    #[error(
        "stored {what} is corrupt or not in a format this version of InLoop understands ({reason}); purge it with `inloop session purge` and capture the session again, or upgrade InLoop"
    )]
    Corrupt {
        /// The blob hash or record key.
        what: String,
        /// A short, non-secret reason.
        reason: &'static str,
    },

    /// A fetched blob does not match its content address.
    #[error(
        "blob {expected} was modified in the store (its contents now hash to {actual}); do not trust it, purge it with `inloop session purge` and check who can write to the store"
    )]
    Tampered {
        /// The hash that was asked for.
        expected: String,
        /// The hash of what came back.
        actual: String,
    },

    /// `ProjectKey::save` found a key already at the path.
    #[error(
        "a project key already exists at {0}; InLoop never overwrites a key. Move it aside yourself if you really mean to replace it"
    )]
    KeyExists(PathBuf),

    /// A key file did not contain a usable age identity.
    #[error(
        "the project key file {0} does not contain a valid age X25519 identity (an AGE-SECRET-KEY-1... line); restore it from a backup or import the key again with `inloop key import`"
    )]
    KeyFile(PathBuf),

    /// `ProjectKey::import` was given something that is not an age identity.
    /// The input is deliberately not echoed.
    #[error(
        "that is not a valid age X25519 secret key; it should be a single line starting with AGE-SECRET-KEY-1, as printed by `inloop key export`"
    )]
    KeyImport,
}

impl StoreError {
    pub(crate) fn io(
        action: &'static str,
        path: impl Into<PathBuf>,
        source: std::io::Error,
    ) -> Self {
        StoreError::Io {
            action,
            path: path.into(),
            source,
        }
    }
}
