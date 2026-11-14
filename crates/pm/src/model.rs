//! Board records (D-V3ZM): one record per task, claim, report and blocker,
//! each with a stable ID, an owner where it has one, and timestamps, so the
//! records merge cleanly when sync arrives later.
//!
//! Every record implements [`Redactable`]: every piece of free text goes
//! through the redactor. IDs and timestamps are InLoop's own and are left
//! alone.

use std::fmt;
use std::str::FromStr;

use inloop_core::{Id, IdSpec};
use inloop_redact::{RedactError, Redactable, Redactor};
use serde::{Deserialize, Serialize};

/// The kinds of record on the board, each with its own ID prefix and folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RecordKind {
    Task,
    Claim,
    Report,
    Blocker,
}

impl RecordKind {
    /// The ID shape: `T-XXXX`, `C-XXXX`, `R-XXXX` or `B-XXXX`.
    pub fn id_spec(self) -> IdSpec {
        match self {
            RecordKind::Task => IdSpec::tasks(),
            RecordKind::Claim => IdSpec::new("C", 4),
            RecordKind::Report => IdSpec::new("R", 4),
            RecordKind::Blocker => IdSpec::new("B", 4),
        }
    }

    /// The folder under the board directory that holds this kind.
    pub fn folder(self) -> &'static str {
        match self {
            RecordKind::Task => "tasks",
            RecordKind::Claim => "claims",
            RecordKind::Report => "reports",
            RecordKind::Blocker => "blockers",
        }
    }
}

/// Where a task stands (docs/05).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TaskStatus {
    Ready,
    Running,
    Review,
    Merged,
    BlockedExternal,
    BlockedTask,
    BlockedHuman,
}

impl TaskStatus {
    /// Every status, in board order: ready, running, review, the three
    /// blocked statuses, merged.
    pub const ALL: [TaskStatus; 7] = [
        TaskStatus::Ready,
        TaskStatus::Running,
        TaskStatus::Review,
        TaskStatus::BlockedExternal,
        TaskStatus::BlockedTask,
        TaskStatus::BlockedHuman,
        TaskStatus::Merged,
    ];

    /// The machine form, as stored and as given on the command line:
    /// `blocked-human`.
    pub fn as_str(self) -> &'static str {
        match self {
            TaskStatus::Ready => "ready",
            TaskStatus::Running => "running",
            TaskStatus::Review => "review",
            TaskStatus::Merged => "merged",
            TaskStatus::BlockedExternal => "blocked-external",
            TaskStatus::BlockedTask => "blocked-task",
            TaskStatus::BlockedHuman => "blocked-human",
        }
    }

    /// The form people read, as in docs/05: `blocked (human)`.
    pub fn label(self) -> &'static str {
        match self {
            TaskStatus::Ready => "ready",
            TaskStatus::Running => "running",
            TaskStatus::Review => "review",
            TaskStatus::Merged => "merged",
            TaskStatus::BlockedExternal => "blocked (external)",
            TaskStatus::BlockedTask => "blocked (task)",
            TaskStatus::BlockedHuman => "blocked (human)",
        }
    }

    /// One of the three blocked statuses.
    pub fn is_blocked(self) -> bool {
        matches!(
            self,
            TaskStatus::BlockedExternal | TaskStatus::BlockedTask | TaskStatus::BlockedHuman
        )
    }
}

impl fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Text that is not a task status.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error(
    "`{0}` is not a task status: expected ready, running, review, merged, blocked-external, blocked-task or blocked-human"
)]
pub struct BadStatus(pub String);

impl FromStr for TaskStatus {
    type Err = BadStatus;
    /// Accepts the machine form (`blocked-human`) and the label
    /// (`blocked (human)`), in any case.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let norm: String = s
            .trim()
            .to_ascii_lowercase()
            .chars()
            .map(|c| if c == ' ' || c == '_' { '-' } else { c })
            .filter(|c| *c != '(' && *c != ')')
            .collect();
        TaskStatus::ALL
            .into_iter()
            .find(|st| st.as_str() == norm)
            .ok_or_else(|| BadStatus(s.to_string()))
    }
}

/// A dated note on a task.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    pub at: String,
    pub by: String,
    pub text: String,
}

/// One task on the board.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub id: Id,
    pub title: String,
    #[serde(default)]
    pub description: String,
    pub status: TaskStatus,
    /// Tasks that must be merged before this one can start.
    #[serde(default)]
    pub blocked_by: Vec<Id>,
    /// Why the task is blocked, for the blocked statuses.
    #[serde(default)]
    pub blocked_reason: Option<String>,
    /// Path globs relative to the repo root that the task expects to change.
    #[serde(default)]
    pub touches: Vec<String>,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub worktree: Option<String>,
    #[serde(default)]
    pub session: Option<String>,
    #[serde(default)]
    pub owner: Option<String>,
    /// Decision IDs recorded for or relevant to this task.
    #[serde(default)]
    pub decisions: Vec<String>,
    #[serde(default)]
    pub notes: Vec<Note>,
    /// RFC 3339 UTC timestamps.
    pub created: String,
    pub updated: String,
}

/// A person's claim on a task while it runs (D-V3ZM). Open while
/// `released_at` is `None`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claim {
    pub id: Id,
    pub task: Id,
    pub owner: String,
    pub claimed_at: String,
    #[serde(default)]
    pub released_at: Option<String>,
}

impl Claim {
    pub fn is_open(&self) -> bool {
        self.released_at.is_none()
    }
}

/// The result of one check command a child ran.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckResult {
    pub command: String,
    pub passed: bool,
    #[serde(default)]
    pub note: Option<String>,
}

/// A child's structured report at the end of its work (`task_report`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    pub id: Id,
    pub task: Id,
    pub at: String,
    #[serde(default)]
    pub session: Option<String>,
    #[serde(default)]
    pub done: Vec<String>,
    #[serde(default)]
    pub not_done: Vec<String>,
    #[serde(default)]
    pub checks: Vec<CheckResult>,
    /// Decision IDs the child recorded.
    #[serde(default)]
    pub decisions: Vec<String>,
    /// Anything that should change the plan.
    #[serde(default)]
    pub plan_changes: Vec<String>,
    #[serde(default)]
    pub summary: String,
    /// True once the PM has folded this report into its task.
    #[serde(default)]
    pub folded: bool,
}

/// What a blocked child is waiting on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BlockerKind {
    /// Something outside the project: a third party, an account, a service.
    External,
    /// Another task that has to land first.
    Task,
    /// A fact or a call only a human can give.
    Human,
}

impl BlockerKind {
    /// The task status a blocker of this kind puts its task in.
    pub fn status(self) -> TaskStatus {
        match self {
            BlockerKind::External => TaskStatus::BlockedExternal,
            BlockerKind::Task => TaskStatus::BlockedTask,
            BlockerKind::Human => TaskStatus::BlockedHuman,
        }
    }
}

/// A child saying it is stuck (`task_blocked`). The PM folds it into the
/// board, which marks it `folded`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Blocker {
    pub id: Id,
    pub task: Id,
    pub at: String,
    #[serde(default)]
    pub session: Option<String>,
    pub kind: BlockerKind,
    /// What it is blocked on: a task ID, a service, a question.
    pub on: String,
    #[serde(default)]
    pub details: String,
    #[serde(default)]
    pub folded: bool,
}

/// Something with an ID that lives in a board folder.
pub trait Record: Serialize + for<'de> Deserialize<'de> {
    const KIND: RecordKind;
    fn id(&self) -> &Id;
}

impl Record for Task {
    const KIND: RecordKind = RecordKind::Task;
    fn id(&self) -> &Id {
        &self.id
    }
}

impl Record for Claim {
    const KIND: RecordKind = RecordKind::Claim;
    fn id(&self) -> &Id {
        &self.id
    }
}

impl Record for Report {
    const KIND: RecordKind = RecordKind::Report;
    fn id(&self) -> &Id {
        &self.id
    }
}

impl Record for Blocker {
    const KIND: RecordKind = RecordKind::Blocker;
    fn id(&self) -> &Id {
        &self.id
    }
}

impl Redactable for Note {
    fn redact_with(self, r: &Redactor) -> Result<Self, RedactError> {
        Ok(Note {
            at: self.at,
            by: self.by.redact_with(r)?,
            text: self.text.redact_with(r)?,
        })
    }
}

impl Redactable for Task {
    fn redact_with(self, r: &Redactor) -> Result<Self, RedactError> {
        Ok(Task {
            id: self.id,
            title: self.title.redact_with(r)?,
            description: self.description.redact_with(r)?,
            status: self.status,
            blocked_by: self.blocked_by,
            blocked_reason: self.blocked_reason.redact_with(r)?,
            touches: self.touches.redact_with(r)?,
            branch: self.branch.redact_with(r)?,
            worktree: self.worktree.redact_with(r)?,
            session: self.session.redact_with(r)?,
            owner: self.owner.redact_with(r)?,
            decisions: self.decisions.redact_with(r)?,
            notes: self.notes.redact_with(r)?,
            created: self.created,
            updated: self.updated,
        })
    }
}

impl Redactable for Claim {
    fn redact_with(self, r: &Redactor) -> Result<Self, RedactError> {
        Ok(Claim {
            owner: self.owner.redact_with(r)?,
            ..self
        })
    }
}

impl Redactable for CheckResult {
    fn redact_with(self, r: &Redactor) -> Result<Self, RedactError> {
        Ok(CheckResult {
            command: self.command.redact_with(r)?,
            passed: self.passed,
            note: self.note.redact_with(r)?,
        })
    }
}

impl Redactable for Report {
    fn redact_with(self, r: &Redactor) -> Result<Self, RedactError> {
        Ok(Report {
            id: self.id,
            task: self.task,
            at: self.at,
            session: self.session.redact_with(r)?,
            done: self.done.redact_with(r)?,
            not_done: self.not_done.redact_with(r)?,
            checks: self.checks.redact_with(r)?,
            decisions: self.decisions.redact_with(r)?,
            plan_changes: self.plan_changes.redact_with(r)?,
            summary: self.summary.redact_with(r)?,
            folded: self.folded,
        })
    }
}

impl Redactable for Blocker {
    fn redact_with(self, r: &Redactor) -> Result<Self, RedactError> {
        Ok(Blocker {
            id: self.id,
            task: self.task,
            at: self.at,
            session: self.session.redact_with(r)?,
            kind: self.kind,
            on: self.on.redact_with(r)?,
            details: self.details.redact_with(r)?,
            folded: self.folded,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_round_trip_in_both_forms() {
        for st in TaskStatus::ALL {
            assert_eq!(st.as_str().parse::<TaskStatus>(), Ok(st));
            assert_eq!(st.label().parse::<TaskStatus>(), Ok(st));
            let json = serde_json::to_string(&st).unwrap_or_default();
            assert_eq!(json, format!("\"{}\"", st.as_str()));
        }
        assert_eq!(
            "Blocked (Human)".parse::<TaskStatus>(),
            Ok(TaskStatus::BlockedHuman)
        );
        assert!("done".parse::<TaskStatus>().is_err());
        assert_eq!(TaskStatus::BlockedExternal.label(), "blocked (external)");
    }

    #[test]
    fn record_kinds_have_their_own_prefixes() {
        let prefixes: Vec<String> = [
            RecordKind::Task,
            RecordKind::Claim,
            RecordKind::Report,
            RecordKind::Blocker,
        ]
        .into_iter()
        .map(|k| k.id_spec().prefix)
        .collect();
        assert_eq!(prefixes, ["T", "C", "R", "B"]);
    }
}
