//! The conflict check (`board_check`, docs/05): before the PM proposes a
//! batch, report every pair of tasks that would touch the same files, and
//! every conflict group (docs/07, `[pm].conflict_groups`) more than one of
//! them touches.
//!
//! A task's `touches` are globs relative to the repo root, matched against
//! the repo's file list: `*` stays within one path segment and `**` crosses
//! segments. An entry without wildcards that names a folder (`crates/core`
//! or `crates/core/`) means everything under it. A glob that matches no
//! existing file (a file the task will create, say) is compared
//! conservatively: two globs overlap when the literal part of one, before
//! its first wildcard, starts with the literal part of the other.

use std::collections::{BTreeMap, BTreeSet};

use globset::{GlobBuilder, GlobMatcher};
use inloop_core::Id;
use inloop_core::config::ConflictGroup;
use serde::Serialize;

use crate::model::{Task, TaskStatus};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CheckError {
    #[error("{owner} has an invalid path glob `{glob}`: {message}")]
    BadGlob {
        owner: String,
        glob: String,
        message: String,
    },
}

/// Why two tasks conflict.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "detail")]
pub enum ConflictReason {
    /// Existing files both tasks touch.
    SharedFiles(Vec<String>),
    /// A conflict group both tasks touch.
    Group(String),
    /// A glob of each task (at least one matching no file yet) that could
    /// cover the same paths: `(a's glob, b's glob)`.
    Overlap(String, String),
}

/// One conflict between two tasks; `a` sorts before `b`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct Conflict {
    pub a: Id,
    pub b: Id,
    pub reason: ConflictReason,
}

/// The result of a check. `ok` when there are no conflicts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CheckReport {
    pub ok: bool,
    /// The tasks that were checked: the proposed ones plus every running
    /// task, sorted by ID.
    pub checked: Vec<Id>,
    /// Sorted by `(a, b, reason)`.
    pub conflicts: Vec<Conflict>,
}

const WILDCARDS: &[char] = &['*', '?', '[', '{'];

/// One `touches` or group entry, ready to match.
struct Pattern {
    text: String,
    /// The literal part before the first wildcard, for the conservative
    /// comparison.
    prefix: String,
    matcher: GlobMatcher,
}

fn compile(owner: &str, raw: &str, files: &[String]) -> Result<Pattern, CheckError> {
    let trimmed = raw.trim().trim_start_matches("./");
    let has_wild = trimmed.contains(WILDCARDS);
    // A plain folder name means everything under it.
    let text = if !has_wild && trimmed.ends_with('/') {
        format!("{trimmed}**")
    } else if !has_wild
        && !files.iter().any(|f| f == trimmed)
        && files.iter().any(|f| f.starts_with(&format!("{trimmed}/")))
    {
        format!("{trimmed}/**")
    } else {
        trimmed.to_string()
    };
    let matcher = GlobBuilder::new(&text)
        .literal_separator(true)
        .build()
        .map_err(|e| CheckError::BadGlob {
            owner: owner.to_string(),
            glob: raw.to_string(),
            message: e.kind().to_string(),
        })?
        .compile_matcher();
    let prefix = match text.find(WILDCARDS) {
        Some(i) => text[..i].to_string(),
        None => text.clone(),
    };
    Ok(Pattern {
        text,
        prefix,
        matcher,
    })
}

/// Whether two patterns, at least one of which matches no file, could
/// cover the same path.
fn may_overlap(p: &Pattern, q: &Pattern) -> bool {
    let p_lit = !p.text.contains(WILDCARDS);
    let q_lit = !q.text.contains(WILDCARDS);
    match (p_lit, q_lit) {
        (true, true) => p.text == q.text,
        (true, false) => q.matcher.is_match(&p.text) || p.text.starts_with(&q.prefix),
        (false, true) => p.matcher.is_match(&q.text) || q.text.starts_with(&p.prefix),
        (false, false) => p.prefix.starts_with(&q.prefix) || q.prefix.starts_with(&p.prefix),
    }
}

struct Compiled {
    id: Id,
    patterns: Vec<Pattern>,
    /// Existing files any pattern matches.
    files: BTreeSet<String>,
    /// Indexes of patterns that match no existing file.
    unmatched: Vec<usize>,
}

/// Check `proposed` tasks against each other and against every `running`
/// task in `board`. Only pairs that include a proposed task are reported:
/// conflicts between tasks already running are not news. A proposed task
/// that is also running is checked once. `files` is the repo's file list
/// (`git ls-files`), paths relative to the root with `/` separators.
pub fn board_check(
    proposed: &[Task],
    board: &[Task],
    files: &[String],
    groups: &[ConflictGroup],
) -> Result<CheckReport, CheckError> {
    let mut tasks: BTreeMap<Id, (&Task, bool)> = BTreeMap::new();
    for t in board.iter().filter(|t| t.status == TaskStatus::Running) {
        tasks.insert(t.id.clone(), (t, false));
    }
    for t in proposed {
        tasks.insert(t.id.clone(), (t, true));
    }

    let mut compiled = Vec::with_capacity(tasks.len());
    for (id, (task, is_proposed)) in &tasks {
        let patterns: Vec<Pattern> = task
            .touches
            .iter()
            .filter(|g| !g.trim().is_empty())
            .map(|g| compile(id.as_str(), g, files))
            .collect::<Result<_, _>>()?;
        let mut matched = BTreeSet::new();
        let mut unmatched = Vec::new();
        for (i, p) in patterns.iter().enumerate() {
            let hits: Vec<&String> = files
                .iter()
                .filter(|f| p.matcher.is_match(f.as_str()))
                .collect();
            if hits.is_empty() {
                unmatched.push(i);
            }
            matched.extend(hits.into_iter().cloned());
        }
        compiled.push((
            Compiled {
                id: id.clone(),
                patterns,
                files: matched,
                unmatched,
            },
            *is_proposed,
        ));
    }

    let mut group_patterns = Vec::with_capacity(groups.len());
    for g in groups {
        let pats: Vec<Pattern> = g
            .paths
            .iter()
            .map(|p| compile(&format!("conflict group `{}`", g.name), p, files))
            .collect::<Result<_, _>>()?;
        group_patterns.push((g.name.clone(), pats));
    }

    // Which groups each task touches.
    let touches_group = |c: &Compiled, pats: &[Pattern]| -> bool {
        c.files
            .iter()
            .any(|f| pats.iter().any(|p| p.matcher.is_match(f.as_str())))
            || c.unmatched
                .iter()
                .any(|&i| pats.iter().any(|gp| may_overlap(&c.patterns[i], gp)))
    };
    let task_groups: Vec<BTreeSet<&str>> = compiled
        .iter()
        .map(|(c, _)| {
            group_patterns
                .iter()
                .filter(|(_, pats)| touches_group(c, pats))
                .map(|(name, _)| name.as_str())
                .collect()
        })
        .collect();

    let mut conflicts = BTreeSet::new();
    for i in 0..compiled.len() {
        for j in (i + 1)..compiled.len() {
            let (a, a_prop) = &compiled[i];
            let (b, b_prop) = &compiled[j];
            if !a_prop && !b_prop {
                continue;
            }
            let shared: Vec<String> = a.files.intersection(&b.files).cloned().collect();
            if !shared.is_empty() {
                conflicts.insert(Conflict {
                    a: a.id.clone(),
                    b: b.id.clone(),
                    reason: ConflictReason::SharedFiles(shared),
                });
            }
            for &ai in &a.unmatched {
                for bp in &b.patterns {
                    if may_overlap(&a.patterns[ai], bp) {
                        conflicts.insert(Conflict {
                            a: a.id.clone(),
                            b: b.id.clone(),
                            reason: ConflictReason::Overlap(
                                a.patterns[ai].text.clone(),
                                bp.text.clone(),
                            ),
                        });
                    }
                }
            }
            for &bi in &b.unmatched {
                for ap in &a.patterns {
                    if may_overlap(ap, &b.patterns[bi]) {
                        conflicts.insert(Conflict {
                            a: a.id.clone(),
                            b: b.id.clone(),
                            reason: ConflictReason::Overlap(
                                ap.text.clone(),
                                b.patterns[bi].text.clone(),
                            ),
                        });
                    }
                }
            }
            for g in task_groups[i].intersection(&task_groups[j]) {
                conflicts.insert(Conflict {
                    a: a.id.clone(),
                    b: b.id.clone(),
                    reason: ConflictReason::Group((*g).to_string()),
                });
            }
        }
    }

    let conflicts: Vec<Conflict> = conflicts.into_iter().collect();
    Ok(CheckReport {
        ok: conflicts.is_empty(),
        checked: tasks.into_keys().collect(),
        conflicts,
    })
}

impl CheckReport {
    /// One line per conflict, for people.
    pub fn to_text(&self) -> String {
        if self.ok {
            return format!("No conflicts among {} task(s).\n", self.checked.len());
        }
        let mut out = String::new();
        for c in &self.conflicts {
            let why = match &c.reason {
                ConflictReason::SharedFiles(files) => format!("both touch {}", files.join(", ")),
                ConflictReason::Group(g) => format!("both touch conflict group `{g}`"),
                ConflictReason::Overlap(x, y) => format!("`{x}` and `{y}` may overlap"),
            };
            out.push_str(&format!("{} and {}: {why}\n", c.a, c.b));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(s: &str) -> Id {
        Id::parse_loose(s).unwrap_or_else(|_| unreachable!("bad id {s}"))
    }

    fn task(i: &str, status: TaskStatus, touches: &[&str]) -> Task {
        Task {
            id: id(i),
            title: i.into(),
            description: String::new(),
            status,
            blocked_by: vec![],
            blocked_reason: None,
            touches: touches.iter().map(|s| s.to_string()).collect(),
            branch: None,
            worktree: None,
            session: None,
            owner: None,
            decisions: vec![],
            notes: vec![],
            created: String::new(),
            updated: String::new(),
        }
    }

    fn files() -> Vec<String> {
        [
            "Cargo.toml",
            "Cargo.lock",
            "README.md",
            "crates/core/Cargo.toml",
            "crates/core/src/lib.rs",
            "crates/core/src/id.rs",
            "crates/core/src/config.rs",
            "crates/index/src/lib.rs",
            "crates/index/src/fts.rs",
            "crates/cli/src/main.rs",
            "crates/cli/src/commands/board.rs",
            "docs/05-pm-layer.md",
            "docs/06-cli-and-mcp.md",
        ]
        .into_iter()
        .map(String::from)
        .collect()
    }

    fn groups() -> Vec<ConflictGroup> {
        vec![
            ConflictGroup {
                name: "manifests".into(),
                paths: vec!["Cargo.toml".into(), "Cargo.lock".into()],
            },
            ConflictGroup {
                name: "core".into(),
                paths: vec!["crates/core/**".into()],
            },
        ]
    }

    fn check(proposed: &[Task], board: &[Task]) -> CheckReport {
        board_check(proposed, board, &files(), &groups()).unwrap_or_else(|e| unreachable!("{e}"))
    }

    #[test]
    fn disjoint_tasks_are_ok() {
        let a = task("T-000A", TaskStatus::Ready, &["crates/index/**"]);
        let b = task(
            "T-000B",
            TaskStatus::Ready,
            &["crates/cli/**", "docs/06-cli-and-mcp.md"],
        );
        let r = check(&[a, b], &[]);
        assert!(r.ok, "{:?}", r.conflicts);
        assert_eq!(r.checked, [id("T-000A"), id("T-000B")]);
        assert_eq!(r.to_text(), "No conflicts among 2 task(s).\n");
    }

    #[test]
    fn shared_files_are_listed() {
        let a = task(
            "T-000A",
            TaskStatus::Ready,
            &["crates/index/**", "docs/*.md"],
        );
        let b = task(
            "T-000B",
            TaskStatus::Ready,
            &["crates/index/src/fts.rs", "docs/05-pm-layer.md"],
        );
        let r = check(&[b, a], &[]);
        assert!(!r.ok);
        assert_eq!(
            r.conflicts,
            [Conflict {
                a: id("T-000A"),
                b: id("T-000B"),
                reason: ConflictReason::SharedFiles(vec![
                    "crates/index/src/fts.rs".into(),
                    "docs/05-pm-layer.md".into(),
                ]),
            }]
        );
        assert_eq!(
            r.to_text(),
            "T-000A and T-000B: both touch crates/index/src/fts.rs, docs/05-pm-layer.md\n"
        );
    }

    #[test]
    fn single_star_stays_in_one_segment() {
        // `crates/*` matches no file (every file is deeper), so this falls
        // back to the conservative prefix check, which flags it.
        let a = task("T-000A", TaskStatus::Ready, &["crates/*.rs"]);
        let b = task("T-000B", TaskStatus::Ready, &["crates/index/src/lib.rs"]);
        let r = check(&[a, b], &[]);
        assert_eq!(
            r.conflicts
                .iter()
                .map(|c| c.reason.clone())
                .collect::<Vec<_>>(),
            [ConflictReason::Overlap(
                "crates/*.rs".into(),
                "crates/index/src/lib.rs".into()
            )]
        );
        // But `docs/*.md` matches files and doesn't reach into crates.
        let a = task("T-000A", TaskStatus::Ready, &["docs/*.md"]);
        let b = task("T-000B", TaskStatus::Ready, &["crates/index/src/lib.rs"]);
        assert!(check(&[a, b], &[]).ok);
    }

    #[test]
    fn double_star_and_directories() {
        let a = task("T-000A", TaskStatus::Ready, &["**/lib.rs"]);
        let b = task("T-000B", TaskStatus::Ready, &["crates/index"]);
        let c = task("T-000C", TaskStatus::Ready, &["crates/cli/"]);
        let r = check(&[a, b, c], &[]);
        assert_eq!(
            r.conflicts,
            [Conflict {
                a: id("T-000A"),
                b: id("T-000B"),
                reason: ConflictReason::SharedFiles(vec!["crates/index/src/lib.rs".into()]),
            }]
        );
    }

    #[test]
    fn groups_are_reported_per_pair() {
        let a = task(
            "T-000A",
            TaskStatus::Ready,
            &["crates/core/src/id.rs", "Cargo.toml"],
        );
        let b = task(
            "T-000B",
            TaskStatus::Ready,
            &["crates/core/src/config.rs", "Cargo.lock"],
        );
        let r = check(&[a, b], &[]);
        assert_eq!(
            r.conflicts
                .iter()
                .map(|c| c.reason.clone())
                .collect::<Vec<_>>(),
            [
                ConflictReason::Group("core".into()),
                ConflictReason::Group("manifests".into()),
            ]
        );
        assert!(r.to_text().contains("conflict group `core`"));
    }

    #[test]
    fn new_files_are_compared_by_prefix() {
        // Neither file exists yet.
        let a = task("T-000A", TaskStatus::Ready, &["crates/pm/**"]);
        let b = task("T-000B", TaskStatus::Ready, &["crates/pm/src/board.rs"]);
        let c = task("T-000C", TaskStatus::Ready, &["crates/sync/src/lib.rs"]);
        let r = check(&[a, b, c], &[]);
        assert_eq!(
            r.conflicts,
            [Conflict {
                a: id("T-000A"),
                b: id("T-000B"),
                reason: ConflictReason::Overlap(
                    "crates/pm/**".into(),
                    "crates/pm/src/board.rs".into()
                ),
            }]
        );
        // A new file inside a conflict group's folder touches the group.
        let d = task("T-000D", TaskStatus::Ready, &["crates/core/src/new.rs"]);
        let e = task("T-000E", TaskStatus::Ready, &["crates/core/src/lib.rs"]);
        let r = check(&[d, e], &[]);
        assert_eq!(
            r.conflicts
                .iter()
                .map(|c| c.reason.clone())
                .collect::<Vec<_>>(),
            [ConflictReason::Group("core".into())]
        );
        // Two different new files don't conflict.
        let f = task("T-000F", TaskStatus::Ready, &["crates/pm/src/a.rs"]);
        let g = task("T-000G", TaskStatus::Ready, &["crates/pm/src/b.rs"]);
        assert!(check(&[f, g], &[]).ok);
    }

    #[test]
    fn running_tasks_are_included_but_not_paired_together() {
        let r1 = task("T-00R1", TaskStatus::Running, &["crates/index/**"]);
        let r2 = task("T-00R2", TaskStatus::Running, &["crates/index/src/lib.rs"]);
        let merged = task("T-00M1", TaskStatus::Merged, &["crates/cli/**"]);
        let p = task(
            "T-00P1",
            TaskStatus::Ready,
            &["crates/cli/src/main.rs", "crates/index/src/fts.rs"],
        );
        let board = vec![r1, r2, merged];
        let r = check(std::slice::from_ref(&p), &board);
        assert_eq!(r.checked, [id("T-00P1"), id("T-00R1"), id("T-00R2")]);
        assert_eq!(
            r.conflicts,
            [Conflict {
                a: id("T-00P1"),
                b: id("T-00R1"),
                reason: ConflictReason::SharedFiles(vec!["crates/index/src/fts.rs".into()]),
            }]
        );
        // Nothing proposed: running-only pairs aren't reported.
        assert!(check(&[], &board).ok);
    }

    #[test]
    fn bad_globs_are_errors() {
        let a = task("T-000A", TaskStatus::Ready, &["crates/[oops"]);
        let err = board_check(&[a], &[], &files(), &groups());
        assert!(matches!(err, Err(CheckError::BadGlob { .. })));
    }

    #[test]
    fn report_serialises_stably() {
        let a = task("T-000A", TaskStatus::Ready, &["Cargo.toml"]);
        let b = task("T-000B", TaskStatus::Ready, &["Cargo.toml"]);
        let r = check(&[a, b], &[]);
        let json = serde_json::to_string(&r).unwrap_or_default();
        assert_eq!(
            json,
            r#"{"ok":false,"checked":["T-000A","T-000B"],"conflicts":[{"a":"T-000A","b":"T-000B","reason":{"kind":"shared_files","detail":["Cargo.toml"]}},{"a":"T-000A","b":"T-000B","reason":{"kind":"group","detail":"manifests"}}]}"#
        );
    }
}
