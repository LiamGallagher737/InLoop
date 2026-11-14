//! Session summaries, built from the redacted transcript with no model call
//! (D-proposed in docs/03): what was asked, what was done, the files
//! touched, the open ends.

use std::collections::BTreeMap;

use inloop_redact::{RedactError, Redactable, Redactor};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::transcript::Transcript;

const PROMPT_LIMIT: usize = 400;
const DONE_LIMIT: usize = 800;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    /// The first thing the person asked.
    pub asked: String,
    /// Later prompts, shortened.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub later_prompts: Vec<String>,
    /// The agent's last message.
    pub done: String,
    /// Files the agent edited or wrote, relative to the worktree when possible.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files_touched: Vec<String>,
    /// Tool calls by name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tools_used: BTreeMap<String, usize>,
    /// To-do items still open at the end.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub open_ends: Vec<String>,
    /// Decision IDs recorded in the session.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub decisions: Vec<String>,
    /// Number of conversation records.
    pub turns: usize,
}

fn shorten(s: &str, max: usize) -> String {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() <= max {
        s
    } else {
        let mut t: String = s.chars().take(max.saturating_sub(1)).collect();
        t.push('…');
        t
    }
}

/// Prompts that are app plumbing, not something a person typed.
fn is_plumbing(p: &str) -> bool {
    let t = p.trim_start();
    t.starts_with("<command-")
        || t.starts_with("<local-command")
        || t.starts_with("<system-reminder>")
        || t.starts_with("Caveat:")
        || t.starts_with("[Request interrupted")
}

impl Summary {
    /// Build a summary. File paths under any of `roots` become relative.
    pub fn build(t: &Transcript, roots: &[String]) -> Summary {
        let prompts: Vec<String> = t
            .records
            .iter()
            .filter_map(Transcript::prompt_text)
            .filter(|p| !is_plumbing(p))
            .collect();
        let asked = prompts
            .first()
            .map(|p| shorten(p, PROMPT_LIMIT))
            .unwrap_or_default();
        let later_prompts = prompts
            .iter()
            .skip(1)
            .take(8)
            .map(|p| shorten(p, 160))
            .collect();
        let done = t
            .records
            .iter()
            .rev()
            .find_map(Transcript::assistant_text)
            .map(|d| shorten(&d, DONE_LIMIT))
            .unwrap_or_default();

        let mut files: Vec<String> = Vec::new();
        let mut tools: BTreeMap<String, usize> = BTreeMap::new();
        let mut last_todos: Option<&Value> = None;
        for (_, name, input) in t.tool_uses() {
            *tools.entry(name.to_string()).or_default() += 1;
            let lname = name.to_ascii_lowercase();
            if matches!(
                lname.as_str(),
                "edit" | "write" | "multiedit" | "notebookedit"
            ) && let Some(p) = input
                .get("file_path")
                .or_else(|| input.get("notebook_path"))
                .and_then(Value::as_str)
            {
                let rel = roots
                    .iter()
                    .find_map(|w| p.strip_prefix(w.as_str()))
                    .map(|r| r.trim_start_matches('/').to_string())
                    .unwrap_or_else(|| p.to_string());
                if !files.contains(&rel) {
                    files.push(rel);
                }
            }
            if lname == "todowrite" || lname.ends_with("update_plan") {
                last_todos = input.get("todos").or_else(|| input.get("items"));
            }
        }
        let open_ends = last_todos
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter(|i| i.get("status").and_then(Value::as_str) != Some("completed"))
                    .filter_map(|i| i.get("content").and_then(Value::as_str))
                    .map(|c| shorten(c, 120))
                    .collect()
            })
            .unwrap_or_default();
        Summary {
            asked,
            later_prompts,
            done,
            files_touched: files,
            tools_used: tools,
            open_ends,
            decisions: Vec::new(),
            turns: t.records.len(),
        }
    }

    /// One line, for titles and lists.
    pub fn title(&self) -> String {
        if self.asked.is_empty() {
            "(no prompt)".into()
        } else {
            shorten(&self.asked, 90)
        }
    }

    /// The text that gets indexed for search.
    pub fn search_text(&self) -> String {
        let mut parts = vec![self.asked.clone()];
        parts.extend(self.later_prompts.iter().cloned());
        parts.push(self.done.clone());
        parts.extend(self.files_touched.iter().cloned());
        parts.extend(self.open_ends.iter().cloned());
        parts.extend(self.decisions.iter().cloned());
        parts.retain(|p| !p.is_empty());
        parts.join("\n")
    }
}

impl Redactable for Summary {
    fn redact_with(self, r: &Redactor) -> Result<Self, RedactError> {
        Ok(Summary {
            asked: r.redact_text(&self.asked)?,
            later_prompts: self.later_prompts.redact_with(r)?,
            done: r.redact_text(&self.done)?,
            files_touched: self.files_touched.redact_with(r)?,
            tools_used: self.tools_used,
            open_ends: self.open_ends.redact_with(r)?,
            decisions: self.decisions,
            turns: self.turns,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarises_a_session() {
        let jsonl = r#"{"type":"user","message":{"content":"<command-name>/clear</command-name>"}}
{"type":"user","message":{"content":"Add a retry to the uploader"}}
{"type":"assistant","message":{"content":[{"type":"tool_use","id":"a","name":"TodoWrite","input":{"todos":[{"content":"Write retry","status":"completed"},{"content":"Add tests","status":"in_progress"}]}}]}}
{"type":"assistant","message":{"content":[{"type":"tool_use","id":"b","name":"Edit","input":{"file_path":"/w/src/up.rs"}}]}}
{"type":"assistant","message":{"content":[{"type":"tool_use","id":"c","name":"Edit","input":{"file_path":"/w/src/up.rs"}}]}}
{"type":"user","message":{"content":"Also log failures"}}
{"type":"assistant","message":{"content":[{"type":"text","text":"Done: retries with backoff."}]}}
"#;
        let t = Transcript::parse_jsonl(jsonl);
        let s = Summary::build(&t, &["/w".to_string()]);
        assert_eq!(s.asked, "Add a retry to the uploader");
        assert_eq!(s.later_prompts, vec!["Also log failures"]);
        assert_eq!(s.done, "Done: retries with backoff.");
        assert_eq!(s.files_touched, vec!["src/up.rs"]);
        assert_eq!(s.tools_used.get("Edit"), Some(&2));
        assert_eq!(s.open_ends, vec!["Add tests"]);
        assert!(s.search_text().contains("uploader"));
    }
}
