//! `inloop lint`: checks over every decision InLoop can see (docs/02).

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use globset::{Glob, GlobSetBuilder};
use inloop_core::{Id, IdSpec};
use serde::Serialize;

use crate::model::{Status, id_from_file_name};
use crate::sources::{DecisionSet, Place};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Issue {
    pub severity: Severity,
    /// A short stable code, for example `missing-basis`.
    pub code: &'static str,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
}

impl Issue {
    fn error(code: &'static str, message: String, file: Option<String>) -> Self {
        Issue {
            severity: Severity::Error,
            code,
            message,
            file,
            line: None,
        }
    }
    fn warning(code: &'static str, message: String, file: Option<String>) -> Self {
        Issue {
            severity: Severity::Warning,
            code,
            message,
            file,
            line: None,
        }
    }
}

pub struct LintOptions<'a> {
    pub spec: &'a IdSpec,
    pub decisions_dir: &'a str,
    /// Globs (relative to the repo root) of files whose references to check.
    pub reference_globs: &'a [String],
    /// IDs that appear as examples and needn't exist.
    pub example_ids: &'a [String],
}

/// Check the decisions themselves.
pub fn lint_decisions(set: &DecisionSet, opts: &LintOptions<'_>) -> Vec<Issue> {
    let mut issues = Vec::new();
    let file_of = |name: &str| Some(format!("{}/{}", opts.decisions_dir, name));

    for b in &set.broken {
        issues.push(Issue::error(
            "parse",
            format!("{} ({}) does not parse: {}", b.file_name, b.place, b.error),
            file_of(&b.file_name),
        ));
    }

    // Per copy: the ID matches the file name. Report each file name once.
    let mut reported: HashSet<String> = HashSet::new();
    for f in &set.found {
        let Ok(d) = &f.parsed else { continue };
        if id_from_file_name(&f.file_name).as_ref() != Some(&d.id)
            && reported.insert(f.file_name.clone())
        {
            issues.push(Issue::error(
                "id-mismatch",
                format!(
                    "{} has id {} but its file name says otherwise ({})",
                    f.file_name,
                    d.id,
                    f.place.describe()
                ),
                file_of(&f.file_name),
            ));
        }
        if let Err(e) = opts.spec.parse(d.id.as_str())
            && reported.insert(format!("shape:{}", f.file_name))
        {
            issues.push(Issue::warning(
                "id-shape",
                format!("{}: {e}", f.file_name),
                file_of(&f.file_name),
            ));
        }
    }

    for c in &set.collisions {
        let copies: Vec<String> = c.copies.iter().map(|(n, p)| format!("{n} ({p})")).collect();
        issues.push(Issue::error(
            "duplicate-id",
            format!(
                "{} is used by more than one decision: {}. Give one of them a new ID.",
                c.id,
                copies.join(", ")
            ),
            None,
        ));
    }

    let ids: HashSet<&Id> = set.entries.iter().map(|e| &e.decision.id).collect();
    for e in &set.entries {
        let d = &e.decision;
        let file = file_of(&e.file_name);
        let place = match &e.place {
            Place::Main => String::new(),
            p => format!(" ({})", p.describe()),
        };
        if d.status == Status::Accepted && d.basis.is_none() {
            issues.push(Issue::error(
                "missing-basis",
                format!(
                    "{}{place} is accepted but has no basis: quote the human who decided it, or set status: proposed",
                    d.id
                ),
                file.clone(),
            ));
        }
        if d.status == Status::Superseded && d.superseded_by.is_none() {
            issues.push(Issue::error(
                "superseded-without-link",
                format!("{}{place} is superseded but superseded_by is empty", d.id),
                file.clone(),
            ));
        }
        if let Some(by) = &d.superseded_by {
            if d.status != Status::Superseded {
                issues.push(Issue::warning(
                    "status-not-superseded",
                    format!(
                        "{}{place} has superseded_by {by} but status {}",
                        d.id, d.status
                    ),
                    file.clone(),
                ));
            }
            match set.get(by.as_str()) {
                None => issues.push(Issue::error(
                    "dangling-link",
                    format!("{}{place} is superseded by {by}, which doesn't exist", d.id),
                    file.clone(),
                )),
                Some(newer) if !newer.decision.supersedes.contains(&d.id) => {
                    issues.push(Issue::warning(
                        "one-way-link",
                        format!("{}{place} says {by} supersedes it, but {by} doesn't list it in supersedes", d.id),
                        file.clone(),
                    ))
                }
                _ => {}
            }
        }
        for old in &d.supersedes {
            match set.get(old.as_str()) {
                None => issues.push(Issue::error(
                    "dangling-link",
                    format!("{}{place} supersedes {old}, which doesn't exist", d.id),
                    file.clone(),
                )),
                Some(o) if o.decision.superseded_by.as_ref() != Some(&d.id) => {
                    issues.push(Issue::warning(
                        "one-way-link",
                        format!("{}{place} supersedes {old}, but {old} doesn't name it in superseded_by", d.id),
                        file.clone(),
                    ))
                }
                _ => {}
            }
        }
        for alias in &d.aliases {
            if let Ok(a) = Id::parse_loose(alias)
                && ids.contains(&a)
            {
                issues.push(Issue::error(
                    "alias-is-an-id",
                    format!(
                        "{}{place} has alias {alias}, which is also a decision's ID",
                        d.id
                    ),
                    file.clone(),
                ));
            }
        }
        if !d.unknown_keys.is_empty() {
            issues.push(Issue::warning(
                "unknown-field",
                format!(
                    "{}{place} has unknown frontmatter: {}",
                    d.id,
                    d.unknown_keys.join(", ")
                ),
                file.clone(),
            ));
        }
        if d.title.is_empty() {
            issues.push(Issue::error(
                "empty-title",
                format!("{}{place} has no title", d.id),
                file.clone(),
            ));
        }
    }
    issues
}

/// Check `D-XXXX` references in the given files (path relative to the
/// repo root, and content). Unknown IDs are errors; references to
/// superseded decisions are warnings, except inside decision files.
pub fn lint_references(
    set: &DecisionSet,
    files: &[(String, String)],
    opts: &LintOptions<'_>,
) -> Vec<Issue> {
    let mut builder = GlobSetBuilder::new();
    for g in opts.reference_globs {
        if let Ok(glob) = Glob::new(g) {
            builder.add(glob);
        }
    }
    let Ok(globs) = builder.build() else {
        return Vec::new();
    };
    let examples: HashSet<String> = opts
        .example_ids
        .iter()
        .filter_map(|e| Id::parse_loose(e).ok().map(|i| i.to_string()))
        .collect();
    let dec_dir = format!("{}/", opts.decisions_dir.trim_end_matches('/'));
    let mut unknown: BTreeMap<(String, usize), Vec<String>> = BTreeMap::new();
    let mut issues = Vec::new();
    for (path, text) in files {
        if !globs.is_match(Path::new(path)) {
            continue;
        }
        let in_decisions = path.starts_with(&dec_dir);
        for (line_no, line) in text.lines().enumerate() {
            for (_, r) in inloop_core::id::find_references(opts.spec, line) {
                if examples.contains(r) {
                    continue;
                }
                match set.get(r) {
                    None => unknown
                        .entry((path.clone(), line_no + 1))
                        .or_default()
                        .push(r.to_string()),
                    Some(e) if e.decision.status == Status::Superseded && !in_decisions => {
                        let by = e
                            .decision
                            .superseded_by
                            .as_ref()
                            .map(|b| format!(" by {b}"))
                            .unwrap_or_default();
                        issues.push(Issue {
                            severity: Severity::Warning,
                            code: "cites-superseded",
                            message: format!(
                                "cites {r}, which was superseded{by}; update the text"
                            ),
                            file: Some(path.clone()),
                            line: Some(line_no + 1),
                        });
                    }
                    _ => {}
                }
            }
        }
    }
    for ((path, line), ids) in unknown {
        issues.push(Issue {
            severity: Severity::Error,
            code: "unknown-reference",
            message: format!(
                "cites {}, which no decision uses (add it to [decisions] example_ids if it's an example)",
                ids.join(", ")
            ),
            file: Some(path),
            line: Some(line),
        });
    }
    issues
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Decision;
    use crate::sources::{Entry, Found};
    use inloop_core::Provenance;

    fn entry(text: &str) -> (Entry, Found) {
        let d = Decision::parse(text).unwrap();
        let name = d.file_name();
        (
            Entry {
                decision: d.clone(),
                file_name: name.clone(),
                path: None,
                provenance: Provenance::merged(),
                place: Place::Main,
            },
            Found {
                place: Place::Main,
                file_name: name,
                text: text.into(),
                parsed: Ok(d),
            },
        )
    }

    fn set(texts: &[&str]) -> DecisionSet {
        let mut s = DecisionSet::default();
        for t in texts {
            let (e, f) = entry(t);
            s.entries.push(e);
            s.found.push(f);
        }
        s
    }

    fn fm(id: &str, status: &str, extra: &str) -> String {
        format!(
            "---\nid: {id}\ntitle: \"T {id}\"\nstatus: {status}\ndate: 2026-11-14\ndecided_by: agent\n{extra}---\n\nBody.\n"
        )
    }

    #[test]
    fn flags_the_documented_problems() {
        let spec = IdSpec::decisions();
        let opts = LintOptions {
            spec: &spec,
            decisions_dir: "docs/decisions",
            reference_globs: &["**/*.md".to_string()],
            example_ids: &["D-7KQ2".to_string()],
        };
        let a = fm("D-AAAA", "accepted", "basis: null\n");
        let b = fm("D-BBBB", "superseded", "superseded_by: D-CCCC\n");
        let c = fm(
            "D-CCCC",
            "accepted",
            "basis: \"yes\"\nsupersedes: [D-BBBB]\naliases: [D-AAAA]\n",
        );
        let s = set(&[&a, &b, &c]);
        let issues = lint_decisions(&s, &opts);
        let codes: Vec<&str> = issues.iter().map(|i| i.code).collect();
        assert!(codes.contains(&"missing-basis"), "{codes:?}");
        assert!(codes.contains(&"alias-is-an-id"), "{codes:?}");
        assert!(!codes.contains(&"one-way-link"), "{codes:?}");

        let files = vec![
            (
                "docs/x.md".to_string(),
                "See D-AAAA, D-BBBB, D-ZZZZ and D-7KQ2.\n".to_string(),
            ),
            (
                "src/main.rs".to_string(),
                "// D-YYYY is not checked here\n".to_string(),
            ),
            (
                "docs/decisions/D-CCCC-t.md".to_string(),
                "supersedes D-BBBB\n".to_string(),
            ),
        ];
        let refs = lint_references(&s, &files, &opts);
        assert_eq!(refs.len(), 2, "{refs:?}");
        assert!(
            refs.iter()
                .any(|i| i.code == "cites-superseded" && i.line == Some(1))
        );
        assert!(
            refs.iter()
                .any(|i| i.code == "unknown-reference" && i.message.contains("D-ZZZZ"))
        );
    }
}
