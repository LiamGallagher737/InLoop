//! The optional project manager layer (docs/05), off unless `[pm] enabled`.
//! Only the PM writes tasks (D-Y40K); children report through reports and
//! blockers, which the PM folds in.

use inloop_core::Id;
use inloop_index::Kind;
use inloop_pm::{
    Blocker, Board, BriefContext, BriefDecision, CheckReport, Claim, DEFAULT_BRIEF, NewBlocker,
    NewReport, NewTask, RecordKind, RenderedBrief, Report, Task, TaskStatus, TaskUpdate,
    UpdateContext, UpdateOutcome,
};
use serde::Serialize;

use crate::project::{AppError, Project};
use crate::scan::ScanFinding;

#[derive(Clone, Debug, Serialize)]
pub struct BoardView {
    pub tasks: Vec<Task>,
    /// Blockers children reported that the PM hasn't folded in yet.
    pub unfolded_blockers: Vec<Blocker>,
    /// Reports the PM hasn't folded in yet.
    pub unfolded_reports: Vec<Report>,
    /// Blocked tasks whose blockers are all merged: ready to propose.
    pub newly_unblocked: Vec<Id>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub corrupt: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TaskView {
    #[serde(flatten)]
    pub task: Task,
    pub claims: Vec<Claim>,
    pub reports: Vec<Report>,
    pub blockers: Vec<Blocker>,
}

#[derive(Clone, Debug, Serialize)]
pub struct FoldOutcome {
    pub blockers_folded: Vec<Id>,
    pub reports_folded: Vec<Id>,
    pub tasks_changed: Vec<Id>,
}

fn err<E: std::fmt::Display>(e: E) -> AppError {
    AppError::Other(e.to_string())
}

impl Project {
    pub fn pm_enabled(&self) -> bool {
        self.config.pm.enabled
    }

    fn require_pm(&self) -> Result<(), AppError> {
        if self.pm_enabled() {
            Ok(())
        } else {
            Err(AppError::Invalid(
                "the project manager layer is off for this project; set `[pm] enabled = true` in inloop.toml to use it".into(),
            ))
        }
    }

    /// The board, in the git common dir so every worktree shares it (D-SRGY).
    pub fn board(&self) -> Board {
        Board::open(self.state_dir().join("board"))
    }

    /// True when running in the main worktree: where the PM works.
    pub fn in_main_worktree(&self) -> bool {
        let here = self
            .repo
            .root
            .canonicalize()
            .unwrap_or_else(|_| self.repo.root.clone());
        self.repo
            .worktrees()
            .ok()
            .and_then(|w| w.into_iter().find(|w| w.is_main))
            .map(|w| w.path.canonicalize().unwrap_or(w.path) == here)
            .unwrap_or(true)
    }

    /// The task whose branch is `branch`, when the PM layer is on.
    pub fn task_for_branch_impl(&self, branch: Option<&str>) -> Option<String> {
        if !self.pm_enabled() {
            return None;
        }
        let branch = branch?;
        let tasks = self.board().list_tasks().ok()?.records;
        tasks
            .into_iter()
            .filter(|t| t.status != TaskStatus::Merged)
            .find(|t| t.branch.as_deref() == Some(branch))
            .map(|t| t.id.to_string())
    }

    fn tasks(&self) -> Result<Vec<Task>, AppError> {
        Ok(self.board().list_tasks().map_err(err)?.records)
    }

    fn parse_task_id(&self, id: &str) -> Result<Id, AppError> {
        inloop_core::IdSpec::tasks()
            .parse(id)
            .map_err(|e| AppError::Invalid(e.to_string()))
    }

    fn get_task(&self, id: &str) -> Result<Task, AppError> {
        let id = self.parse_task_id(id)?;
        self.board()
            .get_task(&id)
            .map_err(err)?
            .ok_or_else(|| AppError::NotFound(format!("task {id}")))
    }

    fn put_task(&self, t: Task) -> Result<Task, AppError> {
        let r = self
            .redactor()?
            .redact(t)
            .map_err(|e| AppError::Redactor(e.to_string()))?;
        let t = r.get().clone();
        self.board().put_task(r).map_err(err)?;
        Ok(t)
    }

    pub fn board_list(&self) -> Result<BoardView, AppError> {
        self.require_pm()?;
        let board = self.board();
        let listing = board.list_tasks().map_err(err)?;
        let tasks = listing.records;
        let unfolded_blockers = board.unfolded_blockers().map_err(err)?;
        let reports = board.list_reports().map_err(err)?.records;
        let unfolded_reports = reports.into_iter().filter(|r| !r.folded).collect();
        Ok(BoardView {
            newly_unblocked: inloop_pm::newly_unblocked(&tasks),
            corrupt: listing
                .corrupt
                .iter()
                .map(|c| format!("{}: {}", c.path.display(), c.message))
                .collect(),
            tasks,
            unfolded_blockers,
            unfolded_reports,
        })
    }

    /// Fail, naming them, if any of `ids` names no task. Batch commands call
    /// this first so a typo changes nothing.
    pub fn tasks_exist(&self, ids: &[String]) -> Result<(), AppError> {
        self.require_pm()?;
        let missing: Vec<&str> = ids
            .iter()
            .filter(|id| self.get_task(id).is_err())
            .map(String::as_str)
            .collect();
        if missing.is_empty() {
            Ok(())
        } else {
            Err(AppError::NotFound(format!(
                "task {}; nothing changed",
                missing.join(", ")
            )))
        }
    }

    pub fn task_get(&self, id: &str) -> Result<TaskView, AppError> {
        self.require_pm()?;
        let task = self.get_task(id)?;
        let board = self.board();
        Ok(TaskView {
            claims: board.claims_for_task(&task.id).map_err(err)?,
            reports: board.reports_for_task(&task.id).map_err(err)?,
            blockers: board.blockers_for_task(&task.id).map_err(err)?,
            task,
        })
    }

    pub fn task_new(&self, new: NewTask) -> Result<Task, AppError> {
        self.require_pm()?;
        let tasks = self.tasks()?;
        let lookup = |id: &Id| tasks.iter().find(|t| &t.id == id).map(|t| t.status);
        let id = self.board().new_id(RecordKind::Task).map_err(err)?;
        let t = inloop_pm::new_task(new, id, inloop_core::now(), &lookup).map_err(err)?;
        self.put_task(t)
    }

    pub fn task_update(
        &self,
        id: &str,
        update: TaskUpdate,
        actor: &str,
    ) -> Result<UpdateOutcome, AppError> {
        self.require_pm()?;
        let task = self.get_task(id)?;
        let mut update = update;
        update.add_decisions = update
            .add_decisions
            .iter()
            .map(|d| self.decision_ref(d))
            .collect();
        let tasks = self.tasks()?;
        let board = self.board();
        let claims = board.claims_for_task(&task.id).map_err(err)?;
        let lookup = |id: &Id| tasks.iter().find(|t| &t.id == id).map(|t| t.status);
        let ctx = UpdateContext {
            now: inloop_core::now(),
            actor,
            claims: &claims,
            lookup: &lookup,
            new_claim_id: board.new_id(RecordKind::Claim).map_err(err)?,
        };
        let out = inloop_pm::update_task(task, update, &ctx).map_err(err)?;
        let r = self.redactor()?;
        for c in out.changed_claims() {
            let c = r
                .redact(c.clone())
                .map_err(|e| AppError::Redactor(e.to_string()))?;
            board.put_claim(c).map_err(err)?;
        }
        let task = self.put_task(out.task.clone())?;
        Ok(UpdateOutcome { task, ..out })
    }

    /// Check the given tasks (plus every running one) for conflicts.
    pub fn board_check(&self, ids: &[String]) -> Result<CheckReport, AppError> {
        self.require_pm()?;
        let tasks = self.tasks()?;
        let mut proposed = Vec::new();
        for i in ids {
            let id = self.parse_task_id(i)?;
            proposed.push(
                tasks
                    .iter()
                    .find(|t| t.id == id)
                    .cloned()
                    .ok_or_else(|| AppError::NotFound(format!("task {id}")))?,
            );
        }
        if proposed.is_empty() {
            proposed = tasks
                .iter()
                .filter(|t| t.status == TaskStatus::Ready)
                .cloned()
                .collect();
        }
        let files: Vec<String> = self
            .repo
            .run(&["ls-files", "-z"])?
            .split('\0')
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
        inloop_pm::board_check(&proposed, &tasks, &files, &self.config.pm.conflict_groups)
            .map_err(err)
    }

    /// The generated brief for a child session.
    pub fn task_brief(&self, id: &str) -> Result<RenderedBrief, AppError> {
        self.require_pm()?;
        let task = self.get_task(id)?;
        let template_path = self.repo.root.join(&self.config.pm.brief_template);
        let template =
            std::fs::read_to_string(&template_path).unwrap_or_else(|_| DEFAULT_BRIEF.to_string());
        // Relevant decisions: search on the task's title and description.
        let query = format!("{} {}", task.title, task.description);
        let mut decisions: Vec<BriefDecision> = Vec::new();
        for h in self
            .search(&query, &[Kind::Decision], 8)
            .unwrap_or_default()
        {
            let status = h
                .meta
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if status == "superseded" || status == "rejected" {
                continue;
            }
            let merged = h
                .meta
                .get("provenance")
                .and_then(|p| p.get("state"))
                .and_then(|s| s.as_str())
                == Some("merged");
            decisions.push(BriefDecision {
                id: h.id,
                title: h.title,
                status,
                merged,
            });
        }
        for d in &task.decisions {
            if !decisions.iter().any(|x| &x.id == d)
                && let Ok(e) = self.decision_get(d)
            {
                decisions.push(BriefDecision {
                    id: e.decision.id.to_string(),
                    title: e.decision.title.clone(),
                    status: e.decision.status.to_string(),
                    merged: e.provenance.is_merged(),
                });
            }
        }
        Ok(inloop_pm::render_brief(
            &template,
            &BriefContext {
                project: &self.name,
                task: &task,
                checks: &self.config.checks.commands,
                decisions: &decisions,
            },
        ))
    }

    /// The task a child is working on: given, or the one owning this branch.
    pub fn child_task(&self, id: Option<&str>) -> Result<Id, AppError> {
        if let Some(id) = id {
            return Ok(self.get_task(id)?.id);
        }
        let branch = self.repo.current_branch()?;
        let id = self
            .task_for_branch_impl(branch.as_deref())
            .ok_or_else(|| {
                AppError::Invalid(format!(
                    "no task owns branch {}; pass the task ID",
                    branch.as_deref().unwrap_or("(detached)")
                ))
            })?;
        self.parse_task_id(&id)
    }

    /// A child's structured end-of-work report. Doesn't change the task:
    /// the PM folds it in.
    pub fn task_report(&self, mut new: NewReport) -> Result<Report, AppError> {
        self.require_pm()?;
        new.decisions = new.decisions.iter().map(|d| self.decision_ref(d)).collect();
        self.get_task(new.task.as_str())?;
        let id = self.board().new_id(RecordKind::Report).map_err(err)?;
        let rep = inloop_pm::new_report(new, id, inloop_core::now());
        let r = self
            .redactor()?
            .redact(rep)
            .map_err(|e| AppError::Redactor(e.to_string()))?;
        let rep = r.get().clone();
        self.board().put_report(r).map_err(err)?;
        Ok(rep)
    }

    /// A child reporting it's blocked. The PM folds it in.
    pub fn task_blocked(&self, new: NewBlocker) -> Result<Blocker, AppError> {
        self.require_pm()?;
        self.get_task(new.task.as_str())?;
        let id = self.board().new_id(RecordKind::Blocker).map_err(err)?;
        let b = inloop_pm::new_blocker(new, id, inloop_core::now());
        let r = self
            .redactor()?
            .redact(b)
            .map_err(|e| AppError::Redactor(e.to_string()))?;
        let b = r.get().clone();
        self.board().put_blocker(r).map_err(err)?;
        Ok(b)
    }

    /// Fold children's blockers and reports into the board (the PM's job).
    pub fn board_fold(&self) -> Result<FoldOutcome, AppError> {
        self.require_pm()?;
        let board = self.board();
        let r = self.redactor()?;
        let mut out = FoldOutcome {
            blockers_folded: vec![],
            reports_folded: vec![],
            tasks_changed: vec![],
        };
        for b in board.unfolded_blockers().map_err(err)? {
            let Some(task) = board.get_task(&b.task).map_err(err)? else {
                continue;
            };
            let Ok((task, b)) = inloop_pm::apply_blocker(task, b, inloop_core::now()) else {
                continue;
            };
            out.blockers_folded.push(b.id.clone());
            out.tasks_changed.push(task.id.clone());
            board
                .put_blocker(r.redact(b).map_err(|e| AppError::Redactor(e.to_string()))?)
                .map_err(err)?;
            self.put_task(task)?;
        }
        for rep in board.list_reports().map_err(err)?.records {
            let Some(task) = board.get_task(&rep.task).map_err(err)? else {
                continue;
            };
            if rep.folded {
                continue;
            }
            let Ok(task) = inloop_pm::apply_report(task, &rep, inloop_core::now()) else {
                continue;
            };
            let mut rep = rep;
            rep.folded = true;
            board
                .put_report(
                    r.redact(rep.clone())
                        .map_err(|e| AppError::Redactor(e.to_string()))?,
                )
                .map_err(err)?;
            out.reports_folded.push(rep.id.clone());
            out.tasks_changed.push(task.id.clone());
            self.put_task(task)?;
        }
        out.tasks_changed.sort();
        out.tasks_changed.dedup();
        Ok(out)
    }

    pub fn board_export(&self) -> Result<String, AppError> {
        self.require_pm()?;
        let tasks = self.tasks()?;
        let blockers = self.board().list_blockers().map_err(err)?.records;
        Ok(inloop_pm::export_markdown(
            &format!("{} board", self.name),
            &tasks,
            &blockers,
        ))
    }

    /// `inloop scan` over board records.
    pub fn scan_board_impl(
        &self,
        fix: bool,
        findings: &mut Vec<ScanFinding>,
    ) -> Result<usize, AppError> {
        if !self.pm_enabled() {
            return Ok(0);
        }
        let dir = self.state_dir().join("board");
        let r = self.redactor()?;
        let mut n = 0;
        for sub in ["tasks", "claims", "reports", "blockers"] {
            let Ok(rd) = std::fs::read_dir(dir.join(sub)) else {
                continue;
            };
            for e in rd.flatten() {
                let p = e.path();
                if p.extension().and_then(|x| x.to_str()) != Some("json") {
                    continue;
                }
                let Ok(text) = std::fs::read_to_string(&p) else {
                    continue;
                };
                n += 1;
                let found = r.find(&text);
                if found.is_empty() {
                    continue;
                }
                let mut kinds = std::collections::BTreeMap::new();
                for f in &found {
                    *kinds.entry(f.kind.clone()).or_insert(0) += 1;
                }
                // Board records are InLoop's own: always fixed, as JSON.
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                    let v = r.redact(v).map_err(|e| AppError::Redactor(e.to_string()))?;
                    let out = serde_json::to_string_pretty(v.get()).map_err(err)?;
                    std::fs::write(&p, out)
                        .map_err(|e| AppError::io("rewriting a board record", e))?;
                }
                let _ = fix;
                findings.push(ScanFinding {
                    what: format!("board record {}", p.display()),
                    kinds,
                    fixed: true,
                });
            }
        }
        Ok(n)
    }
}
