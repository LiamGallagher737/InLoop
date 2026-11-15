//! The PM toolset (docs/05): registered only when `[pm] enabled = true`,
//! and it never changes how the core tools behave (D-N6YH). Only the PM,
//! in the main worktree, writes tasks (D-Y40K); children report.

use inloop_app::{AppError, Project};
use inloop_core::Id;
use inloop_pm::{BlockerKind, CheckResult, NewBlocker, NewReport, NewTask, TaskStatus, TaskUpdate};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::schemars::{self, JsonSchema};
use rmcp::{tool, tool_router};
use serde::Deserialize;

use crate::InLoopServer;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct Empty {}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TaskIdParams {
    /// One or more task IDs such as T-4M1P (or 4M1P: the prefix is optional).
    #[serde(alias = "id", deserialize_with = "crate::one_or_many")]
    pub ids: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CheckParams {
    /// Tasks you want to run together. Empty means every ready task. Running tasks are always included.
    #[serde(default)]
    pub ids: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TaskNewParams {
    pub title: String,
    #[serde(default)]
    pub description: String,
    /// Path globs the task will touch, relative to the repo root, e.g. "crates/core/**".
    #[serde(default)]
    pub touches: Vec<String>,
    /// Tasks that must merge first.
    #[serde(default)]
    pub blocked_by: Vec<String>,
    pub owner: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TaskUpdateParams {
    /// One or more task IDs, all given the same change. The prefix is optional.
    #[serde(alias = "id", deserialize_with = "crate::one_or_many")]
    pub ids: Vec<String>,
    /// ready, running, review, merged, blocked-external, blocked-task or blocked-human.
    pub status: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub touches: Option<Vec<String>>,
    pub blocked_by: Option<Vec<String>>,
    /// Why it's blocked (needed for blocked-external and blocked-human). Empty clears it.
    pub blocked_reason: Option<String>,
    /// The branch the task's session works on (needed for running). Empty clears it.
    pub branch: Option<String>,
    pub worktree: Option<String>,
    pub session: Option<String>,
    pub owner: Option<String>,
    /// A note to add to the task's history.
    pub note: Option<String>,
    /// Decision IDs to link to the task.
    #[serde(default)]
    pub decisions: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CheckResultParam {
    pub command: String,
    pub passed: bool,
    pub note: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReportParams {
    /// The task. Defaults to the one owning this branch.
    pub task: Option<String>,
    /// What is finished.
    #[serde(default)]
    pub done: Vec<String>,
    /// Anything left or skipped.
    #[serde(default)]
    pub not_done: Vec<String>,
    /// Each check command, whether it passed, and a note if not.
    #[serde(default)]
    pub checks: Vec<CheckResultParam>,
    /// Decision IDs you recorded.
    #[serde(default)]
    pub decisions: Vec<String>,
    /// Anything that should change the plan.
    #[serde(default)]
    pub plan_changes: Vec<String>,
    pub summary: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct BlockedParams {
    /// The task. Defaults to the one owning this branch.
    pub task: Option<String>,
    /// external (an outside service or account), task (another task must land first; give its ID in `on`) or human (a fact or call only a person can give).
    pub kind: String,
    /// What you're blocked on.
    pub on: String,
    pub details: String,
}

fn opt(s: Option<String>) -> Option<Option<String>> {
    s.map(|v| if v.trim().is_empty() { None } else { Some(v) })
}

fn ids(p: &Project, v: Vec<String>) -> Result<Vec<Id>, AppError> {
    let _ = p;
    v.iter()
        .map(|s| {
            inloop_core::IdSpec::tasks()
                .parse(s)
                .map_err(|e| AppError::Invalid(e.to_string()))
        })
        .collect()
}

fn update_from(proj: &Project, p: &TaskUpdateParams) -> Result<TaskUpdate, AppError> {
    let status = match &p.status {
        None => None,
        Some(s) => Some(
            s.parse::<TaskStatus>()
                .map_err(|e| AppError::Invalid(e.to_string()))?,
        ),
    };
    let blocked_by = match &p.blocked_by {
        None => None,
        Some(v) => Some(ids(proj, v.clone())?),
    };
    Ok(TaskUpdate {
        status,
        title: p.title.clone(),
        description: p.description.clone(),
        touches: p.touches.clone(),
        blocked_by,
        blocked_reason: opt(p.blocked_reason.clone()),
        branch: opt(p.branch.clone()),
        worktree: opt(p.worktree.clone()),
        session: opt(p.session.clone()),
        owner: opt(p.owner.clone()),
        add_note: p.note.clone(),
        add_decisions: p.decisions.clone(),
    })
}

fn pm_only(p: &Project) -> Result<(), AppError> {
    if p.in_main_worktree() {
        Ok(())
    } else {
        Err(AppError::Invalid(
            "only the project manager, in the main checkout, changes tasks; from a task worktree use task_report or task_blocked".into(),
        ))
    }
}

#[tool_router(router = pm_router, vis = "pub")]
impl InLoopServer {
    #[tool(
        description = "The work board: every task, plus blockers and reports from task sessions not folded in yet, and tasks newly unblocked."
    )]
    pub async fn board_list(&self, Parameters(_p): Parameters<Empty>) -> CallToolResult {
        self.with_project(|p| p.board_list()).await
    }

    #[tool(
        description = "Before proposing tasks to run side by side, check them (and every running task) for overlapping paths and shared conflict groups."
    )]
    pub async fn board_check(&self, Parameters(p): Parameters<CheckParams>) -> CallToolResult {
        self.with_project(move |proj| proj.board_check(&p.ids))
            .await
    }

    #[tool(
        description = "Fold task sessions' blockers and reports into the board: blocked tasks get their status and reason, reported tasks move to review with a note."
    )]
    pub async fn board_fold(&self, Parameters(_p): Parameters<Empty>) -> CallToolResult {
        self.with_project(|p| {
            pm_only(p)?;
            p.board_fold()
        })
        .await
    }

    #[tool(
        description = "Tasks with their claims, reports and blockers. Pass several IDs to get them all in one call."
    )]
    pub async fn task_get(&self, Parameters(p): Parameters<TaskIdParams>) -> CallToolResult {
        self.with_project_each(p.ids, |_, _| Ok(()), |proj, id| proj.task_get(id))
            .await
    }

    #[tool(description = "Add a task to the board (project manager only).")]
    pub async fn task_new(&self, Parameters(p): Parameters<TaskNewParams>) -> CallToolResult {
        self.with_project(move |proj| {
            pm_only(proj)?;
            let blocked_by = ids(proj, p.blocked_by)?;
            proj.task_new(NewTask {
                title: p.title,
                description: p.description,
                touches: p.touches,
                blocked_by,
                owner: p.owner,
            })
        })
        .await
    }

    #[tool(
        description = "Change tasks (project manager only). Several IDs all get the same change. Moving to running needs a branch and claims the task; moving to ready or merged releases the claim."
    )]
    pub async fn task_update(&self, Parameters(p): Parameters<TaskUpdateParams>) -> CallToolResult {
        let ids = p.ids.clone();
        self.with_project_each(
            ids,
            |proj, ids| {
                pm_only(proj)?;
                proj.tasks_exist(ids)
            },
            move |proj, id| {
                pm_only(proj)?;
                proj.task_update(id, update_from(proj, &p)?, "pm")
                    .map(|o| o.task)
            },
        )
        .await
    }

    #[tool(
        description = "The generated brief to start a task's session with: the task, docs to read, rules, checks, relevant decisions and the report format. Pass several IDs to get each brief in one call."
    )]
    pub async fn task_brief(&self, Parameters(p): Parameters<TaskIdParams>) -> CallToolResult {
        self.with_project_each(p.ids, |_, _| Ok(()), |proj, id| proj.task_brief(id))
            .await
    }

    #[tool(
        description = "At the end of a task, report once: what's done, what isn't, check results, decisions recorded and plan changes. Be plain about what doesn't work."
    )]
    pub async fn task_report(&self, Parameters(p): Parameters<ReportParams>) -> CallToolResult {
        self.with_project(move |proj| {
            let task = proj.child_task(p.task.as_deref())?;
            proj.task_report(NewReport {
                task,
                session: proj.current_session(),
                done: p.done,
                not_done: p.not_done,
                checks: p
                    .checks
                    .into_iter()
                    .map(|c| CheckResult {
                        command: c.command,
                        passed: c.passed,
                        note: c.note,
                    })
                    .collect(),
                decisions: p.decisions,
                plan_changes: p.plan_changes,
                summary: p.summary,
            })
        })
        .await
    }

    #[tool(
        description = "Report that a task is blocked on something you can't resolve yourself, then stop and wait."
    )]
    pub async fn task_blocked(&self, Parameters(p): Parameters<BlockedParams>) -> CallToolResult {
        self.with_project(move |proj| {
            let task = proj.child_task(p.task.as_deref())?;
            let kind = match p.kind.trim().to_ascii_lowercase().as_str() {
                "external" => BlockerKind::External,
                "task" => BlockerKind::Task,
                "human" => BlockerKind::Human,
                other => {
                    return Err(AppError::Invalid(format!(
                        "kind `{other}`: use external, task or human"
                    )));
                }
            };
            proj.task_blocked(NewBlocker {
                task,
                session: proj.current_session(),
                kind,
                on: p.on,
                details: p.details,
            })
        })
        .await
    }
}
