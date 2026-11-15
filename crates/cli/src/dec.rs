//! `inloop dec ...`, `inloop search` and `inloop lint`.

use std::io::{BufRead, IsTerminal, Write};
use std::process::ExitCode;

use anyhow::{Context as _, bail};
use clap::{Args, Subcommand};
use inloop_app::decisions::{DecisionView, ListFilter, SearchResult};
use inloop_decisions::Status;
use inloop_decisions::lint::Severity;
use inloop_decisions::ops::NewDecision;
use inloop_index::Kind;

use crate::Ctx;
use crate::output::{self, Style};

#[derive(Subcommand)]
pub enum DecCmd {
    /// Record a decision (prompts for anything missing when run in a terminal).
    New(NewArgs),
    /// Show decisions.
    Show {
        /// IDs or aliases, e.g. D-7KQ2, 7KQ2 or D-264. The prefix is optional.
        #[arg(required = true)]
        ids: Vec<String>,
    },
    /// List decisions, newest first.
    List(ListArgs),
    /// Search decisions.
    Search {
        #[arg(required = true)]
        query: Vec<String>,
        #[arg(long, default_value_t = 10)]
        limit: usize,
    },
    /// Accept proposed decisions. You are the human deciding.
    Accept {
        /// One or more IDs; the `D-` prefix is optional.
        #[arg(required = true)]
        ids: Vec<String>,
        /// Your words: why you accept it. Defaults to "Accepted by <you>".
        #[arg(long)]
        basis: Option<String>,
        /// Who decided (defaults to your git user.name).
        #[arg(long)]
        by: Option<String>,
    },
    /// Reject proposed decisions.
    Reject {
        /// One or more IDs; the `D-` prefix is optional.
        #[arg(required = true)]
        ids: Vec<String>,
        /// Why, in your words.
        #[arg(long)]
        reason: Option<String>,
    },
    /// Record that NEW replaces OLD (both files in this worktree).
    Supersede { old: String, new: String },
}

#[derive(Args, Default)]
pub struct NewArgs {
    /// One line, stated as the decision itself.
    #[arg(long, short)]
    pub title: Option<String>,
    #[arg(long)]
    pub context: Option<String>,
    #[arg(long)]
    pub decision: Option<String>,
    #[arg(long)]
    pub consequences: Option<String>,
    /// proposed or accepted. Accepted needs --basis.
    #[arg(long)]
    pub status: Option<String>,
    /// The human's words this rests on.
    #[arg(long)]
    pub basis: Option<String>,
    /// Who decided (defaults to your git user.name for accepted decisions).
    #[arg(long)]
    pub by: Option<String>,
    #[arg(long = "tag")]
    pub tags: Vec<String>,
    /// IDs this decision replaces.
    #[arg(long)]
    pub supersedes: Vec<String>,
}

#[derive(Args)]
pub struct ListArgs {
    #[arg(long)]
    pub status: Option<String>,
    #[arg(long)]
    pub tag: Option<String>,
    /// Only decisions not on the main branch yet.
    #[arg(long)]
    pub unmerged: bool,
    /// Only decisions on the main branch.
    #[arg(long, conflicts_with = "unmerged")]
    pub merged: bool,
    /// Only decisions dated on or after YYYY-MM-DD.
    #[arg(long)]
    pub since: Option<String>,
}

pub fn run(ctx: &Ctx, cmd: DecCmd) -> anyhow::Result<ExitCode> {
    match cmd {
        DecCmd::New(a) => new(ctx, a),
        DecCmd::Show { ids } => show(ctx, &ids),
        DecCmd::List(a) => list(ctx, a),
        DecCmd::Search { query, limit } => search(ctx, &query.join(" "), &[Kind::Decision], limit),
        DecCmd::Accept { ids, basis, by } => {
            let p = ctx.project()?;
            let who = by
                .or_else(|| p.git_user())
                .unwrap_or_else(|| "unknown".into());
            let basis =
                basis.unwrap_or_else(|| format!("Accepted by {who} with `inloop dec accept`."));
            p.decisions_exist(&ids)?;
            let (done, code) = crate::each_id(&ids, |id| {
                Ok(p.decision_promote(id, true, Some(basis.clone()), Some(who.clone()), &who)?)
            });
            ctx.emit_each(&done, |s, w| {
                format!("{} {} accepted\n", s.green("✓"), w.id)
            })?;
            Ok(code)
        }
        DecCmd::Reject { ids, reason } => {
            let p = ctx.project()?;
            let who = p.git_user().unwrap_or_else(|| "unknown".into());
            p.decisions_exist(&ids)?;
            let (done, code) = crate::each_id(&ids, |id| {
                Ok(p.decision_promote(id, false, reason.clone(), None, &who)?)
            });
            ctx.emit_each(&done, |s, w| {
                format!("{} {} rejected\n", s.yellow("✓"), w.id)
            })?;
            Ok(code)
        }
        DecCmd::Supersede { old, new } => {
            let p = ctx.project()?;
            let w = p.decision_supersede(&old, &new)?;
            ctx.emit(&w, |s| {
                format!("{} {} now supersedes {old}\n", s.green("✓"), w.id)
            })?;
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn prompt(label: &str, multiline: bool) -> anyhow::Result<String> {
    let stdin = std::io::stdin();
    let mut err = std::io::stderr();
    if multiline {
        writeln!(err, "{label} (end with an empty line):")?;
        let mut lines = Vec::new();
        for line in stdin.lock().lines() {
            let line = line?;
            if line.trim().is_empty() {
                break;
            }
            lines.push(line);
        }
        Ok(lines.join("\n"))
    } else {
        write!(err, "{label}: ")?;
        err.flush()?;
        let mut s = String::new();
        stdin.lock().read_line(&mut s)?;
        Ok(s.trim().to_string())
    }
}

fn new(ctx: &Ctx, mut a: NewArgs) -> anyhow::Result<ExitCode> {
    let p = ctx.project()?;
    let interactive = std::io::stdin().is_terminal() && !ctx.json;
    if a.title.is_none() {
        if !interactive {
            bail!("--title is required (or run in a terminal to be asked)");
        }
        a.title = Some(prompt("Title, stated as the decision", false)?);
        a.context = Some(prompt("Context: why this came up", true)?);
        a.decision = Some(prompt("Decision: what was decided", true)?);
        a.consequences = Some(prompt(
            "Consequences: what follows, what it rules out",
            true,
        )?);
        if a.basis.is_none() && a.status.as_deref() != Some("proposed") {
            let b = prompt(
                "Basis: the human's words it rests on (empty = proposed)",
                false,
            )?;
            if !b.is_empty() {
                a.basis = Some(b);
            }
        }
    }
    let status = match a.status.as_deref() {
        None => None,
        Some(s) => Some(
            Status::parse(s).with_context(|| format!("status `{s}`: use proposed or accepted"))?,
        ),
    };
    let accepted = status == Some(Status::Accepted) || (status.is_none() && a.basis.is_some());
    let decided_by = if accepted {
        a.by.or_else(|| p.git_user())
    } else {
        a.by
    };
    let w = p.decision_new(NewDecision {
        title: a.title.unwrap_or_default(),
        status,
        decided_by,
        basis: a.basis,
        session: p.current_session(),
        tags: a.tags,
        context: a.context.unwrap_or_default(),
        decision: a.decision.unwrap_or_default(),
        consequences: a.consequences.unwrap_or_default(),
        supersedes: a.supersedes,
    })?;
    ctx.emit(&w, |s| {
        let mut out = format!(
            "{} recorded {}  {}\n",
            s.green("✓"),
            s.bold(w.id.as_str()),
            s.dim(&w.path.display().to_string())
        );
        for c in &w.also_changed {
            out.push_str(&format!("  updated {}\n", s.dim(&c.display().to_string())));
        }
        out
    })?;
    Ok(ExitCode::SUCCESS)
}

/// The status line under a decision: where it stands.
pub fn where_line(s: &Style, e: &DecisionView) -> Option<String> {
    let line = e
        .provenance
        .status_line(inloop_core::config::home_dir().as_deref());
    if line.is_empty() {
        None
    } else {
        Some(s.yellow(&line))
    }
}

fn show(ctx: &Ctx, ids: &[String]) -> anyhow::Result<ExitCode> {
    let p = ctx.project()?;
    let (found, code) = crate::each_id(ids, |id| Ok(p.decision_get(id)?));
    ctx.emit_each(&found, render_decision)?;
    Ok(code)
}

pub fn render_decision(s: &Style, e: &DecisionView) -> String {
    let d = &e.decision;
    let mut out = String::new();
    out.push_str(&format!(
        "{}  {}\n",
        s.bold(d.id.as_str()),
        s.bold(&d.title)
    ));
    let mut meta = vec![s.status(d.status.as_str()), d.date.clone()];
    if !d.decided_by.is_empty() {
        meta.push(format!("decided by {}", d.decided_by));
    }
    if let Some(p) = &d.promoted_by {
        meta.push(format!("promoted by {p}"));
    }
    out.push_str(&format!("{}\n", meta.join(" · ")));
    if let Some(w) = where_line(s, e) {
        out.push_str(&format!("{w}\n"));
    }
    if let Some(b) = &d.basis {
        out.push_str(&format!("{} {}\n", s.dim("basis:"), b));
    }
    let mut links = Vec::new();
    if !d.supersedes.is_empty() {
        links.push(format!(
            "supersedes {}",
            d.supersedes
                .iter()
                .map(|i| i.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if let Some(b) = &d.superseded_by {
        links.push(s.red(&format!("superseded by {b}")));
    }
    if !d.aliases.is_empty() {
        links.push(format!("aliases {}", d.aliases.join(", ")));
    }
    if !d.tags.is_empty() {
        links.push(format!("tags {}", d.tags.join(", ")));
    }
    if let Some(sess) = &d.session {
        links.push(format!("session {sess}"));
    }
    if !links.is_empty() {
        out.push_str(&format!("{}\n", s.dim(&links.join(" · "))));
    }
    out.push('\n');
    out.push_str(&output::markdown(s, &d.body));
    out
}

fn list(ctx: &Ctx, a: ListArgs) -> anyhow::Result<ExitCode> {
    let p = ctx.project()?;
    let status = match a.status.as_deref() {
        None => None,
        Some(s) => Some(Status::parse(s).with_context(|| {
            format!("status `{s}`: use proposed, accepted, rejected or superseded")
        })?),
    };
    let items = p.decision_list(&ListFilter {
        status,
        tag: a.tag,
        unmerged_only: a.unmerged,
        since: a.since,
        merged_only: a.merged,
    })?;
    ctx.emit(&items, |s| list_text(s, &items))?;
    Ok(ExitCode::SUCCESS)
}

pub fn list_text(s: &Style, items: &[DecisionView]) -> String {
    if items.is_empty() {
        return "No decisions found.\n".into();
    }
    let rows: Vec<Vec<String>> = items
        .iter()
        .map(|e| {
            let title = match where_line(s, e) {
                Some(w) => format!("{}\n{}", e.decision.title, w),
                None => e.decision.title.clone(),
            };
            vec![
                e.decision.id.to_string(),
                s.status(e.decision.status.as_str()),
                e.decision.date.clone(),
                title,
            ]
        })
        .collect();
    // The status line goes on its own row under the title.
    let mut flat = Vec::new();
    for r in rows {
        let mut parts = r[3].splitn(2, '\n');
        let title = parts.next().unwrap_or_default().to_string();
        flat.push(vec![r[0].clone(), r[1].clone(), r[2].clone(), title]);
        if let Some(w) = parts.next() {
            flat.push(vec![
                String::new(),
                String::new(),
                String::new(),
                w.to_string(),
            ]);
        }
    }
    let mut out = output::table(s, &["ID", "STATUS", "DATE", "TITLE"], &flat);
    out.push_str(&s.dim(&format!("{} decisions\n", items.len())));
    out
}

pub fn search_all(ctx: &Ctx, query: &str, limit: usize) -> anyhow::Result<ExitCode> {
    search(ctx, query, &[], limit)
}

fn search(ctx: &Ctx, query: &str, kinds: &[Kind], limit: usize) -> anyhow::Result<ExitCode> {
    let p = ctx.project()?;
    let hits = p.search(query, kinds, limit)?;
    ctx.emit(&hits, |s| search_text(s, &hits))?;
    Ok(ExitCode::SUCCESS)
}

pub fn search_text(s: &Style, hits: &[SearchResult]) -> String {
    if hits.is_empty() {
        return "Nothing found.\n".into();
    }
    let mut out = String::new();
    for h in hits {
        let status = h.meta.get("status").and_then(|v| v.as_str()).unwrap_or("");
        let kind = match h.kind {
            Kind::Decision => String::new(),
            Kind::Session => s.dim("session "),
        };
        out.push_str(&format!(
            "{kind}{}  {}  {}\n",
            s.bold(&h.id),
            h.title,
            s.status(status)
        ));
        if let Some(p) = h.meta.get("provenance")
            && let Ok(p) = serde_json::from_value::<inloop_core::Provenance>(p.clone())
        {
            let line = p.status_line(inloop_core::config::home_dir().as_deref());
            if !line.is_empty() {
                out.push_str(&format!("  {}\n", s.yellow(&line)));
            }
        }
        let snippet = h.snippet.replace('\n', " ");
        let snippet = highlight(s, &snippet);
        out.push_str(&output::wrap_text(&snippet, s.width, "  "));
        out.push('\n');
    }
    out
}

/// Turn the index's `[match]` markers into bold.
fn highlight(s: &Style, text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find('[') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find(']') {
            Some(end) if !after[..end].starts_with("REDACTED") => {
                out.push_str(&s.bold(&after[..end]));
                rest = &after[end + 1..];
            }
            _ => {
                out.push('[');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

pub fn lint(ctx: &Ctx) -> anyhow::Result<ExitCode> {
    let p = ctx.project()?;
    let r = p.lint()?;
    ctx.emit(&r, |s| {
        let mut out = String::new();
        for i in &r.issues {
            let sev = match i.severity {
                Severity::Error => s.red("error"),
                Severity::Warning => s.yellow("warning"),
            };
            let loc = match (&i.file, i.line) {
                (Some(f), Some(l)) => format!("{f}:{l}: "),
                (Some(f), None) => format!("{f}: "),
                _ => String::new(),
            };
            out.push_str(&format!(
                "{sev} [{}] {}{}\n",
                i.code,
                s.dim(&loc),
                i.message
            ));
        }
        if r.issues.is_empty() {
            out.push_str(&format!("{} no problems found\n", s.green("✓")));
        } else {
            out.push_str(&format!("\n{} errors, {} warnings\n", r.errors, r.warnings));
        }
        out
    })?;
    Ok(if r.errors > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}
