//! The encrypted, pluggable session store (docs/03, D-T1C2).
//!
//! Everything handed to a [`SessionStore`] is compressed with zstd and then
//! encrypted with age to the project's X25519 key before it reaches a
//! [`Backend`], so a bucket provider (or a hosted service later) only ever
//! sees ciphertext.
//!
//! The store does not redact anything itself. Callers must pass bytes that
//! have already been through the redaction gate (docs/04, D-G1R6).
//!
//! Layers, from the caller down:
//!
//! - [`SessionStore`]: content-addressed blobs (`blobs/<sha256>`) and small
//!   named records (`<kind>/<id>.json.age`), with the payload format in
//!   `format`.
//! - [`ProjectKey`]: the project's age identity, saved under the personal
//!   config directory (see [`key_path`]), never in the repo.
//! - [`Backend`]: where the bytes live. [`LocalBackend`] is a directory on
//!   disk; [`S3Backend`] is any S3-compatible bucket (AWS S3, Cloudflare R2,
//!   Backblaze B2, MinIO).

#![forbid(unsafe_code)]

mod backend;
mod error;
mod format;
mod key;
mod local;
mod s3;
mod store;

pub use backend::{Backend, validate_key};
pub use error::StoreError;
pub use key::{ProjectKey, key_path};
pub use local::LocalBackend;
pub use s3::{S3Backend, S3Config};
pub use store::{BLOB_PREFIX, RECORD_SUFFIX, SessionStore};
