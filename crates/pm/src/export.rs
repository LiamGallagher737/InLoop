//! `board export`: the board as Markdown for a human or co-founder to read.
//! Tasks are grouped by status in board order (ready, running, review, the
//! three blocked statuses, merged), one table per non-empty group.

use crate::model::{Blocker, Task, TaskStatus};

fn cell(text: &str) -> String {
    let one_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    one_line.replace('|', "\\|")
}

fn opt(text: &Option<String>) -> String {
    text.as_deref().map(cell).unwrap_or_default()
}

/// Render the board. `blockers` are matched to tasks by ID; unfolded ones
/// are listed under the task as well, since the board hasn't caught up
/// with them yet. `title` heads the document.
pub fn export_markdown(title: &str, tasks: &[Task], blockers: &[Blocker]) -> String {
    let mut out = format!("# {}\n", cell(title));
    if tasks.is_empty() {
        out.push_str("\nThe board is empty.\n");
        return out;
    }
    for status in TaskStatus::ALL {
        let mut group: Vec<&Task> = tasks.iter().filter(|t| t.status == status).collect();
        if group.is_empty() {
            continue;
        }
        group.sort_by(|a, b| a.id.cmp(&b.id));
        let label = status.label();
        let mut heading = label.to_string();
        if let Some(first) = heading.get_mut(0..1) {
            first.make_ascii_uppercase();
        }
        out.push_str(&format!("\n## {heading} ({})\n\n", group.len()));
        let blocked = status.is_blocked();
        if blocked {
            out.push_str("| ID | Title | Blocked on | Branch | Owner | Updated |\n");
            out.push_str("|---|---|---|---|---|---|\n");
        } else {
            out.push_str("| ID | Title | Branch | Owner | Touches | Updated |\n");
            out.push_str("|---|---|---|---|---|---|\n");
        }
        for t in &group {
            let updated = t.updated.get(..10).unwrap_or(&t.updated);
            if blocked {
                let mut on: Vec<String> = t.blocked_by.iter().map(|b| b.to_string()).collect();
                if let Some(r) = &t.blocked_reason {
                    on.push(cell(r));
                }
                for b in blockers.iter().filter(|b| b.task == t.id && !b.folded) {
                    on.push(format!("{} (not folded in yet): {}", b.id, cell(&b.on)));
                }
                out.push_str(&format!(
                    "| {} | {} | {} | {} | {} | {} |\n",
                    t.id,
                    cell(&t.title),
                    on.join("; "),
                    opt(&t.branch),
                    opt(&t.owner),
                    updated
                ));
            } else {
                let touches: Vec<String> =
                    t.touches.iter().map(|p| format!("`{}`", cell(p))).collect();
                out.push_str(&format!(
                    "| {} | {} | {} | {} | {} | {} |\n",
                    t.id,
                    cell(&t.title),
                    opt(&t.branch),
                    opt(&t.owner),
                    touches.join(", "),
                    updated
                ));
            }
        }
        // Unfolded blockers on tasks that aren't marked blocked yet.
        if !blocked {
            let pending: Vec<&Blocker> = blockers
                .iter()
                .filter(|b| !b.folded && group.iter().any(|t| t.id == b.task))
                .collect();
            if !pending.is_empty() {
                out.push('\n');
                for b in pending {
                    out.push_str(&format!(
                        "- {} reports it is blocked ({}, not folded in yet): {}\n",
                        b.task,
                        b.id,
                        cell(&b.on)
                    ));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::BlockerKind;
    use inloop_core::Id;

    fn id(s: &str) -> Id {
        Id::parse_loose(s).unwrap_or_else(|_| unreachable!())
    }

    fn task(i: &str, title: &str, status: TaskStatus) -> Task {
        Task {
            id: id(i),
            title: title.into(),
            description: String::new(),
            status,
            blocked_by: vec![],
            blocked_reason: None,
            touches: vec![],
            branch: None,
            worktree: None,
            session: None,
            owner: None,
            decisions: vec![],
            notes: vec![],
            created: "2026-11-14T01:00:00Z".into(),
            updated: "2026-11-14T01:00:00Z".into(),
        }
    }

    #[test]
    fn exports_grouped_tables() {
        let mut running = task("T-00B2", "Search | ranking", TaskStatus::Running);
        running.branch = Some("task/search".into());
        running.owner = Some("sam".into());
        running.touches = vec!["crates/index/**".into()];
        let mut waiting = task("T-00C3", "Billing page", TaskStatus::BlockedTask);
        waiting.blocked_by = vec![id("T-00B2")];
        let mut human = task("T-00D4", "Pricing copy", TaskStatus::BlockedHuman);
        human.blocked_reason = Some("Which currency\nto show first".into());
        let tasks = vec![
            task("T-00Z9", "Old work", TaskStatus::Merged),
            human,
            waiting,
            running,
            task("T-00A1", "Write the README", TaskStatus::Ready),
        ];
        let blockers = vec![
            Blocker {
                id: id("B-0001"),
                task: id("T-00B2"),
                at: "2026-11-14T02:00:00Z".into(),
                session: None,
                kind: BlockerKind::External,
                on: "the payment provider's sandbox".into(),
                details: String::new(),
                folded: false,
            },
            Blocker {
                id: id("B-0002"),
                task: id("T-00D4"),
                at: "2026-11-14T02:00:00Z".into(),
                session: None,
                kind: BlockerKind::Human,
                on: "folded already".into(),
                details: String::new(),
                folded: true,
            },
        ];
        insta::assert_snapshot!(export_markdown("Example board", &tasks, &blockers));
    }

    #[test]
    fn empty_board() {
        assert_eq!(
            export_markdown("B", &[], &[]),
            "# B\n\nThe board is empty.\n"
        );
    }
}
