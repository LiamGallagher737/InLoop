//! Child briefs (`task_brief`, docs/05): a per-project template filled in
//! with the task, the project's checks and the decisions relevant to it.
//!
//! Placeholders are `{{name}}`, with optional spaces inside the braces:
//!
//! | Placeholder | Becomes |
//! |---|---|
//! | `{{project}}` | The project name |
//! | `{{task.id}}`, `{{task.title}}`, `{{task.description}}`, `{{task.branch}}` | Those task fields |
//! | `{{task.touches}}` | A bullet list of the paths the task may change |
//! | `{{checks}}` | A bullet list of the check commands |
//! | `{{decisions}}` | A bullet list of decisions; unmerged ones are marked as not settled |
//! | `{{report_format}}` | How to report with `task_report`, and `task_blocked` when stuck |
//!
//! An unknown placeholder is left as it is and listed in
//! [`RenderedBrief::unknown`], so a typo shows up rather than vanishing.

use serde::Serialize;

use crate::model::Task;

/// The brief used when a project has no template of its own
/// (`[pm].brief_template`, by default `.inloop/brief.md`).
pub const DEFAULT_BRIEF: &str = r#"# {{task.id}}: {{task.title}}

You are working on one task for {{project}}, in your own worktree on branch `{{task.branch}}`. Another agent (the project manager) reviews your work, and a human decides when it lands.

## The task

{{task.description}}

## Before you start

- Read `CLAUDE.md` and the docs it points to for the areas you will change.
- Search the project's decisions (`decision_search`) for anything that bears on this task, and follow them.

## Paths you may change

{{task.touches}}

Stay inside these paths. If the task needs changes elsewhere, stop and report it with `task_blocked` rather than spreading out; another task may own those files.

## Decisions relevant to this task

{{decisions}}

## While you work

- Record design choices with `decision_new`. Record one as `accepted` only when a human made it, quoting them in `basis`; anything you chose yourself is `proposed`.
- Keep commits small and on `{{task.branch}}`. Don't push, merge or rebase onto main; the project manager handles landing once the human agrees.

## Checks

Run these before you report, and fix what fails:

{{checks}}

## When you finish

{{report_format}}
"#;

/// The text `{{report_format}}` expands to.
pub const REPORT_FORMAT: &str = "When you are done, call `task_report` once with: `done` (what is finished), `not_done` (anything left or skipped), `checks` (each command, whether it passed, and a note if not), `decisions` (IDs you recorded), `plan_changes` (anything the project manager should change in the plan) and a short `summary`. Be plain about what doesn't work.\n\nIf you get stuck on something you can't resolve yourself (another task, an outside service, or a question only a human can answer), call `task_blocked` with the kind, what you are blocked on and the details, then stop and wait.";

/// A decision as the brief shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct BriefDecision {
    pub id: String,
    pub title: String,
    pub status: String,
    /// On the main branch. An unmerged decision is marked as not settled.
    pub merged: bool,
}

/// Everything a brief is rendered from.
#[derive(Clone, Copy, Debug)]
pub struct BriefContext<'a> {
    pub project: &'a str,
    pub task: &'a Task,
    pub checks: &'a [String],
    pub decisions: &'a [BriefDecision],
}

/// A rendered brief, and any placeholders the template used that aren't
/// known (left in the text as they were).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RenderedBrief {
    pub text: String,
    pub unknown: Vec<String>,
}

impl RenderedBrief {
    /// A warning for people, or `None` when every placeholder was known.
    pub fn warning(&self) -> Option<String> {
        if self.unknown.is_empty() {
            return None;
        }
        let list: Vec<String> = self
            .unknown
            .iter()
            .map(|u| format!("{{{{{u}}}}}"))
            .collect();
        Some(format!(
            "the brief template has unknown placeholders, left as they are: {}",
            list.join(", ")
        ))
    }
}

fn bullets(items: &[String], empty: &str) -> String {
    if items.is_empty() {
        return empty.to_string();
    }
    items
        .iter()
        .map(|i| format!("- `{i}`"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn decision_bullets(decisions: &[BriefDecision]) -> String {
    if decisions.is_empty() {
        return "None recorded yet.".to_string();
    }
    decisions
        .iter()
        .map(|d| {
            let settled = if d.merged {
                String::new()
            } else {
                " (not merged yet, so not settled: it may still change)".to_string()
            };
            format!("- {} {} [{}]{settled}", d.id, d.title, d.status)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn value(name: &str, ctx: &BriefContext) -> Option<String> {
    let t = ctx.task;
    Some(match name {
        "project" => ctx.project.to_string(),
        "task.id" => t.id.to_string(),
        "task.title" => t.title.clone(),
        "task.description" => {
            if t.description.trim().is_empty() {
                "(No description. Ask the project manager if the title isn't enough.)".to_string()
            } else {
                t.description.trim_end().to_string()
            }
        }
        "task.branch" => t.branch.clone().unwrap_or_else(|| "(not set)".to_string()),
        "task.touches" => bullets(
            &t.touches,
            "No paths listed. Ask the project manager before changing files.",
        ),
        "checks" => bullets(ctx.checks, "No checks are configured for this project."),
        "decisions" => decision_bullets(ctx.decisions),
        "report_format" => REPORT_FORMAT.to_string(),
        _ => return None,
    })
}

/// Fill `template` in from `ctx`. Values are inserted as they are and are
/// not scanned for further placeholders.
pub fn render_brief(template: &str, ctx: &BriefContext) -> RenderedBrief {
    let mut out = String::with_capacity(template.len() + 1024);
    let mut unknown: Vec<String> = Vec::new();
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            out.push_str(&rest[start..]);
            rest = "";
            break;
        };
        let name = after[..end].trim();
        match value(name, ctx) {
            Some(v) => out.push_str(&v),
            None => {
                out.push_str(&rest[start..start + 2 + end + 2]);
                if !unknown.iter().any(|u| u == name) {
                    unknown.push(name.to_string());
                }
            }
        }
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    RenderedBrief { text: out, unknown }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::TaskStatus;
    use inloop_core::Id;

    fn task() -> Task {
        Task {
            id: Id::parse_loose("T-4M1P").unwrap_or_else(|_| unreachable!()),
            title: "Add full text search".into(),
            description: "Index decision bodies with FTS5 and expose `search`.\n".into(),
            status: TaskStatus::Running,
            blocked_by: vec![],
            blocked_reason: None,
            touches: vec!["crates/index/**".into(), "docs/06-cli-and-mcp.md".into()],
            branch: Some("task/search".into()),
            worktree: Some("../example-search".into()),
            session: None,
            owner: Some("sam".into()),
            decisions: vec![],
            notes: vec![],
            created: String::new(),
            updated: String::new(),
        }
    }

    fn decisions() -> Vec<BriefDecision> {
        vec![
            BriefDecision {
                id: "D-7KQ2".into(),
                title: "Use SQLite FTS5 for search".into(),
                status: "accepted".into(),
                merged: true,
            },
            BriefDecision {
                id: "D-9XYZ".into(),
                title: "Rank by recency".into(),
                status: "proposed".into(),
                merged: false,
            },
        ]
    }

    #[test]
    fn default_brief_renders() {
        let t = task();
        let checks = vec![
            "cargo test --workspace".to_string(),
            "cargo fmt --check".to_string(),
        ];
        let d = decisions();
        let r = render_brief(
            DEFAULT_BRIEF,
            &BriefContext {
                project: "example",
                task: &t,
                checks: &checks,
                decisions: &d,
            },
        );
        assert!(r.unknown.is_empty(), "{:?}", r.unknown);
        assert_eq!(r.warning(), None);
        assert!(!r.text.contains("{{"));
        insta::assert_snapshot!(r.text);
    }

    #[test]
    fn empty_lists_get_plain_words() {
        let mut t = task();
        t.touches.clear();
        t.description.clear();
        t.branch = None;
        let r = render_brief(
            "{{task.touches}}|{{checks}}|{{decisions}}|{{task.description}}|{{task.branch}}",
            &BriefContext {
                project: "p",
                task: &t,
                checks: &[],
                decisions: &[],
            },
        );
        assert_eq!(
            r.text,
            "No paths listed. Ask the project manager before changing files.|No checks are configured for this project.|None recorded yet.|(No description. Ask the project manager if the title isn't enough.)|(not set)"
        );
    }

    #[test]
    fn unknown_placeholders_stay_and_are_listed() {
        let t = task();
        let r = render_brief(
            "{{ task.id }} {{task.owner}} {{nope}} {{task.owner}} {{ unclosed",
            &BriefContext {
                project: "p",
                task: &t,
                checks: &[],
                decisions: &[],
            },
        );
        assert_eq!(
            r.text,
            "T-4M1P {{task.owner}} {{nope}} {{task.owner}} {{ unclosed"
        );
        assert_eq!(r.unknown, ["task.owner", "nope"]);
        assert_eq!(
            r.warning().as_deref(),
            Some(
                "the brief template has unknown placeholders, left as they are: {{task.owner}}, {{nope}}"
            )
        );
    }

    #[test]
    fn values_are_not_expanded_again() {
        let mut t = task();
        t.title = "Literal {{checks}} in a title".into();
        let r = render_brief(
            "{{task.title}}",
            &BriefContext {
                project: "p",
                task: &t,
                checks: &["x".into()],
                decisions: &[],
            },
        );
        assert_eq!(r.text, "Literal {{checks}} in a title");
        assert!(r.unknown.is_empty());
    }

    #[test]
    fn default_brief_is_neutral() {
        for word in ["Adpatch", "Cirkit", "Fable", "Opus"] {
            assert!(!DEFAULT_BRIEF.contains(word), "{word}");
            assert!(!REPORT_FORMAT.contains(word), "{word}");
        }
        assert!(!DEFAULT_BRIEF.contains('\u{2014}'), "no em dashes");
    }
}
