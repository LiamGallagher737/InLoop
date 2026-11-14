//! The decision file format (docs/02): YAML frontmatter plus Markdown.

use std::collections::BTreeMap;
use std::fmt;

use inloop_core::Id;
use serde::{Deserialize, Serialize};

/// A decision's status.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Proposed,
    Accepted,
    Rejected,
    Superseded,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Proposed => "proposed",
            Status::Accepted => "accepted",
            Status::Rejected => "rejected",
            Status::Superseded => "superseded",
        }
    }

    pub fn parse(s: &str) -> Option<Status> {
        match s.trim().to_ascii_lowercase().as_str() {
            "proposed" => Some(Status::Proposed),
            "accepted" => Some(Status::Accepted),
            "rejected" => Some(Status::Rejected),
            "superseded" => Some(Status::Superseded),
            _ => None,
        }
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One decision, as stored in `D-XXXX-slug.md`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Decision {
    pub id: Id,
    pub title: String,
    pub status: Status,
    pub date: String,
    pub decided_by: String,
    pub basis: Option<String>,
    pub session: Option<String>,
    pub supersedes: Vec<Id>,
    pub superseded_by: Option<Id>,
    /// Other IDs that resolve to this decision, such as an old sequential
    /// number (D-P5PT). Kept as written.
    pub aliases: Vec<String>,
    pub tags: Vec<String>,
    /// Everything after the frontmatter.
    pub body: String,
    /// Frontmatter keys InLoop doesn't know. Lint reports them.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub unknown_keys: Vec<String>,
}

#[derive(Deserialize)]
struct Frontmatter {
    id: String,
    title: String,
    status: String,
    #[serde(default)]
    date: Option<serde_yaml_ng::Value>,
    #[serde(default)]
    decided_by: Option<String>,
    #[serde(default)]
    basis: Option<String>,
    #[serde(default)]
    session: Option<serde_yaml_ng::Value>,
    #[serde(default)]
    supersedes: Option<Vec<String>>,
    #[serde(default)]
    superseded_by: Option<String>,
    #[serde(default)]
    aliases: Option<Vec<serde_yaml_ng::Value>>,
    #[serde(default)]
    tags: Option<Vec<serde_yaml_ng::Value>>,
    #[serde(flatten)]
    extra: BTreeMap<String, serde_yaml_ng::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseError {
    #[error("no frontmatter: the file must start with a `---` line")]
    NoFrontmatter,
    #[error("frontmatter is not closed with a `---` line")]
    UnclosedFrontmatter,
    #[error("frontmatter is not valid: {0}")]
    Yaml(String),
    #[error("`{field}` is not valid: {message}")]
    Field {
        field: &'static str,
        message: String,
    },
}

fn value_to_string(v: &serde_yaml_ng::Value) -> Option<String> {
    match v {
        serde_yaml_ng::Value::Null => None,
        serde_yaml_ng::Value::String(s) => Some(s.clone()),
        serde_yaml_ng::Value::Number(n) => Some(n.to_string()),
        serde_yaml_ng::Value::Bool(b) => Some(b.to_string()),
        other => serde_yaml_ng::to_string(other)
            .ok()
            .map(|s| s.trim().to_string()),
    }
}

/// Split a file into its frontmatter text and body.
pub fn split_frontmatter(text: &str) -> Result<(&str, &str), ParseError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let rest = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
        .ok_or(ParseError::NoFrontmatter)?;
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        if line.trim_end() == "---" {
            let fm = &rest[..offset];
            let body = &rest[offset + line.len()..];
            return Ok((fm, body));
        }
        offset += line.len();
    }
    Err(ParseError::UnclosedFrontmatter)
}

impl Decision {
    /// Parse a decision file.
    pub fn parse(text: &str) -> Result<Decision, ParseError> {
        let (fm, body) = split_frontmatter(text)?;
        let f: Frontmatter =
            serde_yaml_ng::from_str(fm).map_err(|e| ParseError::Yaml(e.to_string()))?;
        let id = Id::parse_loose(&f.id).map_err(|e| ParseError::Field {
            field: "id",
            message: e.to_string(),
        })?;
        let status = Status::parse(&f.status).ok_or_else(|| ParseError::Field {
            field: "status",
            message: format!(
                "`{}`: expected proposed, accepted, rejected or superseded",
                f.status
            ),
        })?;
        let parse_ids = |field: &'static str, v: Vec<String>| -> Result<Vec<Id>, ParseError> {
            v.iter()
                .map(|s| {
                    Id::parse_loose(s).map_err(|e| ParseError::Field {
                        field,
                        message: e.to_string(),
                    })
                })
                .collect()
        };
        let superseded_by = match f.superseded_by.as_deref().map(str::trim) {
            None | Some("") | Some("null") => None,
            Some(s) => Some(Id::parse_loose(s).map_err(|e| ParseError::Field {
                field: "superseded_by",
                message: e.to_string(),
            })?),
        };
        Ok(Decision {
            id,
            title: f.title.trim().to_string(),
            status,
            date: f
                .date
                .as_ref()
                .and_then(value_to_string)
                .unwrap_or_default(),
            decided_by: f.decided_by.unwrap_or_default(),
            basis: f.basis.filter(|b| !b.trim().is_empty()),
            session: f.session.as_ref().and_then(value_to_string),
            supersedes: parse_ids("supersedes", f.supersedes.unwrap_or_default())?,
            superseded_by,
            aliases: f
                .aliases
                .unwrap_or_default()
                .iter()
                .filter_map(value_to_string)
                .collect(),
            tags: f
                .tags
                .unwrap_or_default()
                .iter()
                .filter_map(value_to_string)
                .collect(),
            body: body.to_string(),
            unknown_keys: f.extra.into_keys().collect(),
        })
    }

    /// Render a new decision file in the canonical format.
    pub fn render(&self) -> String {
        let mut s = String::from("---\n");
        s.push_str(&format!("id: {}\n", self.id));
        s.push_str(&format!("title: {}\n", yaml_string(&self.title)));
        s.push_str(&format!("status: {}\n", self.status));
        s.push_str(&format!("date: {}\n", self.date));
        s.push_str(&format!("decided_by: {}\n", yaml_scalar(&self.decided_by)));
        s.push_str(&format!(
            "basis: {}\n",
            yaml_opt_string(self.basis.as_deref())
        ));
        s.push_str(&format!(
            "session: {}\n",
            yaml_opt_scalar(self.session.as_deref())
        ));
        s.push_str(&format!("supersedes: {}\n", yaml_list(&self.supersedes)));
        s.push_str(&format!(
            "superseded_by: {}\n",
            self.superseded_by
                .as_ref()
                .map(|i| i.to_string())
                .unwrap_or_else(|| "null".into())
        ));
        s.push_str(&format!("aliases: {}\n", yaml_list(&self.aliases)));
        s.push_str(&format!("tags: {}\n", yaml_list(&self.tags)));
        s.push_str("---\n");
        if !self.body.starts_with('\n') {
            s.push('\n');
        }
        s.push_str(&self.body);
        if !s.ends_with('\n') {
            s.push('\n');
        }
        s
    }

    /// The file name for this decision: `D-7KQ2-snap-writes-go-through-one-helper.md`.
    pub fn file_name(&self) -> String {
        file_name(&self.id, &self.title)
    }

    /// The text of a `## Heading` section of the body, if present.
    pub fn section(&self, heading: &str) -> Option<String> {
        section(&self.body, heading)
    }

    /// True if `text` names this decision, by ID or alias.
    pub fn answers_to(&self, text: &str) -> bool {
        let text = text.trim();
        if let Ok(id) = Id::parse_loose(text)
            && id == self.id
        {
            return true;
        }
        self.aliases.iter().any(|a| a.eq_ignore_ascii_case(text))
    }
}

/// The body section under `## <heading>`, up to the next `## ` heading.
pub fn section(body: &str, heading: &str) -> Option<String> {
    let mut out: Option<String> = None;
    for line in body.lines() {
        if let Some(h) = line.strip_prefix("## ") {
            if out.is_some() {
                break;
            }
            if h.trim().eq_ignore_ascii_case(heading) {
                out = Some(String::new());
            }
            continue;
        }
        if let Some(o) = out.as_mut() {
            o.push_str(line);
            o.push('\n');
        }
    }
    out.map(|s| s.trim().to_string())
}

/// `D-7KQ2` and a title become `D-7KQ2-short-slug.md`.
pub fn file_name(id: &Id, title: &str) -> String {
    let slug = slugify(title, 60);
    if slug.is_empty() {
        format!("{id}.md")
    } else {
        format!("{id}-{slug}.md")
    }
}

/// Lower-case words joined by dashes, cut at a word boundary near `max`.
pub fn slugify(title: &str, max: usize) -> String {
    let mut words = Vec::new();
    let mut len = 0;
    for word in title
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
    {
        let w = word.to_ascii_lowercase();
        if len + w.len() + usize::from(!words.is_empty()) > max && !words.is_empty() {
            break;
        }
        len += w.len() + usize::from(!words.is_empty());
        words.push(w);
    }
    words.join("-")
}

/// The ID at the start of a decision file name, if it looks like one.
pub fn id_from_file_name(name: &str) -> Option<Id> {
    let stem = name.strip_suffix(".md")?;
    let mut parts = stem.splitn(3, '-');
    let prefix = parts.next()?;
    let body = parts.next()?;
    Id::parse_loose(&format!("{prefix}-{body}")).ok()
}

fn needs_quotes(s: &str) -> bool {
    s.is_empty()
        || s.trim() != s
        || s.contains(": ")
        || s.contains(" #")
        || s.ends_with(':')
        || s.starts_with(|c: char| "!&*-?[]{}|>'\"%@`#,".contains(c))
        || s.contains(['\n', '\t', '"', '\\'])
        || matches!(
            s.to_ascii_lowercase().as_str(),
            "null" | "~" | "true" | "false" | "yes" | "no" | "on" | "off"
        )
        || s.parse::<f64>().is_ok()
}

/// A YAML double-quoted string (JSON escaping is valid YAML).
pub fn yaml_string(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| format!("\"{}\"", s.replace('"', "'")))
}

/// A plain scalar when safe, otherwise double-quoted.
pub fn yaml_scalar(s: &str) -> String {
    if needs_quotes(s) {
        yaml_string(s)
    } else {
        s.to_string()
    }
}

fn yaml_opt_string(s: Option<&str>) -> String {
    s.map(yaml_string).unwrap_or_else(|| "null".into())
}

fn yaml_opt_scalar(s: Option<&str>) -> String {
    s.map(yaml_scalar).unwrap_or_else(|| "null".into())
}

fn yaml_list<T: fmt::Display>(items: &[T]) -> String {
    let parts: Vec<String> = items.iter().map(|i| yaml_scalar(&i.to_string())).collect();
    format!("[{}]", parts.join(", "))
}

/// Set one top-level frontmatter field in place, leaving every other line
/// of the file exactly as it was. Adds the field before the closing `---`
/// if it isn't there. `value` is already YAML.
pub fn set_field(text: &str, key: &str, value: &str) -> Result<String, ParseError> {
    let (fm, body) = split_frontmatter(text)?;
    let prefix = format!("{key}:");
    let mut lines: Vec<String> = Vec::new();
    let mut found = false;
    let mut skipping_block = false;
    for line in fm.lines() {
        if skipping_block {
            // Continuation lines of a block value we replaced.
            if line.starts_with(' ') || line.starts_with('\t') || line.starts_with("- ") {
                continue;
            }
            skipping_block = false;
        }
        if !found && line.starts_with(&prefix) {
            lines.push(format!("{key}: {value}"));
            found = true;
            skipping_block = true;
        } else {
            lines.push(line.to_string());
        }
    }
    if !found {
        lines.push(format!("{key}: {value}"));
    }
    let mut out = String::from("---\n");
    for l in lines {
        out.push_str(&l);
        out.push('\n');
    }
    out.push_str("---\n");
    out.push_str(body);
    Ok(out)
}

/// YAML for a list of IDs, for use with `set_field`.
pub fn yaml_id_list(ids: &[Id]) -> String {
    yaml_list(ids)
}

/// YAML for an optional string, for use with `set_field`.
pub fn yaml_optional_string(s: Option<&str>) -> String {
    yaml_opt_string(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"---
id: D-7KQ2
title: "Snap writes go through one helper"
status: accepted
date: 2026-11-14
decided_by: Liam
basis: "Keep every Snap write in one helper so the confirm check lives in one place."
session: 9f2c0a1e
supersedes: []
superseded_by: null
aliases: [D-264]
tags: [snap, writes]
---

## Context

Why this came up.

## Decision

What was decided.

## Consequences

What follows.
"#;

    #[test]
    fn parses_the_documented_format() {
        let d = Decision::parse(SAMPLE).unwrap();
        assert_eq!(d.id.as_str(), "D-7KQ2");
        assert_eq!(d.status, Status::Accepted);
        assert_eq!(d.date, "2026-11-14");
        assert_eq!(d.session.as_deref(), Some("9f2c0a1e"));
        assert_eq!(d.aliases, vec!["D-264"]);
        assert_eq!(d.tags, vec!["snap", "writes"]);
        assert_eq!(d.section("decision").as_deref(), Some("What was decided."));
        assert!(d.answers_to("d-7kq2"));
        assert!(d.answers_to("D-264"));
        assert!(d.unknown_keys.is_empty());
    }

    #[test]
    fn render_round_trips() {
        let d = Decision::parse(SAMPLE).unwrap();
        let again = Decision::parse(&d.render()).unwrap();
        assert_eq!(d, again);
        assert_eq!(d.render(), SAMPLE);
    }

    #[test]
    fn every_decision_in_this_repo_parses() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/decisions");
        let mut n = 0;
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let text = std::fs::read_to_string(&path).unwrap();
            let d = Decision::parse(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            assert_eq!(id_from_file_name(&name), Some(d.id.clone()), "{name}");
            n += 1;
        }
        assert!(n > 20);
    }

    #[test]
    fn set_field_changes_one_line() {
        let out = set_field(SAMPLE, "status", "superseded").unwrap();
        assert!(out.contains("status: superseded\n"));
        assert_eq!(out.lines().count(), SAMPLE.lines().count());
        let out = set_field(&out, "superseded_by", "D-AAAA").unwrap();
        let d = Decision::parse(&out).unwrap();
        assert_eq!(
            d.superseded_by.map(|i| i.to_string()).as_deref(),
            Some("D-AAAA")
        );
        let added = set_field(SAMPLE, "extra", "1").unwrap();
        assert!(added.contains("extra: 1\n---\n"));
    }

    #[test]
    fn slugs_and_file_names() {
        let id = Id::parse_loose("D-7KQ2").unwrap();
        assert_eq!(
            file_name(&id, "Snap writes go through one helper"),
            "D-7KQ2-snap-writes-go-through-one-helper.md"
        );
        assert_eq!(slugify("Use `age`; not GPG!", 60), "use-age-not-gpg");
        assert_eq!(
            id_from_file_name("D-7KQ2-x-y.md").unwrap().as_str(),
            "D-7KQ2"
        );
        assert_eq!(id_from_file_name("D-7KQ2.md").unwrap().as_str(), "D-7KQ2");
        assert!(id_from_file_name("README.md").is_none());
    }

    #[test]
    fn quoting_is_safe() {
        assert_eq!(yaml_scalar("Liam"), "Liam");
        assert_eq!(yaml_scalar("agent"), "agent");
        assert_eq!(yaml_scalar("yes"), "\"yes\"");
        assert_eq!(yaml_scalar("a: b"), "\"a: b\"");
        assert_eq!(yaml_string("say \"hi\""), r#""say \"hi\"""#);
    }

    #[test]
    fn bad_files_give_clear_errors() {
        assert_eq!(
            Decision::parse("no frontmatter"),
            Err(ParseError::NoFrontmatter)
        );
        assert_eq!(
            Decision::parse("---\nid: D-1\n"),
            Err(ParseError::UnclosedFrontmatter)
        );
        assert!(matches!(
            Decision::parse("---\nid: D-7KQ2\ntitle: x\nstatus: maybe\n---\n"),
            Err(ParseError::Field {
                field: "status",
                ..
            })
        ));
    }
}
