//! Writing decisions: create, accept, reject, supersede. Every write goes
//! through the redactor (D-G1R6, D-VH2N): the file writer only accepts
//! `Redacted<DecisionText>`, which only the redactor can make.

use std::io::Write;
use std::path::{Path, PathBuf};

use inloop_core::{Id, IdSpec};
use inloop_redact::{RedactError, Redactable, Redacted, Redactor};
use serde::Serialize;

use crate::model::{
    Decision, ParseError, Status, set_field, yaml_id_list, yaml_optional_string, yaml_scalar,
};
use crate::sources::{DecisionSet, Entry, Place};

#[derive(Debug, thiserror::Error)]
pub enum OpError {
    #[error(
        "an accepted decision needs a basis: quote the human who made it. If you chose this yourself, record it as proposed (D-BY48)"
    )]
    MissingBasis,
    #[error("a decision needs a title")]
    MissingTitle,
    #[error("new decisions are proposed or accepted, not {0}")]
    BadNewStatus(Status),
    #[error("no decision {0} found on main, in any worktree, or on any unmerged branch")]
    NotFound(String),
    #[error(
        "{id} is {where_}, not in this worktree. Change it where it lives, or after it is merged"
    )]
    NotHere { id: Id, where_: String },
    #[error("{id} is {status}; only proposed decisions can be {action}")]
    WrongStatus {
        id: Id,
        status: Status,
        action: &'static str,
    },
    #[error("{0} can't supersede itself")]
    SelfSupersede(Id),
    #[error("{0} already exists")]
    Exists(PathBuf),
    #[error(transparent)]
    Id(#[from] inloop_core::IdError),
    #[error("redaction failed, so nothing was written: {0}")]
    Redact(#[from] RedactError),
    #[error(
        "after redaction the file no longer parses ({0}); a secret is probably in a frontmatter field. Remove it and try again"
    )]
    RedactionBrokeFile(ParseError),
    #[error("{0}")]
    Parse(#[from] ParseError),
    #[error("could not write {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
}

/// The full text of a decision file, ready to be redacted and written.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DecisionText(pub String);

impl Redactable for DecisionText {
    /// Redact the body as text, and each frontmatter value on its own,
    /// re-quoting any value that changed so a `[REDACTED:...]` marker can
    /// never turn a plain scalar into a YAML list.
    fn redact_with(self, r: &Redactor) -> Result<Self, RedactError> {
        let Ok((fm, body)) = crate::model::split_frontmatter(&self.0) else {
            return Ok(DecisionText(r.redact_text(&self.0)?));
        };
        let mut out = String::from("---\n");
        for line in fm.lines() {
            out.push_str(&redact_frontmatter_line(r, line)?);
            out.push('\n');
        }
        out.push_str("---\n");
        out.push_str(&r.redact_text(body)?);
        Ok(DecisionText(out))
    }
}

fn redact_frontmatter_line(r: &Redactor, line: &str) -> Result<String, RedactError> {
    let Some((key, value)) = line.split_once(':') else {
        return r.redact_text(line);
    };
    if key.starts_with([' ', '\t', '-']) || key.contains(' ') {
        // Continuation of a block value: redact as text.
        let red = r.redact_text(line)?;
        return Ok(if red == line {
            red
        } else {
            format!("  {}", yaml_string_raw(&red))
        });
    }
    let raw = value.trim();
    // Unquote a double-quoted value so encodings inside it are seen.
    let plain: String = if raw.starts_with('"') {
        serde_json::from_str::<String>(raw).unwrap_or_else(|_| raw.to_string())
    } else {
        raw.to_string()
    };
    let red = r.redact_text(&plain)?;
    if red == plain {
        return Ok(line.to_string());
    }
    if raw.starts_with('[') && raw.ends_with(']') {
        // A flow list: redact each item and keep it a list.
        let items: Result<Vec<String>, RedactError> = raw[1..raw.len() - 1]
            .split(',')
            .map(|i| i.trim())
            .filter(|i| !i.is_empty())
            .map(|i| {
                r.redact_text(i.trim_matches('"'))
                    .map(|x| yaml_string_raw(&x))
            })
            .collect();
        return Ok(format!("{key}: [{}]", items?.join(", ")));
    }
    Ok(format!("{key}: {}", yaml_string_raw(&red)))
}

fn yaml_string_raw(s: &str) -> String {
    crate::model::yaml_string(s)
}

/// What a caller supplies to record a decision.
#[derive(Clone, Debug, Default)]
pub struct NewDecision {
    pub title: String,
    pub status: Option<Status>,
    pub decided_by: Option<String>,
    pub basis: Option<String>,
    pub session: Option<String>,
    pub tags: Vec<String>,
    pub context: String,
    pub decision: String,
    pub consequences: String,
    pub supersedes: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Written {
    pub id: Id,
    pub path: PathBuf,
    /// Other files changed in the same operation (superseded decisions).
    pub also_changed: Vec<PathBuf>,
}

/// Write a file atomically: a temp file in the same directory, then rename.
/// With `create_new`, refuse to replace an existing file.
fn write_file(path: &Path, text: Redacted<DecisionText>, create_new: bool) -> Result<(), OpError> {
    let io = |source| OpError::Io {
        path: path.to_path_buf(),
        source,
    };
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir).map_err(io)?;
    if create_new && path.exists() {
        return Err(OpError::Exists(path.to_path_buf()));
    }
    // Check the file still parses after redaction.
    Decision::parse(&text.get().0).map_err(OpError::RedactionBrokeFile)?;
    let tmp = dir.join(format!(
        ".{}.inloop-tmp",
        path.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    ));
    {
        let mut f = std::fs::File::create(&tmp).map_err(io)?;
        f.write_all(text.get().0.as_bytes()).map_err(io)?;
        f.sync_all().map_err(io)?;
    }
    std::fs::rename(&tmp, path).map_err(io)?;
    Ok(())
}

fn body(context: &str, decision: &str, consequences: &str) -> String {
    let sec = |h: &str, t: &str| {
        let t = t.trim();
        format!(
            "## {h}\n\n{}\n",
            if t.is_empty() { "(not given)" } else { t }
        )
    };
    format!(
        "\n{}\n{}\n{}",
        sec("Context", context),
        sec("Decision", decision),
        sec("Consequences", consequences)
    )
}

/// The worktree file of `entry`, if it is in the current worktree.
fn here(entry: &Entry) -> Result<PathBuf, OpError> {
    match (&entry.place, &entry.path) {
        (Place::Worktree { current: true, .. }, Some(p)) => Ok(p.clone()),
        (place, _) => Err(OpError::NotHere {
            id: entry.decision.id.clone(),
            where_: match place {
                Place::Main => "only on main".to_string(),
                other => format!("on {}", other.describe()),
            },
        }),
    }
}

/// Record a new decision in `dir` (the decisions directory of the current
/// worktree).
pub fn create(
    set: &DecisionSet,
    dir: &Path,
    spec: &IdSpec,
    redactor: &Redactor,
    new: NewDecision,
    today: &str,
) -> Result<Written, OpError> {
    let title = new.title.trim().to_string();
    if title.is_empty() {
        return Err(OpError::MissingTitle);
    }
    let status = new.status.unwrap_or(if new.basis.is_some() {
        Status::Accepted
    } else {
        Status::Proposed
    });
    if !matches!(status, Status::Proposed | Status::Accepted) {
        return Err(OpError::BadNewStatus(status));
    }
    let basis = new.basis.filter(|b| !b.trim().is_empty());
    if status == Status::Accepted && basis.is_none() {
        return Err(OpError::MissingBasis);
    }
    // Everything superseded must be here, so both files change together.
    let mut olds: Vec<(Entry, PathBuf)> = Vec::new();
    for s in &new.supersedes {
        let e = set.get(s).ok_or_else(|| OpError::NotFound(s.clone()))?;
        let p = here(e)?;
        olds.push((e.clone(), p));
    }
    let id =
        spec.generate_unused(|id| set.id_taken(id) || dir.join(format!("{id}.md")).exists())?;
    let decided_by = match status {
        Status::Proposed => new.decided_by.unwrap_or_else(|| "agent".into()),
        _ => new.decided_by.unwrap_or_else(|| "unknown".into()),
    };
    let d = Decision {
        id: id.clone(),
        title,
        status,
        date: today.to_string(),
        decided_by,
        basis,
        session: new.session,
        supersedes: olds.iter().map(|(e, _)| e.decision.id.clone()).collect(),
        superseded_by: None,
        aliases: Vec::new(),
        tags: new.tags,
        body: body(&new.context, &new.decision, &new.consequences),
        promoted_by: None,
        unknown_keys: Vec::new(),
    };
    let path = dir.join(d.file_name());
    // Redact everything before writing anything.
    let new_text = redactor.redact(DecisionText(d.render()))?;
    let mut old_texts = Vec::new();
    for (e, p) in &olds {
        let text = std::fs::read_to_string(p).map_err(|source| OpError::Io {
            path: p.clone(),
            source,
        })?;
        let text = set_field(&text, "status", Status::Superseded.as_str())?;
        let text = set_field(&text, "superseded_by", id.as_str())?;
        old_texts.push((p.clone(), redactor.redact(DecisionText(text))?));
        let _ = e;
    }
    write_file(&path, new_text, true)?;
    let mut also_changed = Vec::new();
    for (p, t) in old_texts {
        write_file(&p, t, false)?;
        also_changed.push(p);
    }
    Ok(Written {
        id,
        path,
        also_changed,
    })
}

/// Accept or reject a proposed decision.
pub fn promote(
    set: &DecisionSet,
    redactor: &Redactor,
    id: &str,
    accept: bool,
    basis: Option<String>,
    decided_by: Option<String>,
    promoted_by: &str,
) -> Result<Written, OpError> {
    let entry = set
        .get(id)
        .ok_or_else(|| OpError::NotFound(id.to_string()))?;
    let path = here(entry)?;
    let d = &entry.decision;
    if d.status != Status::Proposed {
        return Err(OpError::WrongStatus {
            id: d.id.clone(),
            status: d.status,
            action: if accept { "accepted" } else { "rejected" },
        });
    }
    let basis = basis.filter(|b| !b.trim().is_empty());
    if accept && basis.is_none() {
        return Err(OpError::MissingBasis);
    }
    let text = std::fs::read_to_string(&path).map_err(|source| OpError::Io {
        path: path.clone(),
        source,
    })?;
    let new_status = if accept {
        Status::Accepted
    } else {
        Status::Rejected
    };
    let mut text = set_field(&text, "status", new_status.as_str())?;
    if let Some(b) = &basis {
        text = set_field(&text, "basis", &yaml_optional_string(Some(b)))?;
    }
    if let Some(who) = decided_by.filter(|w| !w.trim().is_empty()) {
        text = set_field(&text, "decided_by", &yaml_scalar(&who))?;
    }
    text = set_field(&text, "promoted_by", &yaml_scalar(promoted_by))?;
    let redacted = redactor.redact(DecisionText(text))?;
    write_file(&path, redacted, false)?;
    Ok(Written {
        id: d.id.clone(),
        path,
        also_changed: Vec::new(),
    })
}

/// Record that `new` supersedes `old`. Both must be in this worktree.
pub fn supersede(
    set: &DecisionSet,
    redactor: &Redactor,
    old: &str,
    new: &str,
) -> Result<Written, OpError> {
    let o = set
        .get(old)
        .ok_or_else(|| OpError::NotFound(old.to_string()))?;
    let n = set
        .get(new)
        .ok_or_else(|| OpError::NotFound(new.to_string()))?;
    if o.decision.id == n.decision.id {
        return Err(OpError::SelfSupersede(o.decision.id.clone()));
    }
    let op = here(o)?;
    let np = here(n)?;
    let otext = std::fs::read_to_string(&op).map_err(|source| OpError::Io {
        path: op.clone(),
        source,
    })?;
    let otext = set_field(&otext, "status", Status::Superseded.as_str())?;
    let otext = set_field(&otext, "superseded_by", n.decision.id.as_str())?;
    let mut supersedes = n.decision.supersedes.clone();
    if !supersedes.contains(&o.decision.id) {
        supersedes.push(o.decision.id.clone());
    }
    let ntext = std::fs::read_to_string(&np).map_err(|source| OpError::Io {
        path: np.clone(),
        source,
    })?;
    let ntext = set_field(&ntext, "supersedes", &yaml_id_list(&supersedes))?;
    let otext = redactor.redact(DecisionText(otext))?;
    let ntext = redactor.redact(DecisionText(ntext))?;
    write_file(&op, otext, false)?;
    write_file(&np, ntext, false)?;
    Ok(Written {
        id: n.decision.id.clone(),
        path: np,
        also_changed: vec![op],
    })
}
