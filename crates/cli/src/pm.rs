//! `inloop board` and `inloop task`: the optional PM layer (docs/05). These
//! refuse to run unless the project sets `[pm] enabled = true`.

use std::process::ExitCode;

use clap::{Args, Subcommand};
use inloop_core::IdSpec;
use inloop_pm::{
    BlockerKind, CheckResult, NewBlocker, NewReport, NewTask, Task, TaskStatus, TaskUpdate,
};

use crate::Ctx;
use crate::output::{self, Style};

#[derive(Subcommand)]
pub enum BoardCmd {
    /// Write the board as Markdown, for people who don't run InLoop.
    Export,
    /// Fold task sessions' blockers and reports into the board.
    Fold,
}

#[derive(Subcommand)]
pub enum TaskCmd {
    /// Add a task.
    New(TaskNewArgs),
    /// Show tasks with their claims, reports and blockers.
    Show {
        /// One or more IDs; the `T-` prefix is optional.
        #[arg(required = true)]
        ids: Vec<String>,
    },
    /// Change a task.
    Update(TaskUpdateArgs),
    /// Print the generated brief for each task's session.
    Brief {
        /// One or more IDs; the `T-` prefix is optional.
        #[arg(required = true)]
        ids: Vec<String>,
    },
    /// Conflict check: the given tasks (default: every ready one) plus every running one.
    Check { ids: Vec<String> },
    /// Report the end of work on a task (what a task session does through MCP).
    Report(ReportArgs),
    /// Report that a task is blocked.
    Blocked(BlockedArgs),
}

#[derive(Args)]
pub struct TaskNewArgs {
    pub title: String,
    #[arg(long, default_value = "")]
    pub description: String,
    /// Path globs the task touches (repeatable).
    #[arg(long = "touch")]
    pub touches: Vec<String>,
    /// Tasks that must merge first (repeatable).
    #[arg(long = "blocked-by")]
    pub blocked_by: Vec<String>,
    #[arg(long)]
    pub owner: Option<String>,
}

#[derive(Args)]
pub struct TaskUpdateArgs {
    /// One or more IDs, all given the same change; the `T-` prefix is optional.
    #[arg(required = true)]
    pub ids: Vec<String>,
    #[arg(long)]
    pub status: Option<String>,
    #[arg(long)]
    pub title: Option<String>,
    #[arg(long)]
    pub description: Option<String>,
    /// Replace the touched paths (repeatable).
    #[arg(long = "touch")]
    pub touches: Vec<String>,
    /// Replace the blockers (repeatable).
    #[arg(long = "blocked-by")]
    pub blocked_by: Vec<String>,
    /// Why it's blocked; empty clears it.
    #[arg(long)]
    pub reason: Option<String>,
    /// The task's branch; empty clears it.
    #[arg(long)]
    pub branch: Option<String>,
    #[arg(long)]
    pub worktree: Option<String>,
    #[arg(long)]
    pub session: Option<String>,
    #[arg(long)]
    pub owner: Option<String>,
    #[arg(long)]
    pub note: Option<String>,
    /// Link a decision (repeatable).
    #[arg(long = "decision")]
    pub decisions: Vec<String>,
}

#[derive(Args)]
pub struct ReportArgs {
    /// The task (defaults to the one owning this branch).
    #[arg(long)]
    pub task: Option<String>,
    #[arg(long)]
    pub summary: String,
    #[arg(long = "done")]
    pub done: Vec<String>,
    #[arg(long = "not-done")]
    pub not_done: Vec<String>,
    /// A passing check command (repeatable).
    #[arg(long = "passed")]
    pub passed: Vec<String>,
    /// A failing check command (repeatable).
    #[arg(long = "failed")]
    pub failed: Vec<String>,
    #[arg(long = "decision")]
    pub decisions: Vec<String>,
    #[arg(long = "plan-change")]
    pub plan_changes: Vec<String>,
}

#[derive(Args)]
pub struct BlockedArgs {
    #[arg(long)]
    pub task: Option<String>,
    /// external, task or human.
    #[arg(long)]
    pub kind: String,
    #[arg(long)]
    pub on: String,
    #[arg(long, default_value = "")]
    pub details: String,
}

fn opt(s: Option<String>) -> Option<Option<String>> {
    s.map(|v| if v.trim().is_empty() { None } else { Some(v) })
}

fn ids(v: &[String]) -> anyhow::Result<Vec<inloop_core::Id>> {
    v.iter().map(|s| Ok(IdSpec::tasks().parse(s)?)).collect()
}

pub fn board(ctx: &Ctx, cmd: Option<BoardCmd>) -> anyhow::Result<ExitCode> {
    let p = ctx.project()?;
    match cmd {
        None => {
            let b = p.board_list()?;
            ctx.emit(&b, |s| board_text(s, &b))?;
        }
        Some(BoardCmd::Export) => {
            let md = p.board_export()?;
            if ctx.json {
                println!("{}", serde_json::json!({ "markdown": md }));
            } else {
                print!("{md}");
            }
        }
        Some(BoardCmd::Fold) => {
            let f = p.board_fold()?;
            ctx.emit(&f, |s| {
                format!(
                    "{} folded {} blockers and {} reports into {} tasks\n",
                    s.green("✓"),
                    f.blockers_folded.len(),
                    f.reports_folded.len(),
                    f.tasks_changed.len()
                )
            })?;
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn board_text(s: &Style, b: &inloop_app::pm::BoardView) -> String {
    let mut out = String::new();
    for status in TaskStatus::ALL {
        let ts: Vec<&Task> = b.tasks.iter().filter(|t| t.status == status).collect();
        if ts.is_empty() {
            continue;
        }
        out.push_str(&format!(
            "{} ({})\n",
            s.bold(&s.status(status.label())),
            ts.len()
        ));
        let rows: Vec<Vec<String>> = ts
            .iter()
            .map(|t| {
                let mut extra = Vec::new();
                if let Some(br) = &t.branch {
                    extra.push(br.clone());
                }
                if let Some(r) = &t.blocked_reason {
                    extra.push(format!("blocked: {r}"));
                }
                if !t.blocked_by.is_empty() {
                    extra.push(format!(
                        "after {}",
                        t.blocked_by
                            .iter()
                            .map(|i| i.to_string())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
                vec![
                    format!("  {}", t.id),
                    t.title.clone(),
                    s.dim(&extra.join(" · ")),
                ]
            })
            .collect();
        let table = output::table(s, &["", "", ""], &rows);
        out.push_str(table.split_once('\n').map(|x| x.1).unwrap_or(""));
        out.push('\n');
    }
    if b.tasks.is_empty() {
        out.push_str("The board is empty. Add a task with `inloop task new`.\n");
    }
    if !b.unfolded_blockers.is_empty() || !b.unfolded_reports.is_empty() {
        out.push_str(&s.yellow(&format!(
            "{} blockers and {} reports to fold in: `inloop board fold`\n",
            b.unfolded_blockers.len(),
            b.unfolded_reports.len()
        )));
    }
    if !b.newly_unblocked.is_empty() {
        let ids: Vec<String> = b.newly_unblocked.iter().map(|i| i.to_string()).collect();
        out.push_str(&s.green(&format!("newly unblocked: {}\n", ids.join(", "))));
    }
    for c in &b.corrupt {
        out.push_str(&s.red(&format!("corrupt record: {c}\n")));
    }
    out
}

fn task_text(s: &Style, t: &Task) -> String {
    let mut out = format!(
        "{}  {}\n{}",
        s.bold(t.id.as_str()),
        s.bold(&t.title),
        s.status(t.status.label())
    );
    for (k, v) in [
        ("branch", &t.branch),
        ("owner", &t.owner),
        ("session", &t.session),
        ("worktree", &t.worktree),
    ] {
        if let Some(v) = v {
            out.push_str(&format!(" · {k} {v}"));
        }
    }
    out.push('\n');
    if let Some(r) = &t.blocked_reason {
        out.push_str(&format!("{} {r}\n", s.magenta("blocked:")));
    }
    if !t.description.is_empty() {
        out.push_str(&format!(
            "\n{}\n",
            output::markdown(s, &t.description).trim_end()
        ));
    }
    if !t.touches.is_empty() {
        out.push_str(&format!(
            "\n{} {}\n",
            s.dim("touches"),
            t.touches.join(", ")
        ));
    }
    if !t.decisions.is_empty() {
        out.push_str(&format!(
            "{} {}\n",
            s.dim("decisions"),
            t.decisions.join(", ")
        ));
    }
    for n in &t.notes {
        out.push_str(&format!(
            "{} {}\n",
            s.dim(&format!(
                "{} {}:",
                n.at.chars().take(10).collect::<String>(),
                n.by
            )),
            n.text
        ));
    }
    out
}

pub fn task(ctx: &Ctx, cmd: TaskCmd) -> anyhow::Result<ExitCode> {
    let p = ctx.project()?;
    let who = p.git_user().unwrap_or_else(|| "human".into());
    match cmd {
        TaskCmd::New(a) => {
            let t = p.task_new(NewTask {
                title: a.title,
                description: a.description,
                touches: a.touches,
                blocked_by: ids(&a.blocked_by)?,
                owner: a.owner,
            })?;
            ctx.emit(&t, |s| {
                format!(
                    "{} added {}  {}\n",
                    s.green("✓"),
                    s.bold(t.id.as_str()),
                    t.title
                )
            })?;
        }
        TaskCmd::Show { ids } => {
            let (views, code) = crate::each_id(&ids, |id| Ok(p.task_get(id)?));
            ctx.emit_each(&views, |s, v| {
                let mut out = task_text(s, &v.task);
                for r in &v.reports {
                    out.push_str(&format!(
                        "\n{} {}: {}\n",
                        s.dim("report"),
                        r.at.chars().take(10).collect::<String>(),
                        r.summary
                    ));
                }
                for b in &v.blockers {
                    out.push_str(&format!(
                        "{} {}{}\n",
                        s.magenta("blocker"),
                        b.on,
                        if b.folded { "" } else { " (not folded)" }
                    ));
                }
                out
            })?;
            return Ok(code);
        }
        TaskCmd::Update(a) => {
            let status = match a.status {
                None => None,
                Some(s) => Some(s.parse::<TaskStatus>()?),
            };
            let update = TaskUpdate {
                status,
                title: a.title,
                description: a.description,
                touches: (!a.touches.is_empty()).then_some(a.touches),
                blocked_by: if a.blocked_by.is_empty() {
                    None
                } else {
                    Some(ids(&a.blocked_by)?)
                },
                blocked_reason: opt(a.reason),
                branch: opt(a.branch),
                worktree: opt(a.worktree),
                session: opt(a.session),
                owner: opt(a.owner),
                add_note: a.note,
                add_decisions: a.decisions,
            };
            // Every ID must name a task before any of them changes.
            let missing: Vec<&str> = a
                .ids
                .iter()
                .filter(|id| p.task_get(id).is_err())
                .map(String::as_str)
                .collect();
            if !missing.is_empty() {
                anyhow::bail!("no task {}; nothing changed", missing.join(", "));
            }
            let (tasks, code) = crate::each_id(&a.ids, |id| {
                Ok(p.task_update(id, update.clone(), &who)?.task)
            });
            ctx.emit_each(&tasks, |s, t| {
                format!(
                    "{} {} is {}\n",
                    s.green("✓"),
                    t.id,
                    s.status(t.status.label())
                )
            })?;
            return Ok(code);
        }
        TaskCmd::Brief { ids } => {
            let (briefs, code) = crate::each_id(&ids, |id| Ok(p.task_brief(id)?));
            if ctx.json {
                match briefs.as_slice() {
                    [one] => println!("{}", serde_json::to_string_pretty(one)?),
                    all => println!("{}", serde_json::to_string_pretty(all)?),
                }
            } else {
                for (i, b) in briefs.iter().enumerate() {
                    if i > 0 {
                        println!("\n---\n");
                    }
                    print!("{}", b.text);
                    if let Some(w) = b.warning() {
                        eprintln!("{} {w}", ctx.style.yellow("warning:"));
                    }
                }
            }
            return Ok(code);
        }
        TaskCmd::Check { ids } => {
            let r = p.board_check(&ids)?;
            ctx.emit(&r, |s| {
                if r.ok {
                    format!("{} no conflicts\n", s.green("✓"))
                } else {
                    r.to_text()
                }
            })?;
            return Ok(if r.ok {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            });
        }
        TaskCmd::Report(a) => {
            let task = p.child_task(a.task.as_deref())?;
            let mut checks: Vec<CheckResult> = a
                .passed
                .into_iter()
                .map(|c| CheckResult {
                    command: c,
                    passed: true,
                    note: None,
                })
                .collect();
            checks.extend(a.failed.into_iter().map(|c| CheckResult {
                command: c,
                passed: false,
                note: None,
            }));
            let r = p.task_report(NewReport {
                task,
                session: p.current_session(),
                done: a.done,
                not_done: a.not_done,
                checks,
                decisions: a.decisions,
                plan_changes: a.plan_changes,
                summary: a.summary,
            })?;
            ctx.emit(&r, |s| format!("{} reported on {}\n", s.green("✓"), r.task))?;
        }
        TaskCmd::Blocked(a) => {
            let task = p.child_task(a.task.as_deref())?;
            let kind = match a.kind.as_str() {
                "external" => BlockerKind::External,
                "task" => BlockerKind::Task,
                "human" => BlockerKind::Human,
                other => anyhow::bail!("kind `{other}`: use external, task or human"),
            };
            let b = p.task_blocked(NewBlocker {
                task,
                session: p.current_session(),
                kind,
                on: a.on,
                details: a.details,
            })?;
            ctx.emit(&b, |s| {
                format!("{} {} marked blocked on {}\n", s.yellow("✓"), b.task, b.on)
            })?;
        }
    }
    Ok(ExitCode::SUCCESS)
}
