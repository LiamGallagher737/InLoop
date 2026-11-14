//! Agent session capture (docs/03): sources, transcripts, redaction and
//! summaries. Storage is the `store` crate; the pipeline that ties them
//! together lives in the app crate.
#![forbid(unsafe_code)]

pub mod source;
pub mod summary;
pub mod transcript;

use inloop_redact::{RedactError, Redactable, Redactor};
use serde::{Deserialize, Serialize};

pub use source::{AgentSource, ClaudeCode, HookEvent};
pub use summary::Summary;
pub use transcript::Transcript;

/// What is stored about each session besides the transcript. Encrypted in
/// the store; its summary is also indexed for search.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SessionMeta {
    pub id: String,
    pub source: String,
    pub project: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended: Option<String>,
    pub captured_at: String,
    /// The hook event that triggered the latest capture.
    pub event: String,
    /// HEAD of the worktree at capture time, to tell later whether the
    /// session's work reached main.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head: Option<String>,
    /// Commits with a `Session:` trailer naming this session.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commits: Vec<String>,
    /// The content hash of the stored transcript blob.
    pub blob: String,
    /// Size of the redacted transcript in bytes, before compression.
    pub transcript_bytes: usize,
    /// Redactions made, by kind.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub redactions: std::collections::BTreeMap<String, usize>,
    /// The PM task this session worked on, when the PM layer is in use.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    pub summary: Summary,
}

impl Redactable for SessionMeta {
    fn redact_with(mut self, r: &Redactor) -> Result<Self, RedactError> {
        self.summary = self.summary.redact_with(r)?;
        self.branch = self.branch.redact_with(r)?;
        self.worktree = self.worktree.redact_with(r)?;
        Ok(self)
    }
}
