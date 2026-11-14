//! Agent sources (D-TBR6). Only Claude Code is built; others plug in behind
//! the same trait without touching storage or search.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// What a hook tells us.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookEvent {
    pub session_id: String,
    pub transcript_path: Option<PathBuf>,
    pub cwd: Option<PathBuf>,
    /// For example `Stop`, `PreCompact`, `SessionEnd`, `SessionStart`.
    pub event: String,
}

#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    #[error("hook input is not valid JSON: {0}")]
    BadInput(String),
    #[error("hook input has no session_id")]
    NoSession,
    #[error("could not read the transcript {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
}

pub trait AgentSource {
    /// A short stable name, stored with each session.
    fn name(&self) -> &'static str;
    /// Parse what the agent app passes to a hook on stdin.
    fn parse_hook_input(&self, input: &str) -> Result<HookEvent, SourceError>;
    /// Read the raw transcript text.
    fn read_transcript(&self, path: &Path) -> Result<String, SourceError>;
    /// Where this source keeps transcripts for a project directory, to find
    /// sessions that were never captured.
    fn transcripts_dir(&self, project_dir: &Path) -> Option<PathBuf>;
}

/// Claude Code (and the Claude desktop app's Code tab), which write JSONL
/// transcripts under `~/.claude/projects/<slug>/`.
pub struct ClaudeCode;

impl ClaudeCode {
    /// Claude Code's directory name for a project path: every character
    /// that isn't alphanumeric becomes `-`.
    pub fn project_slug(path: &Path) -> String {
        path.to_string_lossy()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect()
    }
}

impl AgentSource for ClaudeCode {
    fn name(&self) -> &'static str {
        "claude-code"
    }

    fn parse_hook_input(&self, input: &str) -> Result<HookEvent, SourceError> {
        let v: Value =
            serde_json::from_str(input).map_err(|e| SourceError::BadInput(e.to_string()))?;
        let session_id = v
            .get("session_id")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or(SourceError::NoSession)?
            .to_string();
        Ok(HookEvent {
            session_id,
            transcript_path: v
                .get("transcript_path")
                .and_then(Value::as_str)
                .map(PathBuf::from),
            cwd: v.get("cwd").and_then(Value::as_str).map(PathBuf::from),
            event: v
                .get("hook_event_name")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string(),
        })
    }

    fn read_transcript(&self, path: &Path) -> Result<String, SourceError> {
        std::fs::read(path)
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .map_err(|source| SourceError::Read {
                path: path.to_path_buf(),
                source,
            })
    }

    fn transcripts_dir(&self, project_dir: &Path) -> Option<PathBuf> {
        let home = inloop_core::config::home_dir()?;
        let canon = project_dir
            .canonicalize()
            .unwrap_or_else(|_| project_dir.to_path_buf());
        Some(
            home.join(".claude")
                .join("projects")
                .join(Self::project_slug(&canon)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hook_input() {
        let e = ClaudeCode
            .parse_hook_input(
                r#"{"session_id":"abc","transcript_path":"/t.jsonl","cwd":"/w","hook_event_name":"Stop"}"#,
            )
            .unwrap();
        assert_eq!(e.session_id, "abc");
        assert_eq!(e.event, "Stop");
        assert!(ClaudeCode.parse_hook_input("{}").is_err());
        assert!(ClaudeCode.parse_hook_input("nope").is_err());
    }

    #[test]
    fn slugs_match_claude_code() {
        assert_eq!(
            ClaudeCode::project_slug(Path::new("/Users/liam/Documents/InLoop")),
            "-Users-liam-Documents-InLoop"
        );
    }
}
