//! The InLoop redactor.
//!
//! Everything InLoop stores passes through a [`Redactor`] first (D-G1R6). The
//! storage layer only accepts [`Redacted<T>`], and the only way to make one is
//! [`Redactor::redact`] (D-VNK6), so a write path that skips redaction does not
//! compile.
//!
//! The redactor runs five layers (docs/04-redaction.md):
//!
//! 1. **Known values**: exact secret values loaded from `.env`-style files and
//!    secret-looking environment variables, plus their base64, URL-encoded and
//!    JSON-escaped forms.
//! 2. **Provider patterns**: the gitleaks default ruleset (vendored, MIT) plus a
//!    small built-in set that guarantees the formats docs/04 names.
//! 3. **Context plus entropy**: high-entropy values after `password=`,
//!    `Authorization: Bearer`, `api_key:` and similar, and passwords embedded
//!    in URLs.
//! 4. **Whole outputs**: the output of commands such as `env` or `cat .env` is
//!    dropped entirely ([`Redactor::redact_command_output`]).
//! 5. **Project rules**: extra patterns from `inloop.toml`, and optional rules
//!    for personal data.
//!
//! Each match becomes `[REDACTED:<kind>]`. Redaction is idempotent: running it
//! over its own output changes nothing.
//!
//! # The gate
//!
//! `Redacted<T>` cannot be built outside this crate:
//!
//! ```compile_fail
//! use inloop_redact::Redacted;
//! // The field is private, so this does not compile.
//! let sneaky = Redacted(String::from("ghp_not_redacted"));
//! ```
//!
//! Nor can it be deserialised, which would be a second way in:
//!
//! ```compile_fail
//! use inloop_redact::Redacted;
//! let sneaky: Redacted<String> = serde_json::from_str("\"x\"").unwrap();
//! ```
//!
//! The only way in is through the redactor:
//!
//! ```
//! use inloop_redact::{Redactor, RedactorConfig};
//! # fn main() -> Result<(), inloop_redact::RedactError> {
//! let config = RedactorConfig { include_process_env: false, ..RedactorConfig::default() };
//! let redactor = Redactor::new(&config)?;
//! let safe = redactor.redact(String::from("token=ghp_EXAMPLEfakeTOKEN0000aaaabbbbccccdddd"))?;
//! assert_eq!(safe.get(), "token=[REDACTED:github-pat]");
//! # Ok(())
//! # }
//! ```

#![forbid(unsafe_code)]

mod commands;
mod context;
mod entropy;
mod known;
mod project;
mod provider;

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Settings for [`Redactor::new`], normally read from the `[redaction]` table
/// of `inloop.toml` (docs/07-config.md).
#[derive(Clone, Debug)]
pub struct RedactorConfig {
    /// Repo root; known_value_files globs are relative to it.
    pub root: PathBuf,
    /// Globs of files whose values are always redacted. Default [".env", ".env.*"].
    ///
    /// A glob without a `/` matches a file of that name at any depth under
    /// `root` (as in `.gitignore`); a glob with a `/` matches the path relative
    /// to `root`.
    pub known_value_files: Vec<String>,
    /// Extra project rules from inloop.toml.
    pub patterns: Vec<PatternRule>,
    /// Turn on personal-data rules (emails, phone numbers and similar).
    pub personal_data: bool,
    /// Also load secret-looking environment variables of this process as known values.
    pub include_process_env: bool,
}

impl Default for RedactorConfig {
    fn default() -> Self {
        RedactorConfig {
            root: PathBuf::from("."),
            known_value_files: vec![".env".to_string(), ".env.*".to_string()],
            patterns: Vec::new(),
            personal_data: false,
            include_process_env: true,
        }
    }
}

/// A project redaction rule from `inloop.toml`.
///
/// The whole match is redacted, unless the regex has a capture group named
/// `secret`, in which case only that group is. `kind` is what appears in the
/// marker (`[REDACTED:<kind>]`) and may contain ASCII letters, digits, `_`,
/// `.` and `-`.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PatternRule {
    pub kind: String,
    pub regex: String,
}

/// A value that has been through the redactor.
///
/// There is no public constructor, and it implements none of `Deserialize`,
/// `DerefMut`, `From` or `Default` (D-VNK6): [`Redactor::redact`] is the only
/// way to make one. Reading stored data back gives a plain `T`, which has to go
/// through the redactor again before it can be stored again.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Redacted<T>(T);

impl<T> Redacted<T> {
    /// Borrow the redacted value.
    pub fn get(&self) -> &T {
        &self.0
    }

    /// Take the redacted value out. The result is a plain `T` again.
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T: Serialize> Serialize for Redacted<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

/// Something the redactor knows how to clean.
pub trait Redactable: Sized {
    /// Redact every piece of text in self using `r.redact_text`.
    fn redact_with(self, r: &Redactor) -> Result<Self, RedactError>;
}

impl Redactable for String {
    fn redact_with(self, r: &Redactor) -> Result<Self, RedactError> {
        r.redact_text(&self)
    }
}

impl<T: Redactable> Redactable for Option<T> {
    fn redact_with(self, r: &Redactor) -> Result<Self, RedactError> {
        self.map(|v| v.redact_with(r)).transpose()
    }
}

impl<T: Redactable> Redactable for Vec<T> {
    fn redact_with(self, r: &Redactor) -> Result<Self, RedactError> {
        self.into_iter().map(|v| v.redact_with(r)).collect()
    }
}

impl<T: Redactable> Redactable for BTreeMap<String, T> {
    /// Values only: keys of a `BTreeMap` are taken to be field names chosen by
    /// InLoop, not user data.
    fn redact_with(self, r: &Redactor) -> Result<Self, RedactError> {
        self.into_iter()
            .map(|(k, v)| Ok((k, v.redact_with(r)?)))
            .collect()
    }
}

impl Redactable for serde_json::Value {
    /// Every string value and every object key, recursively. If two keys of
    /// one object redact to the same text, the later ones get a `#2`, `#3`...
    /// suffix so that no value is lost.
    fn redact_with(self, r: &Redactor) -> Result<Self, RedactError> {
        use serde_json::Value;
        Ok(match self {
            Value::String(s) => Value::String(r.redact_text(&s)?),
            Value::Array(items) => Value::Array(
                items
                    .into_iter()
                    .map(|v| v.redact_with(r))
                    .collect::<Result<_, _>>()?,
            ),
            Value::Object(map) => {
                let mut out = serde_json::Map::with_capacity(map.len());
                for (k, v) in map {
                    let mut key = r.redact_text(&k)?;
                    if out.contains_key(&key) {
                        let mut n = 2usize;
                        while out.contains_key(&format!("{key}#{n}")) {
                            n += 1;
                        }
                        key = format!("{key}#{n}");
                    }
                    out.insert(key, v.redact_with(r)?);
                }
                Value::Object(out)
            }
            other => other,
        })
    }
}

/// Why the redactor could not be built or could not run.
///
/// Messages never include secret values: only paths, rule kinds, globs and
/// regex errors for patterns that come from configuration.
#[derive(Debug, thiserror::Error)]
pub enum RedactError {
    /// The configuration is invalid.
    #[error("redaction config: {0}")]
    Config(String),
    /// A known-value file exists but could not be read. Redaction fails closed.
    #[error("could not read known-value file {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// A `known_value_files` glob is invalid.
    #[error("invalid known_value_files glob {glob:?}: {message}")]
    BadGlob { glob: String, message: String },
    /// A project pattern does not compile.
    #[error("invalid redaction pattern for kind {kind:?}: {message}")]
    BadPattern { kind: String, message: String },
    /// A bundled rule file could not be loaded. This is a bug in InLoop.
    #[error("bundled redaction rules are invalid: {0}")]
    Rules(String),
    /// Building a matcher failed.
    #[error("could not build redaction matcher: {0}")]
    Matcher(String),
}

/// One thing the redactor would remove. Offsets are bytes into the input.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Finding {
    pub kind: String,
    pub start: usize,
    pub end: usize,
}

/// How many matches of each kind one redaction removed.
#[derive(Clone, Debug, Default, Serialize)]
pub struct RedactionReport {
    pub counts: BTreeMap<String, usize>,
}

/// Rule counts, for status output.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct RuleStats {
    /// Provider rules compiled: gitleaks rules plus InLoop's built-in rules.
    pub provider_rules: usize,
    /// Gitleaks rules not used, because they do not compile under the Rust
    /// regex engine or only make sense with a file path.
    pub skipped_rules: usize,
    /// Rules from the project's `inloop.toml`, plus personal-data rules when on.
    pub project_rules: usize,
}

/// A candidate match from one layer, before overlaps are resolved.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Span<'a> {
    pub start: usize,
    pub end: usize,
    pub kind: &'a str,
    /// Lower wins when two spans start at the same place and are equally long.
    pub priority: u8,
}

pub(crate) const PRIORITY_GITLEAKS: u8 = 0;
pub(crate) const PRIORITY_BUILTIN: u8 = 1;
pub(crate) const PRIORITY_GENERIC: u8 = 2;
pub(crate) const PRIORITY_KNOWN: u8 = 3;
pub(crate) const PRIORITY_CONTEXT: u8 = 4;
pub(crate) const PRIORITY_PROJECT: u8 = 5;

const COMMAND_OUTPUT_MARKER: &str = "[REDACTED:command-output]";

/// The redactor. Build one per process with [`Redactor::new`] and share it;
/// it is `Send + Sync`.
pub struct Redactor {
    known: known::KnownValues,
    provider: provider::ProviderRules,
    context: context::ContextLayer,
    commands: commands::CommandPolicy,
    project: project::ProjectRules,
    marker: regex::bytes::Regex,
}

impl fmt::Debug for Redactor {
    /// Counts only. Known values are never printed.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Redactor")
            .field("known_values", &self.known_value_count())
            .field("rule_stats", &self.rule_stats())
            .finish_non_exhaustive()
    }
}

impl Redactor {
    /// Build a redactor: load known values, compile every rule. Fails closed:
    /// an unreadable known-value file or an invalid project pattern is an error.
    pub fn new(config: &RedactorConfig) -> Result<Redactor, RedactError> {
        let commands = commands::CommandPolicy::new(&config.known_value_files)?;
        let known = known::KnownValues::load(config)?;
        let provider = provider::ProviderRules::load()?;
        let context = context::ContextLayer::new()?;
        let project = project::ProjectRules::new(&config.patterns, config.personal_data)?;
        let marker = regex::bytes::Regex::new(r"\[REDACTED:[A-Za-z0-9_.:-]{1,80}\]")
            .map_err(|e| RedactError::Rules(e.to_string()))?;
        Ok(Redactor {
            known,
            provider,
            context,
            commands,
            project,
            marker,
        })
    }

    /// The gate: the only way to make a Redacted<T>.
    pub fn redact<T: Redactable>(&self, value: T) -> Result<Redacted<T>, RedactError> {
        Ok(Redacted(value.redact_with(self)?))
    }

    /// Redact one string. Each match becomes "[REDACTED:<kind>]".
    pub fn redact_text(&self, text: &str) -> Result<String, RedactError> {
        Ok(self.redact_text_with_report(text)?.0)
    }

    /// Same, plus counts per kind.
    pub fn redact_text_with_report(
        &self,
        text: &str,
    ) -> Result<(String, RedactionReport), RedactError> {
        let spans = self.resolved_spans(text);
        let mut report = RedactionReport::default();
        if spans.is_empty() {
            return Ok((text.to_string(), report));
        }
        let mut out = String::with_capacity(text.len());
        let mut pos = 0;
        for s in &spans {
            let (Some(before), Some(_)) = (text.get(pos..s.start), text.get(s.start..s.end)) else {
                return Err(RedactError::Matcher(
                    "a match did not fall on character boundaries".to_string(),
                ));
            };
            out.push_str(before);
            out.push_str("[REDACTED:");
            out.push_str(s.kind);
            out.push(']');
            *report.counts.entry(s.kind.to_string()).or_insert(0) += 1;
            pos = s.end;
        }
        out.push_str(text.get(pos..).unwrap_or_default());
        Ok((out, report))
    }

    /// Find without changing, for `inloop scan`. Non-overlapping, sorted by start.
    pub fn find(&self, text: &str) -> Vec<Finding> {
        self.resolved_spans(text)
            .into_iter()
            .map(|s| Finding {
                kind: s.kind.to_string(),
                start: s.start,
                end: s.end,
            })
            .collect()
    }

    /// Layer 4: true for commands whose whole output must be dropped (env,
    /// printenv, `cat .env`, keychain and cloud credential commands and so on).
    /// Understands pipes, `&&`, `;`, subshells, `sudo`, leading `VAR=value`
    /// assignments, wrappers such as `docker exec` and `bash -c`, and absolute
    /// paths of the binary.
    pub fn is_secret_dumping_command(&self, command: &str) -> bool {
        self.commands.is_secret_dumping(command)
    }

    /// Layer 4 in use: "[REDACTED:command-output]" if the command dumps
    /// secrets, else `redact_text(output)`.
    pub fn redact_command_output(
        &self,
        command: &str,
        output: &str,
    ) -> Result<String, RedactError> {
        if self.is_secret_dumping_command(command) {
            Ok(COMMAND_OUTPUT_MARKER.to_string())
        } else {
            self.redact_text(output)
        }
    }

    /// Number of known values loaded (for `inloop` status output). Never
    /// exposes the values. Each value counts once, however many encoded forms
    /// of it are matched.
    pub fn known_value_count(&self) -> usize {
        self.known.count()
    }

    /// Number of provider rules compiled, and how many from the gitleaks
    /// ruleset were skipped as incompatible.
    pub fn rule_stats(&self) -> RuleStats {
        RuleStats {
            provider_rules: self.provider.compiled(),
            skipped_rules: self.provider.skipped(),
            project_rules: self.project.count(),
        }
    }

    /// Every layer's matches, merged into sorted, non-overlapping spans that
    /// never touch an existing `[REDACTED:...]` marker.
    fn resolved_spans<'a>(&'a self, text: &str) -> Vec<Span<'a>> {
        let bytes = text.as_bytes();
        let mut spans: Vec<Span<'a>> = Vec::new();
        self.known.find(bytes, &mut spans);
        self.provider.find(bytes, &mut spans);
        self.context.find(bytes, &mut spans);
        self.project.find(text, &mut spans);
        if spans.is_empty() {
            return spans;
        }
        for s in &mut spans {
            while s.start > 0 && !text.is_char_boundary(s.start) {
                s.start -= 1;
            }
            while s.end < text.len() && !text.is_char_boundary(s.end) {
                s.end += 1;
            }
            s.end = s.end.min(text.len());
        }
        spans.retain(|s| s.end > s.start);
        let merged = merge_spans(spans);
        let markers: Vec<(usize, usize)> = self
            .marker
            .find_iter(bytes)
            .map(|m| (m.start(), m.end()))
            .collect();
        if markers.is_empty() {
            merged
        } else {
            subtract_markers(merged, &markers)
        }
    }
}

/// Earliest start first, longest first, then priority; any overlap is folded
/// into the earlier span so every matched byte is covered.
fn merge_spans(mut spans: Vec<Span<'_>>) -> Vec<Span<'_>> {
    spans.sort_by(|a, b| {
        a.start
            .cmp(&b.start)
            .then((b.end - b.start).cmp(&(a.end - a.start)))
            .then(a.priority.cmp(&b.priority))
    });
    let mut out: Vec<Span<'_>> = Vec::with_capacity(spans.len());
    for s in spans {
        match out.last_mut() {
            Some(last) if s.start < last.end => {
                last.end = last.end.max(s.end);
            }
            _ => out.push(s),
        }
    }
    out
}

/// Cut existing markers out of spans, so redacting redacted text changes
/// nothing.
fn subtract_markers<'a>(spans: Vec<Span<'a>>, markers: &[(usize, usize)]) -> Vec<Span<'a>> {
    let mut out = Vec::with_capacity(spans.len());
    for s in spans {
        let mut start = s.start;
        for &(ms, me) in markers {
            if me <= start {
                continue;
            }
            if ms >= s.end {
                break;
            }
            if ms > start {
                out.push(Span {
                    start,
                    end: ms,
                    ..s
                });
            }
            start = start.max(me);
        }
        if start < s.end {
            out.push(Span { start, ..s });
        }
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn span(start: usize, end: usize, kind: &str, priority: u8) -> Span<'_> {
        Span {
            start,
            end,
            kind,
            priority,
        }
    }

    #[test]
    fn merge_prefers_earliest_then_longest_and_covers_overlaps() {
        let merged = merge_spans(vec![
            span(5, 10, "b", 1),
            span(0, 4, "a", 1),
            span(5, 12, "c", 2),
            span(11, 20, "d", 0),
        ]);
        let got: Vec<_> = merged.iter().map(|s| (s.start, s.end, s.kind)).collect();
        assert_eq!(got, vec![(0, 4, "a"), (5, 20, "c")]);
    }

    #[test]
    fn equal_spans_take_the_lower_priority_number() {
        let merged = merge_spans(vec![span(0, 4, "builtin", 2), span(0, 4, "gitleaks", 1)]);
        assert_eq!(merged[0].kind, "gitleaks");
    }

    #[test]
    fn markers_are_cut_out_of_spans() {
        let out = subtract_markers(vec![span(0, 30, "x", 0)], &[(5, 10), (20, 25)]);
        let got: Vec<_> = out.iter().map(|s| (s.start, s.end)).collect();
        assert_eq!(got, vec![(0, 5), (10, 20), (25, 30)]);
    }

    #[test]
    fn redactor_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Redactor>();
        assert_send_sync::<Redacted<String>>();
    }
}
