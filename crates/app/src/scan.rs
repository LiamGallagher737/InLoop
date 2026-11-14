//! `inloop scan`: re-run the current redactor over everything stored, to
//! catch what an older version missed (docs/04).

use std::collections::BTreeMap;

use inloop_decisions::ops::DecisionText;
use inloop_sessions::{SessionMeta, Transcript};
use serde::Serialize;

use crate::project::{AppError, Project};

#[derive(Clone, Debug, Default, Serialize)]
pub struct ScanFinding {
    pub what: String,
    pub kinds: BTreeMap<String, usize>,
    /// True if the stored copy was rewritten, redacted.
    pub fixed: bool,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct ScanReport {
    pub decisions_checked: usize,
    pub sessions_checked: usize,
    pub board_records_checked: usize,
    pub findings: Vec<ScanFinding>,
}

fn kinds(findings: &[inloop_redact::Finding]) -> BTreeMap<String, usize> {
    let mut m = BTreeMap::new();
    for f in findings {
        *m.entry(f.kind.clone()).or_insert(0) += 1;
    }
    m
}

impl Project {
    /// Scan decisions in this worktree, every stored session, and (with the
    /// PM layer) the board. With `fix`, rewrite anything found, redacted.
    /// Sessions are always fixed: they are InLoop's own copies.
    pub fn scan(&self, fix: bool) -> Result<ScanReport, AppError> {
        let r = self.redactor()?;
        let mut report = ScanReport::default();

        let set = self.decision_set(false)?;
        for e in &set.entries {
            let Some(path) = &e.path else { continue };
            let Ok(text) = std::fs::read_to_string(path) else {
                continue;
            };
            report.decisions_checked += 1;
            let found = r.find(&text);
            if found.is_empty() {
                continue;
            }
            let mut fixed = false;
            if fix {
                let red = r
                    .redact(DecisionText(text))
                    .map_err(|e| AppError::Redactor(e.to_string()))?;
                std::fs::write(path, &red.get().0)
                    .map_err(|e| AppError::io("rewriting a decision", e))?;
                fixed = true;
            }
            report.findings.push(ScanFinding {
                what: format!("decision {} ({})", e.decision.id, path.display()),
                kinds: kinds(&found),
                fixed,
            });
        }

        let store = self.store()?;
        for id in store
            .list_records("sessions")
            .map_err(|e| AppError::Other(e.to_string()))?
        {
            let Some(bytes) = store
                .get_record("sessions", &id)
                .map_err(|e| AppError::Other(e.to_string()))?
            else {
                continue;
            };
            let Ok(mut meta) = serde_json::from_slice::<SessionMeta>(&bytes) else {
                continue;
            };
            report.sessions_checked += 1;
            let mut found_kinds = kinds(&r.find(&String::from_utf8_lossy(&bytes)));
            if let Some(blob) = store
                .get_blob(&meta.blob)
                .map_err(|e| AppError::Other(e.to_string()))?
            {
                let text = String::from_utf8_lossy(&blob).into_owned();
                let found = r.find(&text);
                if !found.is_empty() {
                    for (k, n) in kinds(&found) {
                        *found_kinds.entry(k).or_insert(0) += n;
                    }
                    let t = r
                        .redact(Transcript::parse_jsonl(&text))
                        .map_err(|e| AppError::Redactor(e.to_string()))?;
                    let new_blob = store
                        .put_blob(t.get().to_jsonl().as_bytes())
                        .map_err(|e| AppError::Other(e.to_string()))?;
                    let _ = store.delete_blob(&meta.blob);
                    meta.blob = new_blob;
                }
            }
            if !found_kinds.is_empty() {
                let meta = r
                    .redact(meta)
                    .map_err(|e| AppError::Redactor(e.to_string()))?;
                let json =
                    serde_json::to_vec(meta.get()).map_err(|e| AppError::Other(e.to_string()))?;
                store
                    .put_record("sessions", &id, &json)
                    .map_err(|e| AppError::Other(e.to_string()))?;
                report.findings.push(ScanFinding {
                    what: format!("session {id}"),
                    kinds: found_kinds,
                    fixed: true,
                });
            }
        }
        report.board_records_checked += self.scan_board(fix, &mut report.findings)?;
        Ok(report)
    }

    /// Scan PM board records. Filled in by the PM layer.
    fn scan_board(&self, _fix: bool, _findings: &mut Vec<ScanFinding>) -> Result<usize, AppError> {
        Ok(0)
    }
}
