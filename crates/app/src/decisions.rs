//! Decision operations for the CLI and MCP.

use inloop_decisions::lint::{Issue, LintOptions, Severity, lint_decisions, lint_references};
use inloop_decisions::ops::{self, NewDecision, Written};
use inloop_decisions::{DecisionSet, Entry, ScanOptions, Status, scan};
use inloop_index::{Doc, Hit, Kind};
use serde::Serialize;

use crate::project::{AppError, Project};

/// A decision as people and agents see it: the decision, where its file
/// is, and where it stands.
pub type DecisionView = Entry;

/// Filters for `dec list` and `decision_list`.
#[derive(Clone, Debug, Default)]
pub struct ListFilter {
    pub status: Option<Status>,
    pub tag: Option<String>,
    /// Only decisions that aren't merged yet.
    pub unmerged_only: bool,
    /// Only decisions dated on or after this `YYYY-MM-DD`.
    pub since: Option<String>,
    /// Leave out other worktrees and branches.
    pub merged_only: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct LintReport {
    pub errors: usize,
    pub warnings: usize,
    pub issues: Vec<Issue>,
}

/// A search result: a decision or session, with provenance.
#[derive(Clone, Debug, Serialize)]
pub struct SearchResult {
    pub kind: Kind,
    pub id: String,
    pub title: String,
    pub snippet: String,
    #[serde(flatten)]
    pub meta: serde_json::Map<String, serde_json::Value>,
}

impl From<Hit> for SearchResult {
    fn from(h: Hit) -> Self {
        let meta = match h.meta {
            serde_json::Value::Object(m) => m,
            _ => serde_json::Map::new(),
        };
        SearchResult {
            kind: h.kind,
            id: h.id,
            title: h.title,
            snippet: h.snippet,
            meta,
        }
    }
}

impl Project {
    /// Every decision InLoop can see, with provenance; also refreshes the
    /// index.
    /// Forget the decisions fingerprint so the next refresh re-indexes
    /// (and re-redacts) everything.
    pub fn invalidate_decision_index(&self) -> Result<(), AppError> {
        self.index()?.set_state("decisions-fingerprint", "")?;
        Ok(())
    }

    pub fn decision_set(&self, include_unmerged: bool) -> Result<DecisionSet, AppError> {
        let mut set = scan(
            &self.repo,
            &ScanOptions {
                decisions_dir: &self.config.decisions.dir,
                main_branch: &self.main_branch,
                include_unmerged,
            },
        )?;
        for e in &mut set.entries {
            if e.provenance.task.is_none() && !e.provenance.is_merged() {
                e.provenance.task = self.task_for_branch(e.provenance.branch.as_deref());
            }
        }
        if include_unmerged {
            self.index_decisions(&set)?;
        }
        Ok(set)
    }

    fn index_decisions(&self, set: &DecisionSet) -> Result<(), AppError> {
        let docs: Vec<Doc> = set
            .entries
            .iter()
            .map(|e| {
                let d = &e.decision;
                let mut tags = d.tags.clone();
                tags.extend(d.aliases.iter().cloned());
                tags.push(d.status.to_string());
                Doc {
                    kind: Kind::Decision,
                    id: d.id.to_string(),
                    title: d.title.clone(),
                    body: d.body.clone(),
                    tags: tags.join(" "),
                    meta: serde_json::json!({
                        "status": d.status,
                        "date": d.date,
                        "provenance": e.provenance,
                    }),
                }
            })
            .collect();
        // Skip the work when nothing changed since the last refresh.
        let fingerprint = {
            use std::hash::{Hash, Hasher};
            let mut h = std::hash::DefaultHasher::new();
            for d in &docs {
                serde_json::to_string(d).unwrap_or_default().hash(&mut h);
            }
            format!("{:x}", h.finish())
        };
        let mut index = self.index()?;
        if index.get_state("decisions-fingerprint")?.as_deref() == Some(fingerprint.as_str()) {
            return Ok(());
        }
        let r = self.redactor()?;
        let mut redacted = Vec::with_capacity(docs.len());
        for d in docs {
            redacted.push(r.redact(d).map_err(|e| AppError::Redactor(e.to_string()))?);
        }
        index.replace_all(Kind::Decision, &redacted)?;
        index.set_state("decisions-fingerprint", &fingerprint)?;
        Ok(())
    }

    pub fn decision_list(&self, f: &ListFilter) -> Result<Vec<DecisionView>, AppError> {
        let set = self.decision_set(!f.merged_only)?;
        let mut out: Vec<DecisionView> = set
            .entries
            .into_iter()
            .filter(|e| f.status.is_none_or(|s| e.decision.status == s))
            .filter(|e| {
                f.tag
                    .as_ref()
                    .is_none_or(|t| e.decision.tags.iter().any(|x| x.eq_ignore_ascii_case(t)))
            })
            .filter(|e| !f.unmerged_only || !e.provenance.is_merged())
            .filter(|e| !f.merged_only || e.provenance.is_merged())
            .filter(|e| {
                f.since
                    .as_ref()
                    .is_none_or(|s| e.decision.date.as_str() >= s.as_str())
            })
            .collect();
        out.sort_by(|a, b| {
            b.decision
                .date
                .cmp(&a.decision.date)
                .then_with(|| a.decision.id.cmp(&b.decision.id))
        });
        Ok(out)
    }

    /// A decision reference as the user typed it, with the ID prefix added
    /// when they left it off (`7KQ2` for `D-7KQ2`).
    pub fn decision_ref(&self, id: &str) -> String {
        self.config.decisions.id_spec().qualify(id)
    }

    pub fn decision_get(&self, id: &str) -> Result<DecisionView, AppError> {
        let set = self.decision_set(true)?;
        let id = self.decision_ref(id);
        set.get(&id)
            .cloned()
            .ok_or_else(|| AppError::NotFound(format!("decision {id}")))
    }

    /// Full-text search over decisions (and, with `kinds`, sessions).
    pub fn search(
        &self,
        query: &str,
        kinds: &[Kind],
        limit: usize,
    ) -> Result<Vec<SearchResult>, AppError> {
        if kinds.is_empty() || kinds.contains(&Kind::Decision) {
            self.decision_set(true)?;
        }
        let hits = self.index()?.search(query, kinds, limit)?;
        let mut out: Vec<SearchResult> = hits.into_iter().map(SearchResult::from).collect();
        // Session provenance is worked out now, from the commits.
        for h in out.iter_mut().filter(|h| h.kind == Kind::Session) {
            let get = |k: &str| h.meta.get(k).and_then(|v| v.as_str()).map(str::to_string);
            let commits: Vec<String> = h
                .meta
                .get("commits")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|c| c.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            let p = self.session_provenance_parts(
                get("branch").as_deref(),
                get("worktree").as_deref(),
                &commits,
                get("task").as_deref(),
            );
            h.meta.remove("commits");
            if let Ok(v) = serde_json::to_value(p) {
                h.meta.insert("provenance".into(), v);
            }
        }
        Ok(out)
    }

    pub fn decision_new(&self, new: NewDecision) -> Result<Written, AppError> {
        let set = self.decision_set(true)?;
        let w = ops::create(
            &set,
            &self.decisions_dir(),
            &self.config.decisions.id_spec(),
            self.redactor()?,
            new,
            &inloop_core::today(),
        )?;
        self.decision_set(true)?;
        Ok(w)
    }

    pub fn decision_promote(
        &self,
        id: &str,
        accept: bool,
        basis: Option<String>,
        decided_by: Option<String>,
        promoted_by: &str,
    ) -> Result<Written, AppError> {
        let set = self.decision_set(true)?;
        let id = self.decision_ref(id);
        let w = ops::promote(
            &set,
            self.redactor()?,
            &id,
            accept,
            basis,
            decided_by,
            promoted_by,
        )?;
        self.decision_set(true)?;
        Ok(w)
    }

    pub fn decision_supersede(&self, old: &str, new: &str) -> Result<Written, AppError> {
        let set = self.decision_set(true)?;
        let (old, new) = (self.decision_ref(old), self.decision_ref(new));
        let w = ops::supersede(&set, self.redactor()?, &old, &new)?;
        self.decision_set(true)?;
        Ok(w)
    }

    /// `inloop lint`.
    pub fn lint(&self) -> Result<LintReport, AppError> {
        let set = self.decision_set(true)?;
        let spec = self.config.decisions.id_spec();
        let opts = LintOptions {
            spec: &spec,
            decisions_dir: &self.config.decisions.dir,
            reference_globs: &self.config.decisions.lint_references,
            example_ids: &self.config.decisions.example_ids,
        };
        let mut issues = lint_decisions(&set, &opts);
        let files = self.worktree_text_files()?;
        issues.extend(lint_references(&set, &files, &opts));
        issues.sort_by(|a, b| {
            a.severity
                .cmp(&b.severity)
                .then_with(|| a.file.cmp(&b.file))
                .then_with(|| a.line.cmp(&b.line))
        });
        Ok(LintReport {
            errors: issues
                .iter()
                .filter(|i| i.severity == Severity::Error)
                .count(),
            warnings: issues
                .iter()
                .filter(|i| i.severity == Severity::Warning)
                .count(),
            issues,
        })
    }

    /// Tracked and untracked-but-not-ignored text files in this worktree,
    /// as (path relative to the root, content). Skips large and binary files.
    pub fn worktree_text_files(&self) -> Result<Vec<(String, String)>, AppError> {
        let mut names = Vec::new();
        for args in [
            &["ls-files", "-z"][..],
            &["ls-files", "-z", "-o", "--exclude-standard"][..],
        ] {
            let text = self.repo.run(args)?;
            names.extend(
                text.split('\0')
                    .filter(|n| !n.is_empty())
                    .map(str::to_string),
            );
        }
        names.sort();
        names.dedup();
        let mut out = Vec::new();
        for n in names {
            let p = self.repo.root.join(&n);
            match std::fs::metadata(&p) {
                Ok(m) if m.is_file() && m.len() <= 1_000_000 => {}
                _ => continue,
            }
            if let Ok(text) = std::fs::read_to_string(&p) {
                out.push((n, text));
            }
        }
        Ok(out)
    }
}
