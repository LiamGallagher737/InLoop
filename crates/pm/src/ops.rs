//! Board operations. These are pure: they take records and return records,
//! and the caller (the app layer) reads and writes the board around them.
//! The PM is the only writer of the board (D-Y40K); children's reports and
//! blockers are folded in here.

use inloop_core::{Id, IdSpec};

use crate::model::{Blocker, BlockerKind, Claim, Note, Report, Task, TaskStatus};

/// Who wrote a note when nobody else is named.
pub const DEFAULT_ACTOR: &str = "pm";

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum OpError {
    #[error("a task needs a title")]
    MissingTitle,
    #[error("{0} is not on the board")]
    UnknownTask(Id),
    #[error("{0} can't be blocked by itself")]
    SelfBlocked(Id),
    #[error("{0} is not a task ID")]
    NotATask(Id),
    #[error("{task} can't be {status} without a branch: set the branch the child works on")]
    RunningNeedsBranch { task: Id, status: TaskStatus },
    #[error("{task} can't be blocked (task) without blocked_by: name the tasks it waits for")]
    BlockedTaskNeedsTasks { task: Id },
    #[error("{task} can't be {} without a reason: say what it is waiting on", status.label())]
    BlockedNeedsReason { task: Id, status: TaskStatus },
    #[error(
        "{task} is already claimed by {owner} (claim {claim}). Two people can't run the same task; talk to {owner} or pick another task"
    )]
    ClaimedByOther { task: Id, owner: String, claim: Id },
    #[error("{record} belongs to {actual}, not {expected}")]
    WrongTask {
        record: Id,
        expected: Id,
        actual: Id,
    },
}

/// What the caller supplies to put a new task on the board.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NewTask {
    pub title: String,
    pub description: String,
    pub touches: Vec<String>,
    pub blocked_by: Vec<Id>,
    pub owner: Option<String>,
}

/// Look up a task's current status by ID, or `None` if it isn't on the board.
pub type Lookup<'a> = &'a dyn Fn(&Id) -> Option<TaskStatus>;

fn ts(now: jiff::Timestamp) -> String {
    now.to_string()
}

fn check_blocked_by(task: &Id, blocked_by: &[Id], lookup: Lookup) -> Result<(), OpError> {
    let spec = IdSpec::tasks();
    for b in blocked_by {
        if b == task {
            return Err(OpError::SelfBlocked(b.clone()));
        }
        if spec.parse(b.as_str()).is_err() {
            return Err(OpError::NotATask(b.clone()));
        }
        if lookup(b).is_none() {
            return Err(OpError::UnknownTask(b.clone()));
        }
    }
    Ok(())
}

fn all_merged(blocked_by: &[Id], lookup: Lookup) -> bool {
    blocked_by
        .iter()
        .all(|b| lookup(b) == Some(TaskStatus::Merged))
}

fn dedupe<T: PartialEq>(items: &mut Vec<T>) {
    let mut out: Vec<T> = Vec::with_capacity(items.len());
    for i in items.drain(..) {
        if !out.contains(&i) {
            out.push(i);
        }
    }
    *items = out;
}

/// Build a new task. It starts `ready`, or `blocked-task` when any task in
/// `blocked_by` isn't merged yet. `id` comes from `Board::new_id`.
pub fn new_task(
    input: NewTask,
    id: Id,
    now: jiff::Timestamp,
    lookup: Lookup,
) -> Result<Task, OpError> {
    let title = input.title.trim().to_string();
    if title.is_empty() {
        return Err(OpError::MissingTitle);
    }
    let mut blocked_by = input.blocked_by;
    dedupe(&mut blocked_by);
    check_blocked_by(&id, &blocked_by, lookup)?;
    let status = if all_merged(&blocked_by, lookup) {
        TaskStatus::Ready
    } else {
        TaskStatus::BlockedTask
    };
    let mut touches = input.touches;
    dedupe(&mut touches);
    let at = ts(now);
    Ok(Task {
        id,
        title,
        description: input.description,
        status,
        blocked_by,
        blocked_reason: None,
        touches,
        branch: None,
        worktree: None,
        session: None,
        owner: input.owner,
        decisions: Vec::new(),
        notes: Vec::new(),
        created: at.clone(),
        updated: at,
    })
}

/// A change to a task. `None` leaves a field alone; for the optional
/// fields, `Some(None)` clears it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TaskUpdate {
    pub status: Option<TaskStatus>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub touches: Option<Vec<String>>,
    pub blocked_by: Option<Vec<Id>>,
    pub blocked_reason: Option<Option<String>>,
    pub branch: Option<Option<String>>,
    pub worktree: Option<Option<String>>,
    pub session: Option<Option<String>>,
    pub owner: Option<Option<String>>,
    pub add_note: Option<String>,
    pub add_decisions: Vec<String>,
}

/// What the caller knows when it applies an update.
pub struct UpdateContext<'a> {
    pub now: jiff::Timestamp,
    /// Who is making the change; the author of any note, and the claim
    /// owner when a task starts running without an owner.
    pub actor: &'a str,
    /// Every claim on this task, open or released.
    pub claims: &'a [Claim],
    /// Status of other tasks, to check `blocked_by`.
    pub lookup: Lookup<'a>,
    /// An unused claim ID, used only if the update opens a claim.
    pub new_claim_id: Id,
}

/// The result of an update: the task, plus any claim records to write.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateOutcome {
    pub task: Task,
    /// A claim opened because the task started running.
    pub opened: Option<Claim>,
    /// Claims released because the task left running or review for ready
    /// or merged.
    pub released: Vec<Claim>,
}

impl UpdateOutcome {
    /// Every claim record that changed, to write back to the board.
    pub fn changed_claims(&self) -> impl Iterator<Item = &Claim> {
        self.opened.iter().chain(self.released.iter())
    }
}

/// Apply `update` to `task`, validating the result and keeping claims in
/// step (D-V3ZM):
///
/// - `running` (and `review`) need a branch; a task moving to `running`
///   opens a claim for its owner (or the actor, who then becomes owner).
/// - An open claim by a different owner is refused: that is two people
///   taking the same task.
/// - Moving to `ready` or `merged` releases open claims.
/// - `blocked-task` needs `blocked_by`; the other blocked statuses need a
///   reason. Leaving a blocked status clears the reason.
/// - When `blocked_by` changes without an explicit status, a `ready` or
///   `blocked-task` task moves between the two to match.
pub fn update_task(
    task: Task,
    update: TaskUpdate,
    ctx: &UpdateContext,
) -> Result<UpdateOutcome, OpError> {
    let mut t = task;
    let at = ts(ctx.now);
    let before = t.status;

    if let Some(title) = update.title {
        let title = title.trim().to_string();
        if title.is_empty() {
            return Err(OpError::MissingTitle);
        }
        t.title = title;
    }
    if let Some(d) = update.description {
        t.description = d;
    }
    if let Some(mut touches) = update.touches {
        dedupe(&mut touches);
        t.touches = touches;
    }
    let blocked_by_changed = update.blocked_by.is_some();
    if let Some(mut b) = update.blocked_by {
        dedupe(&mut b);
        check_blocked_by(&t.id, &b, ctx.lookup)?;
        t.blocked_by = b;
    }
    if let Some(r) = update.blocked_reason {
        t.blocked_reason = r.filter(|s| !s.trim().is_empty());
    }
    if let Some(b) = update.branch {
        t.branch = b.filter(|s| !s.trim().is_empty());
    }
    if let Some(w) = update.worktree {
        t.worktree = w.filter(|s| !s.trim().is_empty());
    }
    if let Some(s) = update.session {
        t.session = s.filter(|s| !s.trim().is_empty());
    }
    if let Some(o) = update.owner {
        t.owner = o.filter(|s| !s.trim().is_empty());
    }

    let status = match update.status {
        Some(s) => s,
        None if blocked_by_changed
            && matches!(t.status, TaskStatus::Ready | TaskStatus::BlockedTask) =>
        {
            if all_merged(&t.blocked_by, ctx.lookup) {
                TaskStatus::Ready
            } else {
                TaskStatus::BlockedTask
            }
        }
        None => t.status,
    };
    t.status = status;

    match status {
        TaskStatus::Running | TaskStatus::Review if t.branch.is_none() => {
            return Err(OpError::RunningNeedsBranch { task: t.id, status });
        }
        TaskStatus::BlockedTask if t.blocked_by.is_empty() => {
            return Err(OpError::BlockedTaskNeedsTasks { task: t.id });
        }
        TaskStatus::BlockedExternal | TaskStatus::BlockedHuman if t.blocked_reason.is_none() => {
            return Err(OpError::BlockedNeedsReason { task: t.id, status });
        }
        _ => {}
    }
    if before.is_blocked() && !status.is_blocked() {
        t.blocked_reason = None;
    }

    // Claims.
    let open: Vec<&Claim> = ctx
        .claims
        .iter()
        .filter(|c| c.is_open() && c.task == t.id)
        .collect();
    let mut opened = None;
    let mut released = Vec::new();
    if status == TaskStatus::Running {
        let owner = t.owner.clone().unwrap_or_else(|| ctx.actor.to_string());
        if let Some(other) = open.iter().find(|c| c.owner != owner) {
            return Err(OpError::ClaimedByOther {
                task: t.id,
                owner: other.owner.clone(),
                claim: other.id.clone(),
            });
        }
        if open.is_empty() {
            opened = Some(Claim {
                id: ctx.new_claim_id.clone(),
                task: t.id.clone(),
                owner: owner.clone(),
                claimed_at: at.clone(),
                released_at: None,
            });
        }
        t.owner = Some(owner);
    } else if matches!(status, TaskStatus::Ready | TaskStatus::Merged) {
        released = open
            .into_iter()
            .map(|c| Claim {
                released_at: Some(at.clone()),
                ..c.clone()
            })
            .collect();
    }

    for d in update.add_decisions {
        if !t.decisions.contains(&d) {
            t.decisions.push(d);
        }
    }
    if let Some(text) = update.add_note.filter(|s| !s.trim().is_empty()) {
        t.notes.push(Note {
            at: at.clone(),
            by: ctx.actor.to_string(),
            text,
        });
    }
    t.updated = at;
    Ok(UpdateOutcome {
        task: t,
        opened,
        released,
    })
}

/// What a child sends with `task_report`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewReport {
    pub task: Id,
    pub session: Option<String>,
    pub done: Vec<String>,
    pub not_done: Vec<String>,
    pub checks: Vec<crate::model::CheckResult>,
    pub decisions: Vec<String>,
    pub plan_changes: Vec<String>,
    pub summary: String,
}

/// Build a report record. `id` comes from `Board::new_id`.
pub fn new_report(input: NewReport, id: Id, now: jiff::Timestamp) -> Report {
    Report {
        id,
        task: input.task,
        at: ts(now),
        session: input.session,
        done: input.done,
        not_done: input.not_done,
        checks: input.checks,
        decisions: input.decisions,
        plan_changes: input.plan_changes,
        summary: input.summary,
    }
}

/// What a child sends with `task_blocked`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewBlocker {
    pub task: Id,
    pub session: Option<String>,
    pub kind: BlockerKind,
    pub on: String,
    pub details: String,
}

/// Build a blocker record, not yet folded. `id` comes from `Board::new_id`.
pub fn new_blocker(input: NewBlocker, id: Id, now: jiff::Timestamp) -> Blocker {
    Blocker {
        id,
        task: input.task,
        at: ts(now),
        session: input.session,
        kind: input.kind,
        on: input.on,
        details: input.details,
        folded: false,
    }
}

/// Fold a child's blocker into its task: the task takes the matching
/// blocked status and reason, a `task` blocker whose `on` is a task ID is
/// added to `blocked_by`, and the blocker is marked folded. Claims stay
/// open: the task is still that owner's.
pub fn apply_blocker(
    task: Task,
    blocker: Blocker,
    now: jiff::Timestamp,
) -> Result<(Task, Blocker), OpError> {
    if blocker.task != task.id {
        return Err(OpError::WrongTask {
            record: blocker.id,
            expected: task.id,
            actual: blocker.task,
        });
    }
    let mut t = task;
    let at = ts(now);
    let on = blocker.on.trim();
    if blocker.kind == BlockerKind::Task
        && let Ok(id) = IdSpec::tasks().parse(on)
        && id != t.id
        && !t.blocked_by.contains(&id)
    {
        t.blocked_by.push(id);
    }
    let reason = if blocker.details.trim().is_empty() {
        on.to_string()
    } else {
        format!("{on}: {}", blocker.details.trim())
    };
    t.status = blocker.kind.status();
    if t.status == TaskStatus::BlockedTask && t.blocked_by.is_empty() {
        // `on` named no task ID; keep the reason so the PM can sort it out.
        t.status = TaskStatus::BlockedHuman;
    }
    t.blocked_reason = Some(reason.clone());
    t.notes.push(Note {
        at: at.clone(),
        by: blocker.session.clone().unwrap_or_else(|| "child".into()),
        text: format!("Blocked ({}): {reason}", kind_word(blocker.kind)),
    });
    t.updated = at;
    let folded = Blocker {
        folded: true,
        ..blocker
    };
    Ok((t, folded))
}

fn kind_word(k: BlockerKind) -> &'static str {
    match k {
        BlockerKind::External => "external",
        BlockerKind::Task => "task",
        BlockerKind::Human => "human",
    }
}

/// Fold a child's report into its task: a `running` task moves to
/// `review`, the report's decisions are added, the session is filled in if
/// unknown, and a note records the summary and what isn't done. A task in
/// any other status keeps it; the PM decides.
pub fn apply_report(task: Task, report: &Report, now: jiff::Timestamp) -> Result<Task, OpError> {
    if report.task != task.id {
        return Err(OpError::WrongTask {
            record: report.id.clone(),
            expected: task.id,
            actual: report.task.clone(),
        });
    }
    let mut t = task;
    let at = ts(now);
    if t.status == TaskStatus::Running {
        t.status = TaskStatus::Review;
    }
    for d in &report.decisions {
        if !t.decisions.contains(d) {
            t.decisions.push(d.clone());
        }
    }
    if t.session.is_none() {
        t.session = report.session.clone();
    }
    let failed: Vec<&str> = report
        .checks
        .iter()
        .filter(|c| !c.passed)
        .map(|c| c.command.as_str())
        .collect();
    let mut text = format!("Report {}: {}", report.id, report.summary.trim());
    if !report.not_done.is_empty() {
        text.push_str(&format!(" Not done: {}.", report.not_done.join("; ")));
    }
    if !failed.is_empty() {
        text.push_str(&format!(" Failed checks: {}.", failed.join(", ")));
    }
    if !report.plan_changes.is_empty() {
        text.push_str(&format!(
            " Plan changes: {}.",
            report.plan_changes.join("; ")
        ));
    }
    t.notes.push(Note {
        at: at.clone(),
        by: report.session.clone().unwrap_or_else(|| "child".into()),
        text,
    });
    t.updated = at;
    Ok(t)
}

/// Tasks in `blocked-task` whose `blocked_by` tasks are all merged: what
/// the PM can propose after landing work. Sorted by ID.
pub fn newly_unblocked(tasks: &[Task]) -> Vec<Id> {
    let status_of = |id: &Id| tasks.iter().find(|t| &t.id == id).map(|t| t.status);
    let mut out: Vec<Id> = tasks
        .iter()
        .filter(|t| t.status == TaskStatus::BlockedTask)
        .filter(|t| {
            t.blocked_by
                .iter()
                .all(|b| status_of(b) == Some(TaskStatus::Merged))
        })
        .map(|t| t.id.clone())
        .collect();
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::CheckResult;
    use std::collections::BTreeMap;

    fn id(s: &str) -> Id {
        Id::parse_loose(s).unwrap_or_else(|_| unreachable!("bad id {s}"))
    }

    fn now() -> jiff::Timestamp {
        jiff::Timestamp::from_second(1_800_000_000).unwrap_or_else(|_| unreachable!())
    }

    fn later() -> jiff::Timestamp {
        jiff::Timestamp::from_second(1_800_003_600).unwrap_or_else(|_| unreachable!())
    }

    struct World(BTreeMap<Id, TaskStatus>);
    impl World {
        fn new(entries: &[(&str, TaskStatus)]) -> Self {
            World(entries.iter().map(|(i, s)| (id(i), *s)).collect())
        }
        fn get(&self, i: &Id) -> Option<TaskStatus> {
            self.0.get(i).copied()
        }
    }

    fn base(world: &World) -> Task {
        let lookup = |i: &Id| world.get(i);
        new_task(
            NewTask {
                title: "  Add search  ".into(),
                description: "Full text search".into(),
                touches: vec!["crates/index/**".into(), "crates/index/**".into()],
                blocked_by: vec![],
                owner: Some("sam".into()),
            },
            id("T-AAAA"),
            now(),
            &lookup,
        )
        .unwrap_or_else(|e| unreachable!("{e}"))
    }

    fn ctx<'a>(world: &'a World, claims: &'a [Claim], lookup: Lookup<'a>) -> UpdateContext<'a> {
        let _ = world;
        UpdateContext {
            now: later(),
            actor: "pm",
            claims,
            lookup,
            new_claim_id: id("C-0001"),
        }
    }

    #[test]
    fn new_task_is_ready_and_tidy() {
        let world = World::new(&[]);
        let t = base(&world);
        assert_eq!(t.status, TaskStatus::Ready);
        assert_eq!(t.title, "Add search");
        assert_eq!(t.touches, ["crates/index/**"]);
        assert_eq!(t.created, "2027-01-15T08:00:00Z");
        assert_eq!(t.created, t.updated);
    }

    #[test]
    fn new_task_blocked_by_unmerged_work() {
        let world = World::new(&[
            ("T-BBBB", TaskStatus::Running),
            ("T-CCCC", TaskStatus::Merged),
        ]);
        let lookup = |i: &Id| world.get(i);
        let mk = |blocked_by: Vec<Id>| {
            new_task(
                NewTask {
                    title: "x".into(),
                    blocked_by,
                    ..Default::default()
                },
                id("T-AAAA"),
                now(),
                &lookup,
            )
        };
        assert_eq!(
            mk(vec![id("T-BBBB"), id("T-CCCC")]).map(|t| t.status),
            Ok(TaskStatus::BlockedTask)
        );
        assert_eq!(
            mk(vec![id("T-CCCC")]).map(|t| t.status),
            Ok(TaskStatus::Ready)
        );
        assert_eq!(
            mk(vec![id("T-ZZZZ")]),
            Err(OpError::UnknownTask(id("T-ZZZZ")))
        );
        assert_eq!(
            mk(vec![id("T-AAAA")]),
            Err(OpError::SelfBlocked(id("T-AAAA")))
        );
        assert_eq!(mk(vec![id("D-AAAA")]), Err(OpError::NotATask(id("D-AAAA"))));
        let none = new_task(NewTask::default(), id("T-AAAA"), now(), &lookup);
        assert_eq!(none, Err(OpError::MissingTitle));
    }

    #[test]
    fn running_needs_a_branch_and_opens_a_claim() {
        let world = World::new(&[]);
        let lookup = |i: &Id| world.get(i);
        let t = base(&world);
        let c = ctx(&world, &[], &lookup);
        let no_branch = update_task(
            t.clone(),
            TaskUpdate {
                status: Some(TaskStatus::Running),
                ..Default::default()
            },
            &c,
        );
        assert!(matches!(no_branch, Err(OpError::RunningNeedsBranch { .. })));

        let out = update_task(
            t,
            TaskUpdate {
                status: Some(TaskStatus::Running),
                branch: Some(Some("task/search".into())),
                worktree: Some(Some("../wt-search".into())),
                add_note: Some("Started".into()),
                ..Default::default()
            },
            &c,
        )
        .unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(out.task.status, TaskStatus::Running);
        assert_eq!(out.task.updated, "2027-01-15T09:00:00Z");
        assert_eq!(out.task.notes.len(), 1);
        assert_eq!(out.task.notes[0].by, "pm");
        let claim = out.opened.clone().unwrap_or_else(|| unreachable!());
        assert_eq!(claim.owner, "sam");
        assert_eq!(claim.task, id("T-AAAA"));
        assert!(claim.is_open());
        assert!(out.released.is_empty());
        assert_eq!(out.changed_claims().count(), 1);
    }

    #[test]
    fn running_without_owner_claims_for_the_actor() {
        let world = World::new(&[]);
        let lookup = |i: &Id| world.get(i);
        let mut t = base(&world);
        t.owner = None;
        let out = update_task(
            t,
            TaskUpdate {
                status: Some(TaskStatus::Running),
                branch: Some(Some("b".into())),
                ..Default::default()
            },
            &ctx(&world, &[], &lookup),
        )
        .unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(out.task.owner.as_deref(), Some("pm"));
        assert_eq!(out.opened.map(|c| c.owner).as_deref(), Some("pm"));
    }

    fn open_claim(owner: &str) -> Claim {
        Claim {
            id: id("C-9999"),
            task: id("T-AAAA"),
            owner: owner.into(),
            claimed_at: "2026-11-14T00:00:00Z".into(),
            released_at: None,
        }
    }

    #[test]
    fn a_second_owner_is_refused() {
        let world = World::new(&[]);
        let lookup = |i: &Id| world.get(i);
        let t = base(&world);
        let claims = [open_claim("alex")];
        let err = update_task(
            t,
            TaskUpdate {
                status: Some(TaskStatus::Running),
                branch: Some(Some("b".into())),
                ..Default::default()
            },
            &ctx(&world, &claims, &lookup),
        );
        assert_eq!(
            err,
            Err(OpError::ClaimedByOther {
                task: id("T-AAAA"),
                owner: "alex".into(),
                claim: id("C-9999"),
            })
        );
        let msg = err.map(|_| String::new()).unwrap_or_else(|e| e.to_string());
        assert!(msg.contains("already claimed by alex"), "{msg}");
    }

    #[test]
    fn the_same_owner_keeps_their_claim() {
        let world = World::new(&[]);
        let lookup = |i: &Id| world.get(i);
        let mut t = base(&world);
        t.status = TaskStatus::BlockedHuman;
        t.blocked_reason = Some("which colour".into());
        t.branch = Some("b".into());
        let claims = [open_claim("sam")];
        let out = update_task(
            t,
            TaskUpdate {
                status: Some(TaskStatus::Running),
                ..Default::default()
            },
            &ctx(&world, &claims, &lookup),
        )
        .unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(out.opened, None);
        assert_eq!(out.task.blocked_reason, None);
    }

    #[test]
    fn merging_or_resetting_releases_claims() {
        let world = World::new(&[]);
        let lookup = |i: &Id| world.get(i);
        let mut t = base(&world);
        t.status = TaskStatus::Review;
        t.branch = Some("b".into());
        let mut released_one = open_claim("old");
        released_one.id = id("C-8888");
        released_one.released_at = Some("2026-11-13T00:00:00Z".into());
        let claims = [open_claim("sam"), released_one];
        for status in [TaskStatus::Merged, TaskStatus::Ready] {
            let out = update_task(
                t.clone(),
                TaskUpdate {
                    status: Some(status),
                    ..Default::default()
                },
                &ctx(&world, &claims, &lookup),
            )
            .unwrap_or_else(|e| unreachable!("{e}"));
            assert_eq!(out.released.len(), 1);
            assert_eq!(out.released[0].id, id("C-9999"));
            assert_eq!(
                out.released[0].released_at.as_deref(),
                Some("2027-01-15T09:00:00Z")
            );
        }
        // Going back to running from review keeps the claim, no new one.
        let out = update_task(
            t,
            TaskUpdate {
                status: Some(TaskStatus::Running),
                ..Default::default()
            },
            &ctx(&world, &claims, &lookup),
        )
        .unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(out.opened, None);
        assert!(out.released.is_empty());
    }

    #[test]
    fn blocked_statuses_need_their_details() {
        let world = World::new(&[("T-BBBB", TaskStatus::Ready)]);
        let lookup = |i: &Id| world.get(i);
        let t = base(&world);
        let c = ctx(&world, &[], &lookup);
        let up = |u: TaskUpdate| update_task(t.clone(), u, &c).map(|o| o.task);
        assert!(matches!(
            up(TaskUpdate {
                status: Some(TaskStatus::BlockedHuman),
                ..Default::default()
            }),
            Err(OpError::BlockedNeedsReason { .. })
        ));
        assert!(matches!(
            up(TaskUpdate {
                status: Some(TaskStatus::BlockedTask),
                ..Default::default()
            }),
            Err(OpError::BlockedTaskNeedsTasks { .. })
        ));
        let ok = up(TaskUpdate {
            status: Some(TaskStatus::BlockedExternal),
            blocked_reason: Some(Some("waiting on the vendor".into())),
            ..Default::default()
        });
        assert_eq!(ok.map(|t| t.status), Ok(TaskStatus::BlockedExternal));
        // Changing blocked_by alone moves ready to blocked-task.
        let auto = up(TaskUpdate {
            blocked_by: Some(vec![id("T-BBBB")]),
            ..Default::default()
        });
        assert_eq!(auto.map(|t| t.status), Ok(TaskStatus::BlockedTask));
        assert_eq!(
            up(TaskUpdate {
                blocked_by: Some(vec![id("T-QQQQ")]),
                ..Default::default()
            }),
            Err(OpError::UnknownTask(id("T-QQQQ")))
        );
    }

    #[test]
    fn update_edits_fields_and_dedupes_decisions() {
        let world = World::new(&[]);
        let lookup = |i: &Id| world.get(i);
        let mut t = base(&world);
        t.decisions = vec!["D-AAAA".into()];
        let out = update_task(
            t,
            TaskUpdate {
                title: Some("Search v2".into()),
                description: Some("More".into()),
                touches: Some(vec!["a".into(), "a".into(), "b".into()]),
                owner: Some(None),
                add_decisions: vec!["D-AAAA".into(), "D-BBBB".into()],
                add_note: Some("   ".into()),
                ..Default::default()
            },
            &ctx(&world, &[], &lookup),
        )
        .unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(out.task.title, "Search v2");
        assert_eq!(out.task.touches, ["a", "b"]);
        assert_eq!(out.task.owner, None);
        assert_eq!(out.task.decisions, ["D-AAAA", "D-BBBB"]);
        assert!(out.task.notes.is_empty());
        assert_eq!(
            update_task(
                out.task,
                TaskUpdate {
                    title: Some(" ".into()),
                    ..Default::default()
                },
                &ctx(&world, &[], &lookup)
            ),
            Err(OpError::MissingTitle)
        );
    }

    fn running(world: &World) -> Task {
        let mut t = base(world);
        t.status = TaskStatus::Running;
        t.branch = Some("task/search".into());
        t
    }

    #[test]
    fn blockers_fold_into_the_task() {
        let world = World::new(&[]);
        let t = running(&world);
        let b = new_blocker(
            NewBlocker {
                task: id("T-AAAA"),
                session: Some("sess-1".into()),
                kind: BlockerKind::Task,
                on: "t-bbbb".into(),
                details: "needs the index schema".into(),
            },
            id("B-0001"),
            now(),
        );
        assert!(!b.folded);
        let (task, folded) =
            apply_blocker(t.clone(), b, later()).unwrap_or_else(|e| unreachable!("{e}"));
        assert!(folded.folded);
        assert_eq!(task.status, TaskStatus::BlockedTask);
        assert_eq!(task.blocked_by, [id("T-BBBB")]);
        assert_eq!(
            task.blocked_reason.as_deref(),
            Some("t-bbbb: needs the index schema")
        );
        assert_eq!(task.notes.last().map(|n| n.by.as_str()), Some("sess-1"));

        // A task blocker that names no task becomes a human question.
        let vague = new_blocker(
            NewBlocker {
                task: id("T-AAAA"),
                session: None,
                kind: BlockerKind::Task,
                on: "the other thing".into(),
                details: String::new(),
            },
            id("B-0002"),
            now(),
        );
        let (task2, _) =
            apply_blocker(t.clone(), vague, later()).unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(task2.status, TaskStatus::BlockedHuman);
        assert_eq!(task2.blocked_reason.as_deref(), Some("the other thing"));

        let ext = new_blocker(
            NewBlocker {
                task: id("T-AAAA"),
                session: None,
                kind: BlockerKind::External,
                on: "vendor API keys".into(),
                details: String::new(),
            },
            id("B-0003"),
            now(),
        );
        let (task3, _) =
            apply_blocker(t.clone(), ext, later()).unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(task3.status, TaskStatus::BlockedExternal);

        let mut wrong = new_blocker(
            NewBlocker {
                task: id("T-ZZZZ"),
                session: None,
                kind: BlockerKind::Human,
                on: "x".into(),
                details: String::new(),
            },
            id("B-0004"),
            now(),
        );
        wrong.folded = false;
        assert!(matches!(
            apply_blocker(t, wrong, later()),
            Err(OpError::WrongTask { .. })
        ));
    }

    #[test]
    fn reports_move_running_to_review() {
        let world = World::new(&[]);
        let t = running(&world);
        let r = new_report(
            NewReport {
                task: id("T-AAAA"),
                session: Some("sess-1".into()),
                done: vec!["indexing".into()],
                not_done: vec!["ranking".into()],
                checks: vec![
                    CheckResult {
                        command: "cargo test".into(),
                        passed: true,
                        note: None,
                    },
                    CheckResult {
                        command: "cargo clippy".into(),
                        passed: false,
                        note: Some("two warnings".into()),
                    },
                ],
                decisions: vec!["D-CCCC".into(), "D-CCCC".into()],
                plan_changes: vec!["split ranking out".into()],
                summary: "Indexing works.".into(),
            },
            id("R-0001"),
            now(),
        );
        let out = apply_report(t.clone(), &r, later()).unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(out.status, TaskStatus::Review);
        assert_eq!(out.decisions, ["D-CCCC"]);
        assert_eq!(out.session.as_deref(), Some("sess-1"));
        let note = out.notes.last().map(|n| n.text.clone()).unwrap_or_default();
        assert_eq!(
            note,
            "Report R-0001: Indexing works. Not done: ranking. Failed checks: cargo clippy. Plan changes: split ranking out."
        );
        // Another status is left for the PM to judge.
        let mut ready = t;
        ready.status = TaskStatus::Ready;
        let out = apply_report(ready, &r, later()).unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(out.status, TaskStatus::Ready);
    }

    #[test]
    fn newly_unblocked_finds_tasks_whose_blockers_merged() {
        let mk = |i: &str, status: TaskStatus, blocked_by: &[&str]| Task {
            id: id(i),
            title: i.into(),
            description: String::new(),
            status,
            blocked_by: blocked_by.iter().map(|b| id(b)).collect(),
            blocked_reason: None,
            touches: vec![],
            branch: None,
            worktree: None,
            session: None,
            owner: None,
            decisions: vec![],
            notes: vec![],
            created: String::new(),
            updated: String::new(),
        };
        let tasks = vec![
            mk("T-0A01", TaskStatus::Merged, &[]),
            mk("T-0A02", TaskStatus::Review, &[]),
            mk("T-0A03", TaskStatus::BlockedTask, &["T-0A01"]),
            mk("T-0A04", TaskStatus::BlockedTask, &["T-0A01", "T-0A02"]),
            mk("T-0A05", TaskStatus::BlockedTask, &["T-0A99"]),
            mk("T-0A06", TaskStatus::BlockedHuman, &["T-0A01"]),
            mk("T-0A07", TaskStatus::BlockedTask, &[]),
        ];
        assert_eq!(newly_unblocked(&tasks), [id("T-0A03"), id("T-0A07")]);
    }
}
