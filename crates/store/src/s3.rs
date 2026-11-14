//! [`S3Backend`]: any S3-compatible bucket (AWS S3, Cloudflare R2, Backblaze
//! B2, MinIO), built on `object_store`.

use std::fmt;
use std::future::Future;
use std::sync::Arc;

use futures::TryStreamExt;
use object_store::aws::AmazonS3Builder;
use object_store::path::Path as ObjectPath;
use object_store::prefix::PrefixStore;
use object_store::{ObjectStore, ObjectStoreExt, PutPayload};
use secrecy::{ExposeSecret, SecretString};

use crate::StoreError;
use crate::backend::{Backend, normalise_prefix, validate_key};

/// Settings for an S3-compatible bucket.
///
/// Credentials left as `None` come from the standard AWS environment
/// variables (`AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`,
/// `AWS_SESSION_TOKEN`, and so on). `Debug` never prints either credential.
#[derive(Clone, Default)]
pub struct S3Config {
    /// The bucket name.
    pub bucket: String,
    /// The endpoint URL for non-AWS providers, e.g.
    /// `https://<account>.r2.cloudflarestorage.com` for R2 or
    /// `http://localhost:9000` for MinIO. `http://` endpoints are allowed.
    /// `None` means AWS S3 (or `AWS_ENDPOINT` from the environment).
    pub endpoint: Option<String>,
    /// The region. `None` uses `AWS_REGION` / `AWS_DEFAULT_REGION` when set,
    /// else `auto` (what R2 expects).
    pub region: Option<String>,
    /// A key prefix inside the bucket, so one bucket can hold several
    /// projects. May be empty.
    pub prefix: String,
    /// The access key id; `None` reads `AWS_ACCESS_KEY_ID`.
    pub access_key_id: Option<String>,
    /// The secret access key; `None` reads `AWS_SECRET_ACCESS_KEY`.
    pub secret_access_key: Option<SecretString>,
}

impl fmt::Debug for S3Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let set = |present: bool| {
            if present {
                "<set>"
            } else {
                "<from environment>"
            }
        };
        f.debug_struct("S3Config")
            .field("bucket", &self.bucket)
            .field("endpoint", &self.endpoint.as_deref().map(strip_userinfo))
            .field("region", &self.region)
            .field("prefix", &self.prefix)
            .field("access_key_id", &set(self.access_key_id.is_some()))
            .field("secret_access_key", &set(self.secret_access_key.is_some()))
            .finish()
    }
}

/// Removes any `user:password@` part from a URL so it can be shown.
fn strip_userinfo(url: &str) -> String {
    let (scheme, rest) = match url.split_once("://") {
        Some((scheme, rest)) => (Some(scheme), rest),
        None => (None, url),
    };
    let authority_end = rest.find('/').unwrap_or(rest.len());
    let rest = match rest[..authority_end].rfind('@') {
        Some(at) => &rest[at + 1..],
        None => rest,
    };
    match scheme {
        Some(scheme) => format!("{scheme}://{rest}"),
        None => rest.to_owned(),
    }
}

/// A [`Backend`] over an S3-compatible bucket.
///
/// Calls are synchronous: each one runs on a small internal tokio runtime.
/// That works from plain threads and from inside another tokio runtime (the
/// call then runs on a scoped helper thread so it never nests runtimes).
pub struct S3Backend {
    inner: ObjectStoreBackend,
}

impl fmt::Debug for S3Backend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("S3Backend")
            .field("describe", &self.inner.description)
            .finish_non_exhaustive()
    }
}

impl S3Backend {
    /// Connects to the bucket in `config`. This does not make a network call;
    /// a bad bucket or credentials show up on the first operation.
    pub fn new(config: S3Config) -> Result<S3Backend, StoreError> {
        if config.bucket.trim().is_empty() {
            return Err(StoreError::S3Config(
                "no bucket is set; add `bucket` under [sessions] in inloop.toml".into(),
            ));
        }
        let prefix = config.prefix.trim_matches('/');
        if !prefix.is_empty() {
            validate_key(prefix).map_err(|_| {
                StoreError::S3Config(format!(
                    "the prefix {prefix:?} is not valid; use letters, digits, '.', '-', '_' and '/'"
                ))
            })?;
        }

        let mut builder = AmazonS3Builder::from_env().with_bucket_name(&config.bucket);
        if let Some(endpoint) = &config.endpoint {
            builder = builder.with_endpoint(endpoint);
            if endpoint.starts_with("http://") {
                builder = builder.with_allow_http(true);
            }
        }
        match &config.region {
            Some(region) => builder = builder.with_region(region),
            None => {
                let from_env = ["AWS_REGION", "AWS_DEFAULT_REGION"]
                    .iter()
                    .any(|var| std::env::var_os(var).is_some());
                if !from_env {
                    builder = builder.with_region("auto");
                }
            }
        }
        if let Some(id) = &config.access_key_id {
            builder = builder.with_access_key_id(id);
        }
        if let Some(secret) = &config.secret_access_key {
            builder = builder.with_secret_access_key(secret.expose_secret());
        }
        let store = builder.build().map_err(|e| {
            // object_store's build errors name the missing or bad setting;
            // they do not echo credential values.
            StoreError::S3Config(format!("{e}; check bucket, endpoint and region"))
        })?;

        let mut description = format!("s3 {}", config.bucket);
        if !prefix.is_empty() {
            description.push('/');
            description.push_str(prefix);
        }
        if let Some(endpoint) = &config.endpoint {
            description.push_str(&format!(" at {}", strip_userinfo(endpoint)));
        }
        Ok(S3Backend {
            inner: ObjectStoreBackend::new(Arc::new(store), prefix, description)?,
        })
    }
}

impl Backend for S3Backend {
    fn put(&self, key: &str, bytes: &[u8]) -> Result<(), StoreError> {
        self.inner.put(key, bytes)
    }
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>, StoreError> {
        self.inner.get(key)
    }
    fn delete(&self, key: &str) -> Result<(), StoreError> {
        self.inner.delete(key)
    }
    fn list(&self, prefix: &str) -> Result<Vec<String>, StoreError> {
        self.inner.list(prefix)
    }
    fn describe(&self) -> String {
        self.inner.description.clone()
    }
}

/// A synchronous [`Backend`] over any `object_store` store, with an optional
/// key prefix. [`S3Backend`] is a thin wrapper around this, and the tests run
/// it over `InMemory` so the same code path is covered without a network.
pub(crate) struct ObjectStoreBackend {
    store: Arc<dyn ObjectStore>,
    runtime: Option<tokio::runtime::Runtime>,
    description: String,
}

impl ObjectStoreBackend {
    pub(crate) fn new(
        store: Arc<dyn ObjectStore>,
        prefix: &str,
        description: String,
    ) -> Result<ObjectStoreBackend, StoreError> {
        let store: Arc<dyn ObjectStore> = if prefix.is_empty() {
            store
        } else {
            let path = ObjectPath::parse(prefix).map_err(|e| {
                StoreError::S3Config(format!("the prefix {prefix:?} is not valid: {e}"))
            })?;
            Arc::new(PrefixStore::new(store, path))
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(StoreError::Runtime)?;
        Ok(ObjectStoreBackend {
            store,
            runtime: Some(runtime),
            description,
        })
    }

    /// Runs a future to completion on the internal runtime.
    fn block_on<F>(&self, future: F) -> Result<F::Output, StoreError>
    where
        F: Future + Send,
        F::Output: Send,
    {
        let Some(runtime) = &self.runtime else {
            return Err(StoreError::Runtime(std::io::Error::other(
                "the runtime has shut down",
            )));
        };
        if tokio::runtime::Handle::try_current().is_ok() {
            // Inside someone else's runtime, `block_on` would panic. Drive ours
            // from a scoped helper thread instead.
            std::thread::scope(|scope| {
                scope
                    .spawn(|| runtime.block_on(future))
                    .join()
                    .map_err(|_| {
                        StoreError::Runtime(std::io::Error::other(
                            "the remote store's worker thread stopped unexpectedly",
                        ))
                    })
            })
        } else {
            Ok(runtime.block_on(future))
        }
    }

    fn path(key: &str) -> Result<ObjectPath, StoreError> {
        validate_key(key)?;
        ObjectPath::parse(key).map_err(|_| StoreError::InvalidKey {
            key: key.to_owned(),
            reason: "it is not a valid object path",
        })
    }

    fn remote(&self, action: &'static str, key: &str, source: object_store::Error) -> StoreError {
        StoreError::Remote {
            backend: self.description.clone(),
            action,
            key: key.to_owned(),
            source: Box::new(source),
        }
    }
}

impl Drop for ObjectStoreBackend {
    fn drop(&mut self) {
        // Dropping a runtime from inside another runtime panics; shutting it
        // down in the background never does.
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_background();
        }
    }
}

impl Backend for ObjectStoreBackend {
    fn put(&self, key: &str, bytes: &[u8]) -> Result<(), StoreError> {
        let path = Self::path(key)?;
        let payload = PutPayload::from(bytes.to_vec());
        self.block_on(self.store.put(&path, payload))?
            .map(|_| ())
            .map_err(|e| self.remote("store", key, e))
    }

    fn get(&self, key: &str) -> Result<Option<Vec<u8>>, StoreError> {
        let path = Self::path(key)?;
        let result = self.block_on(async {
            match self.store.get(&path).await {
                Ok(got) => got.bytes().await.map(Some),
                Err(object_store::Error::NotFound { .. }) => Ok(None),
                Err(e) => Err(e),
            }
        })?;
        result
            .map(|bytes| bytes.map(|b| b.to_vec()))
            .map_err(|e| self.remote("fetch", key, e))
    }

    fn delete(&self, key: &str) -> Result<(), StoreError> {
        let path = Self::path(key)?;
        match self.block_on(self.store.delete(&path))? {
            Ok(()) | Err(object_store::Error::NotFound { .. }) => Ok(()),
            Err(e) => Err(self.remote("delete", key, e)),
        }
    }

    fn list(&self, prefix: &str) -> Result<Vec<String>, StoreError> {
        let prefix = normalise_prefix(prefix)?;
        let path = if prefix.is_empty() {
            None
        } else {
            Some(Self::path(prefix)?)
        };
        let metas = self
            .block_on(self.store.list(path.as_ref()).try_collect::<Vec<_>>())?
            .map_err(|e| self.remote("list", prefix, e))?;
        let mut keys: Vec<String> = metas
            .into_iter()
            .map(|meta| meta.location.as_ref().to_owned())
            .filter(|key| validate_key(key).is_ok())
            .collect();
        keys.sort();
        Ok(keys)
    }

    fn describe(&self) -> String {
        self.description.clone()
    }
}

#[cfg(test)]
pub(crate) fn in_memory(
    prefix: &str,
) -> Result<(ObjectStoreBackend, Arc<object_store::memory::InMemory>), StoreError> {
    let memory = Arc::new(object_store::memory::InMemory::new());
    let backend = ObjectStoreBackend::new(memory.clone(), prefix, format!("memory {prefix}"))?;
    Ok((backend, memory))
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn round_trip_delete_and_list_through_prefix() -> TestResult {
        let (backend, memory) = in_memory("team/project")?;
        assert_eq!(backend.get("blobs/x")?, None);
        backend.put("blobs/x", b"one")?;
        backend.put("sessions/a.json.age", b"two")?;
        backend.put("sessions-old/b", b"three")?;
        assert_eq!(backend.get("blobs/x")?.as_deref(), Some(&b"one"[..]));

        // The prefix really is applied in the underlying store.
        let raw = ObjectPath::parse("team/project/blobs/x")?;
        let got = backend.block_on(memory.get(&raw))??.bytes();
        assert_eq!(backend.block_on(got)??.as_ref(), b"one");

        assert_eq!(
            backend.list("")?,
            vec!["blobs/x", "sessions-old/b", "sessions/a.json.age"]
        );
        assert_eq!(backend.list("sessions/")?, vec!["sessions/a.json.age"]);

        backend.delete("blobs/x")?;
        assert_eq!(backend.get("blobs/x")?, None);
        backend.delete("blobs/x")?;
        assert!(backend.put("../escape", b"x").is_err());
        Ok(())
    }

    #[test]
    fn works_inside_another_runtime() -> TestResult {
        let (backend, _memory) = in_memory("")?;
        let outer = tokio::runtime::Builder::new_current_thread().build()?;
        outer.block_on(async {
            backend.put("a", b"x")?;
            assert_eq!(backend.get("a")?.as_deref(), Some(&b"x"[..]));
            Ok::<_, StoreError>(())
        })?;
        // Dropping inside a runtime must not panic either.
        outer.block_on(async move { drop(backend) });
        Ok(())
    }

    #[test]
    fn config_debug_hides_credentials() {
        let config = S3Config {
            bucket: "sessions".into(),
            endpoint: Some("https://user:hunter2@example.com/base".into()),
            region: None,
            prefix: "p".into(),
            access_key_id: Some("AKIAEXAMPLEKEYID0000".into()),
            secret_access_key: Some(SecretString::from("super-secret-value")),
        };
        let debug = format!("{config:?}");
        assert!(!debug.contains("super-secret-value"), "{debug}");
        assert!(!debug.contains("AKIAEXAMPLEKEYID0000"), "{debug}");
        assert!(!debug.contains("hunter2"), "{debug}");
        assert!(debug.contains("sessions"), "{debug}");
    }

    #[test]
    fn strips_userinfo() {
        assert_eq!(strip_userinfo("https://a:b@host/x"), "https://host/x");
        assert_eq!(strip_userinfo("https://host/x@y"), "https://host/x@y");
        assert_eq!(strip_userinfo("host"), "host");
    }

    #[test]
    fn builds_r2_and_minio_configs_without_network() -> TestResult {
        let r2 = S3Backend::new(S3Config {
            bucket: "inloop".into(),
            endpoint: Some("https://0123456789abcdef.r2.cloudflarestorage.com".into()),
            region: None,
            prefix: "/projects/demo/".into(),
            access_key_id: Some("id".into()),
            secret_access_key: Some(SecretString::from("secret")),
        })?;
        assert_eq!(
            r2.describe(),
            "s3 inloop/projects/demo at https://0123456789abcdef.r2.cloudflarestorage.com"
        );
        assert!(!format!("{r2:?}").contains("secret\""));

        let minio = S3Backend::new(S3Config {
            bucket: "inloop".into(),
            endpoint: Some("http://localhost:9000".into()),
            region: Some("us-east-1".into()),
            access_key_id: Some("id".into()),
            secret_access_key: Some(SecretString::from("secret")),
            ..S3Config::default()
        })?;
        assert_eq!(minio.describe(), "s3 inloop at http://localhost:9000");

        assert!(matches!(
            S3Backend::new(S3Config::default()),
            Err(StoreError::S3Config(_))
        ));
        assert!(matches!(
            S3Backend::new(S3Config {
                bucket: "b".into(),
                prefix: "../x".into(),
                ..S3Config::default()
            }),
            Err(StoreError::S3Config(_))
        ));
        Ok(())
    }
}
