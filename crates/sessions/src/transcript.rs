//! Transcripts: the conversation records of a session, as JSON lines.

use std::collections::HashSet;

use inloop_redact::{RedactError, Redactable, Redactor};
use serde_json::Value;

/// Record types kept from a Claude Code transcript. Everything else is the
/// app's own bookkeeping (API request shapes, file history, UI state) and
/// is dropped before storage.
pub const KEPT_TYPES: &[&str] = &["user", "assistant", "system", "summary"];

/// The marker that replaces the whole output of a command that dumps
/// secrets (docs/04, layer 4).
pub const COMMAND_OUTPUT_MARKER: &str = "[REDACTED:command-output]";

/// A transcript: one JSON value per conversation record.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Transcript {
    pub records: Vec<Value>,
    /// Lines that weren't valid JSON, kept as text so nothing is silently
    /// lost (they're redacted too).
    pub raw_lines: Vec<String>,
    /// How many records were dropped as bookkeeping.
    pub dropped: usize,
}

impl Transcript {
    /// Parse JSONL, keeping conversation records only.
    pub fn parse_jsonl(text: &str) -> Transcript {
        let mut t = Transcript::default();
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            match serde_json::from_str::<Value>(line) {
                Ok(v) => {
                    let kind = v.get("type").and_then(Value::as_str).unwrap_or("");
                    if KEPT_TYPES.contains(&kind) {
                        t.records.push(v);
                    } else {
                        t.dropped += 1;
                    }
                }
                Err(_) => t.raw_lines.push(line.to_string()),
            }
        }
        t
    }

    /// Serialise back to JSONL.
    pub fn to_jsonl(&self) -> String {
        let mut out = String::new();
        for r in &self.records {
            if let Ok(s) = serde_json::to_string(r) {
                out.push_str(&s);
                out.push('\n');
            }
        }
        for l in &self.raw_lines {
            out.push_str(l);
            out.push('\n');
        }
        out
    }

    /// Content blocks of a record's message, or an empty slice.
    pub fn blocks(record: &Value) -> &[Value] {
        record
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// The plain text a person typed, for user records that are prompts
    /// (not tool results).
    pub fn prompt_text(record: &Value) -> Option<String> {
        if record.get("type").and_then(Value::as_str) != Some("user") {
            return None;
        }
        if record.get("isMeta").and_then(Value::as_bool) == Some(true) {
            return None;
        }
        let content = record.get("message")?.get("content")?;
        let text = match content {
            Value::String(s) => s.clone(),
            Value::Array(blocks) => {
                if blocks
                    .iter()
                    .any(|b| b.get("type").and_then(Value::as_str) == Some("tool_result"))
                {
                    return None;
                }
                blocks
                    .iter()
                    .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
                    .filter_map(|b| b.get("text").and_then(Value::as_str))
                    .collect::<Vec<_>>()
                    .join("\n")
            }
            _ => return None,
        };
        let text = text.trim().to_string();
        if text.is_empty() { None } else { Some(text) }
    }

    /// The text an assistant record says to the user.
    pub fn assistant_text(record: &Value) -> Option<String> {
        if record.get("type").and_then(Value::as_str) != Some("assistant") {
            return None;
        }
        let text: Vec<&str> = Self::blocks(record)
            .iter()
            .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|b| b.get("text").and_then(Value::as_str))
            .collect();
        let text = text.join("\n").trim().to_string();
        if text.is_empty() { None } else { Some(text) }
    }

    /// Every tool call: (id, name, input).
    pub fn tool_uses(&self) -> impl Iterator<Item = (&str, &str, &Value)> {
        self.records.iter().flat_map(|r| {
            Self::blocks(r).iter().filter_map(|b| {
                if b.get("type").and_then(Value::as_str) != Some("tool_use") {
                    return None;
                }
                Some((
                    b.get("id").and_then(Value::as_str).unwrap_or(""),
                    b.get("name").and_then(Value::as_str).unwrap_or(""),
                    b.get("input").unwrap_or(&Value::Null),
                ))
            })
        })
    }

    /// First and last record timestamps.
    pub fn time_span(&self) -> (Option<String>, Option<String>) {
        let mut ts = self
            .records
            .iter()
            .filter_map(|r| r.get("timestamp").and_then(Value::as_str));
        let first = ts.next().map(str::to_string);
        let last = self
            .records
            .iter()
            .rev()
            .find_map(|r| r.get("timestamp").and_then(Value::as_str))
            .map(str::to_string);
        let last = last.or_else(|| first.clone());
        (first, last)
    }
}

/// Tool names whose input has a shell command.
fn shell_command(name: &str, input: &Value) -> Option<String> {
    let n = name.to_ascii_lowercase();
    if n == "bash" || n.ends_with("__bash") || n == "shell" || n.contains("run_in_terminal") {
        input
            .get("command")
            .and_then(Value::as_str)
            .map(str::to_string)
    } else {
        None
    }
}

impl Redactable for Transcript {
    /// Layer 4 first: drop the whole output of commands that dump secrets.
    /// Then every string in every record goes through the redactor.
    fn redact_with(mut self, r: &Redactor) -> Result<Self, RedactError> {
        let dumping: HashSet<String> = self
            .tool_uses()
            .filter_map(|(id, name, input)| {
                shell_command(name, input)
                    .filter(|c| r.is_secret_dumping_command(c))
                    .map(|_| id.to_string())
            })
            .collect();
        if !dumping.is_empty() {
            for rec in &mut self.records {
                let mut hit = false;
                if let Some(blocks) = rec
                    .get_mut("message")
                    .and_then(|m| m.get_mut("content"))
                    .and_then(Value::as_array_mut)
                {
                    for b in blocks {
                        let is_dump = b.get("type").and_then(Value::as_str) == Some("tool_result")
                            && b.get("tool_use_id")
                                .and_then(Value::as_str)
                                .is_some_and(|id| dumping.contains(id));
                        if is_dump {
                            b["content"] = Value::String(COMMAND_OUTPUT_MARKER.into());
                            hit = true;
                        }
                    }
                }
                if hit
                    && let Some(obj) = rec.as_object_mut()
                    && obj.contains_key("toolUseResult")
                {
                    obj.insert(
                        "toolUseResult".into(),
                        Value::String(COMMAND_OUTPUT_MARKER.into()),
                    );
                }
            }
        }
        let mut records = Vec::with_capacity(self.records.len());
        for rec in self.records {
            records.push(rec.redact_with(r)?);
        }
        let mut raw = Vec::with_capacity(self.raw_lines.len());
        for l in self.raw_lines {
            raw.push(r.redact_text(&l)?);
        }
        Ok(Transcript {
            records,
            raw_lines: raw,
            dropped: self.dropped,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use inloop_redact::RedactorConfig;

    const SAMPLE: &str = r#"{"type":"user","message":{"role":"user","content":"Set up billing with key sk_live_51HxEXAMPLEEXAMPLEEXAMPLEab12"},"timestamp":"2026-11-14T10:00:00Z","sessionId":"s1","cwd":"/x","gitBranch":"main"}
{"type":"api-request","params":{"huge":"bookkeeping"}}
{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Checking the environment."},{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"printenv"}}]},"timestamp":"2026-11-14T10:00:05Z"}
{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"HOME=/x\nSOME_SETTING=value123456789"}]},"toolUseResult":{"stdout":"HOME=/x\nSOME_SETTING=value123456789"},"timestamp":"2026-11-14T10:00:06Z"}
{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t2","name":"Edit","input":{"file_path":"/x/src/billing.rs","old_string":"a","new_string":"b"}}]},"timestamp":"2026-11-14T10:01:00Z"}
not json at all
"#;

    fn redactor() -> Redactor {
        let cfg = RedactorConfig {
            include_process_env: false,
            root: std::env::temp_dir(),
            ..Default::default()
        };
        Redactor::new(&cfg).unwrap()
    }

    #[test]
    fn parses_and_drops_bookkeeping() {
        let t = Transcript::parse_jsonl(SAMPLE);
        assert_eq!(t.records.len(), 4);
        assert_eq!(t.dropped, 1);
        assert_eq!(t.raw_lines, vec!["not json at all"]);
        assert!(
            Transcript::prompt_text(&t.records[0])
                .unwrap()
                .starts_with("Set up billing")
        );
        assert_eq!(Transcript::prompt_text(&t.records[2]), None);
        assert_eq!(
            Transcript::assistant_text(&t.records[1]).as_deref(),
            Some("Checking the environment.")
        );
        assert_eq!(t.tool_uses().count(), 2);
        let (a, b) = t.time_span();
        assert_eq!(a.as_deref(), Some("2026-11-14T10:00:00Z"));
        assert_eq!(b.as_deref(), Some("2026-11-14T10:01:00Z"));
    }

    #[test]
    fn redaction_drops_dumped_env_and_cleans_text() {
        let t = Transcript::parse_jsonl(SAMPLE);
        let red = redactor().redact(t).unwrap().into_inner();
        let out = red.to_jsonl();
        assert!(!out.contains("sk_live_51HxEXAMPLE"), "{out}");
        assert!(!out.contains("value123456789"), "{out}");
        assert!(out.contains(COMMAND_OUTPUT_MARKER));
        assert!(out.contains("billing.rs"));
    }
}
