//! `inloop.toml`, the per-project configuration (docs/07), and the personal
//! configuration in `~/.config/inloop/`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::id::IdSpec;

/// The name of the project config file at the repo root.
pub const CONFIG_FILE: &str = "inloop.toml";

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProjectConfig {
    pub project: ProjectSection,
    pub decisions: DecisionsSection,
    pub checks: ChecksSection,
    pub sessions: SessionsSection,
    pub redaction: RedactionSection,
    pub pm: PmSection,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProjectSection {
    /// Shown in output. Defaults to the repo directory name.
    pub name: Option<String>,
    /// What "merged" means. Defaults to `origin/HEAD`'s branch, else `main`.
    pub main_branch: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DecisionsSection {
    pub dir: String,
    pub id_prefix: String,
    pub id_length: usize,
    /// Globs of files whose decision references `inloop lint` checks.
    pub lint_references: Vec<String>,
    /// IDs used as examples in docs, which lint doesn't expect to exist.
    pub example_ids: Vec<String>,
}

impl Default for DecisionsSection {
    fn default() -> Self {
        Self {
            dir: "docs/decisions".into(),
            id_prefix: "D".into(),
            id_length: 4,
            lint_references: vec!["**/*.md".into()],
            example_ids: Vec::new(),
        }
    }
}

impl DecisionsSection {
    pub fn id_spec(&self) -> IdSpec {
        IdSpec::new(self.id_prefix.clone(), self.id_length)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ChecksSection {
    pub commands: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    Local,
    S3,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SessionsSection {
    pub backend: Backend,
    pub bucket: Option<String>,
    pub endpoint: Option<String>,
    pub region: Option<String>,
    /// Key prefix inside the bucket. Defaults to the project name.
    pub prefix: Option<String>,
}

impl Default for SessionsSection {
    fn default() -> Self {
        Self {
            backend: Backend::Local,
            bucket: None,
            endpoint: None,
            region: None,
            prefix: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PatternConfig {
    pub kind: String,
    pub regex: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RedactionSection {
    pub known_value_files: Vec<String>,
    pub patterns: Vec<PatternConfig>,
    pub personal_data: bool,
}

impl Default for RedactionSection {
    fn default() -> Self {
        Self {
            known_value_files: vec![".env".into(), ".env.*".into()],
            patterns: Vec::new(),
            personal_data: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConflictGroup {
    pub name: String,
    pub paths: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PmSection {
    pub enabled: bool,
    pub brief_template: String,
    pub conflict_groups: Vec<ConflictGroup>,
}

impl Default for PmSection {
    fn default() -> Self {
        Self {
            enabled: false,
            brief_template: ".inloop/brief.md".into(),
            conflict_groups: Vec::new(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("could not read {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{path} is not valid: {message}")]
    Parse { path: PathBuf, message: String },
}

impl ProjectConfig {
    /// Read `inloop.toml` from `root`. A missing file gives the defaults.
    pub fn load(root: &Path) -> Result<Self, ConfigError> {
        let path = root.join(CONFIG_FILE);
        match std::fs::read_to_string(&path) {
            Ok(text) => Self::parse(&text).map_err(|message| ConfigError::Parse { path, message }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(source) => Err(ConfigError::Read { path, source }),
        }
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        toml::from_str(text).map_err(|e| e.to_string())
    }

    /// The project name, falling back to the repo directory's name.
    pub fn project_name(&self, root: &Path) -> String {
        self.project
            .name
            .clone()
            .or_else(|| root.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "project".into())
    }
}

/// The personal config directory: `$INLOOP_CONFIG_DIR`, else
/// `$XDG_CONFIG_HOME/inloop`, else `~/.config/inloop`.
pub fn personal_config_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("INLOOP_CONFIG_DIR") {
        return Some(PathBuf::from(d));
    }
    if let Some(d) = std::env::var_os("XDG_CONFIG_HOME") {
        return Some(PathBuf::from(d).join("inloop"));
    }
    home_dir().map(|h| h.join(".config").join("inloop"))
}

/// The personal data directory for local session storage:
/// `$INLOOP_DATA_DIR`, else `$XDG_DATA_HOME/inloop`, else
/// `~/.local/share/inloop`.
pub fn personal_data_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("INLOOP_DATA_DIR") {
        return Some(PathBuf::from(d));
    }
    if let Some(d) = std::env::var_os("XDG_DATA_HOME") {
        return Some(PathBuf::from(d).join("inloop"));
    }
    home_dir().map(|h| h.join(".local").join("share").join("inloop"))
}

pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// Personal settings that never go in the repo: storage credentials and
/// where keys live.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PersonalConfig {
    /// Overrides for the local session store directory.
    pub data_dir: Option<PathBuf>,
    /// Credentials for the s3 backend. Prefer the environment
    /// (`AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`).
    pub s3: Option<S3Credentials>,
}

#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct S3Credentials {
    pub access_key_id: Option<String>,
    pub secret_access_key: Option<String>,
}

impl std::fmt::Debug for S3Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("S3Credentials")
            .field(
                "access_key_id",
                &self.access_key_id.as_ref().map(|_| "<set>"),
            )
            .field(
                "secret_access_key",
                &self.secret_access_key.as_ref().map(|_| "<set>"),
            )
            .finish()
    }
}

impl PersonalConfig {
    pub fn load() -> Result<Self, ConfigError> {
        let Some(dir) = personal_config_dir() else {
            return Ok(Self::default());
        };
        let path = dir.join("config.toml");
        match std::fs::read_to_string(&path) {
            Ok(text) => toml::from_str(&text).map_err(|e| ConfigError::Parse {
                path,
                message: e.to_string(),
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(source) => Err(ConfigError::Read { path, source }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inloops_own_config_parses() {
        let text = include_str!("../../../inloop.toml");
        let cfg = ProjectConfig::parse(text);
        assert!(cfg.is_ok(), "{cfg:?}");
        let cfg = cfg.unwrap_or_default();
        assert_eq!(cfg.decisions.dir, "docs/decisions");
        assert!(cfg.pm.enabled);
        assert_eq!(cfg.pm.conflict_groups.len(), 3);
    }

    #[test]
    fn empty_config_gives_defaults() {
        let cfg = ProjectConfig::parse("").unwrap_or_default();
        assert_eq!(cfg.decisions.id_spec(), IdSpec::decisions());
        assert_eq!(cfg.sessions.backend, Backend::Local);
        assert!(!cfg.pm.enabled);
        assert_eq!(cfg.redaction.known_value_files, vec![".env", ".env.*"]);
    }

    #[test]
    fn unknown_keys_are_errors() {
        assert!(ProjectConfig::parse("[decisions]\ndri = \"x\"\n").is_err());
    }

    #[test]
    fn credentials_never_debug_print() {
        let c = S3Credentials {
            access_key_id: Some("AKIAEXAMPLE".into()),
            secret_access_key: Some("very-secret".into()),
        };
        let s = format!("{c:?}");
        assert!(!s.contains("very-secret") && !s.contains("AKIAEXAMPLE"));
    }
}
