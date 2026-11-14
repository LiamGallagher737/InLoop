//! `inloop session ...`, `inloop key ...` and `inloop scan`.

use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, ExitCode, Stdio};

use anyhow::{Context as _, bail};
use clap::Subcommand;
use inloop_app::Project;
use inloop_app::sessions::SessionView;
use inloop_index::Kind;
use inloop_sessions::{AgentSource, ClaudeCode, HookEvent};

use crate::Ctx;
use crate::output::{self, Style};

#[derive(Subcommand)]
pub enum SessionCmd {
    /// List captured sessions, newest first.
    List,
    /// Show a session's summary, or its redacted transcript.
    Show {
        /// Session ID, or a unique prefix.
        id: String,
        /// Print the full (redacted) transcript.
        #[arg(long)]
        transcript: bool,
    },
    /// Search session summaries.
    Search {
        #[arg(required = true)]
        query: Vec<String>,
        #[arg(long, default_value_t = 10)]
        limit: usize,
    },
    /// Capture health: store, key, sessions not captured, recent failures.
    Status,
    /// Delete a session completely: transcript, metadata and index entry.
    Purge { id: String },
    /// Rebuild the session index from the store (for a shared bucket).
    Sync,
    /// Capture a session now. Without flags, reads Claude Code hook input on stdin.
    Capture {
        /// The transcript file.
        #[arg(long)]
        transcript: Option<PathBuf>,
        /// The session ID (defaults to the transcript file name).
        #[arg(long)]
        session: Option<String>,
        /// The session's working directory.
        #[arg(long)]
        cwd: Option<PathBuf>,
        /// The hook event that triggered the capture.
        #[arg(long, default_value = "manual")]
        event: String,
        /// Capture every transcript of this project that isn't stored yet.
        #[arg(long, conflicts_with_all = ["transcript", "session"])]
        backfill: bool,
        /// Capture even if the transcript hasn't changed.
        #[arg(long)]
        force: bool,
    },
    /// Called by Claude Code hooks. Reads hook input on stdin, never fails.
    #[command(hide = true)]
    Hook,
    /// Called by the git prepare-commit-msg hook: add a `Session:` trailer.
    #[command(hide = true)]
    Trailer {
        file: PathBuf,
        source: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum KeyCmd {
    /// Where this project's key is, and its public recipient.
    Show,
    /// Write this project's secret key to FILE (mode 0600), to give a co-founder.
    Export { file: PathBuf },
    /// Import a project key from FILE. Refuses to replace an existing key.
    Import { file: PathBuf },
}

pub fn run(ctx: &Ctx, cmd: SessionCmd) -> anyhow::Result<ExitCode> {
    match cmd {
        SessionCmd::Hook => {
            hook(ctx);
            Ok(ExitCode::SUCCESS)
        }
        SessionCmd::Trailer { file, source } => {
            trailer(ctx, &file, source.as_deref());
            Ok(ExitCode::SUCCESS)
        }
        SessionCmd::List => {
            let p = ctx.project()?;
            let items = p.session_list()?;
            ctx.emit(&items, |s| list_text(s, &items))?;
            Ok(ExitCode::SUCCESS)
        }
        SessionCmd::Show { id, transcript } => {
            let p = ctx.project()?;
            let v = p.session_get(&id, transcript)?;
            ctx.emit(&v, |s| show_text(s, &v))?;
            Ok(ExitCode::SUCCESS)
        }
        SessionCmd::Search { query, limit } => {
            let p = ctx.project()?;
            let hits = p.search(&query.join(" "), &[Kind::Session], limit)?;
            ctx.emit(&hits, |s| crate::dec::search_text(s, &hits))?;
            Ok(ExitCode::SUCCESS)
        }
        SessionCmd::Status => {
            let p = ctx.project()?;
            let st = p.session_status(&ClaudeCode)?;
            ctx.emit(&st, |s| {
                let mut out = format!("store     {}\nkey       {}{}\ncaptured  {}\n", st.store, st.key,
                    if st.key_created { s.yellow(" (just created)") } else { String::new() }, st.captured);
                out.push_str(&format!(
                    "hooks     {}\n",
                    if st.hooks_installed { s.green("installed") } else { s.red("not installed: run `inloop init`") }
                ));
                if !st.not_captured.is_empty() {
                    out.push_str(&format!(
                        "\n{} transcripts not captured (run `inloop session capture --backfill`):\n",
                        st.not_captured.len()
                    ));
                    for id in st.not_captured.iter().take(10) {
                        out.push_str(&format!("  {id}\n"));
                    }
                }
                if !st.recent_failures.is_empty() {
                    out.push_str(&format!("\n{}\n", s.red("recent capture failures:")));
                    for f in &st.recent_failures {
                        out.push_str(&format!("  {} {} {}: {}\n", f.at, f.session, f.event, f.error.as_deref().unwrap_or("")));
                    }
                }
                out
            })?;
            Ok(ExitCode::SUCCESS)
        }
        SessionCmd::Purge { id } => {
            let p = ctx.project()?;
            let id = p.session_purge(&id)?;
            ctx.emit(&serde_json::json!({ "purged": id }), |s| {
                format!(
                    "{} purged session {id}: transcript, metadata and index entry are gone\n",
                    s.green("✓")
                )
            })?;
            Ok(ExitCode::SUCCESS)
        }
        SessionCmd::Sync => {
            let p = ctx.project()?;
            let n = p.session_sync()?;
            ctx.emit(&serde_json::json!({ "indexed": n }), |s| {
                format!("{} indexed {n} sessions\n", s.green("✓"))
            })?;
            Ok(ExitCode::SUCCESS)
        }
        SessionCmd::Capture {
            transcript,
            session,
            cwd,
            event,
            backfill,
            force,
        } => {
            let p = ctx.project()?;
            if backfill {
                return backfill_all(ctx, &p, force);
            }
            let ev = match transcript {
                Some(t) => HookEvent {
                    session_id: session.unwrap_or_else(|| {
                        t.file_stem()
                            .map(|s| s.to_string_lossy().into_owned())
                            .unwrap_or_default()
                    }),
                    transcript_path: Some(t),
                    cwd: cwd.or_else(|| Some(p.root().to_path_buf())),
                    event,
                },
                None => {
                    let mut input = String::new();
                    std::io::stdin().read_to_string(&mut input)?;
                    ClaudeCode.parse_hook_input(&input)?
                }
            };
            let out = p.capture(&ev, &ClaudeCode, force)?;
            ctx.emit(&out, |s| {
                if out.captured {
                    let red: Vec<String> = out
                        .redactions
                        .iter()
                        .map(|(k, n)| format!("{k}×{n}"))
                        .collect();
                    format!(
                        "{} captured {} ({} bytes{})\n",
                        s.green("✓"),
                        out.id,
                        out.transcript_bytes,
                        if red.is_empty() {
                            String::new()
                        } else {
                            format!(", redacted {}", red.join(" "))
                        }
                    )
                } else {
                    format!(
                        "{} {}: {}\n",
                        s.dim("="),
                        out.id,
                        out.skipped.as_deref().unwrap_or("skipped")
                    )
                }
            })?;
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn backfill_all(ctx: &Ctx, p: &Project, force: bool) -> anyhow::Result<ExitCode> {
    let st = p.session_status(&ClaudeCode)?;
    let mut done = Vec::new();
    let mut failed = Vec::new();
    let mut dirs = Vec::new();
    for w in p.repo.worktrees()? {
        if let Some(d) = ClaudeCode.transcripts_dir(&w.path) {
            dirs.push((d, w.path));
        }
    }
    for id in &st.not_captured {
        let Some((path, cwd)) = dirs
            .iter()
            .map(|(d, w)| (d.join(format!("{id}.jsonl")), w.clone()))
            .find(|(f, _)| f.exists())
        else {
            continue;
        };
        let ev = HookEvent {
            session_id: id.clone(),
            transcript_path: Some(path),
            cwd: Some(cwd),
            event: "backfill".into(),
        };
        match p.capture(&ev, &ClaudeCode, force) {
            Ok(o) if o.captured => done.push(id.clone()),
            Ok(_) => {}
            Err(e) => failed.push(format!("{id}: {e}")),
        }
    }
    ctx.emit(
        &serde_json::json!({ "captured": done, "failed": failed }),
        |s| {
            let mut out = format!("{} captured {} sessions\n", s.green("✓"), done.len());
            for f in &failed {
                out.push_str(&format!("{} {f}\n", s.red("✗")));
            }
            out
        },
    )?;
    Ok(if failed.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

/// The Claude Code hook. Must be quick and must never block or fail the
/// agent: capture runs in a detached background process, and any error is
/// logged and swallowed.
fn hook(ctx: &Ctx) {
    let mut input = String::new();
    if std::io::stdin().read_to_string(&mut input).is_err() {
        return;
    }
    let Ok(ev) = ClaudeCode.parse_hook_input(&input) else {
        return;
    };
    let dir = ev.cwd.clone().unwrap_or_else(|| ctx.dir.clone());
    let Ok(p) = Project::open(&dir) else {
        return; // not a repo: nothing to do
    };
    if ev.event == "SessionEnd" {
        p.clear_active(&ev);
    } else {
        let _ = p.mark_active(&ev);
    }
    if ev.event == "SessionStart" {
        return;
    }
    if std::env::var_os("INLOOP_CAPTURE_SYNC").is_some() {
        let _ = p.capture(&ev, &ClaudeCode, false);
        return;
    }
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let Some(transcript) = ev.transcript_path.clone() else {
        return;
    };
    let mut cmd = Command::new(exe);
    cmd.arg("-C")
        .arg(&dir)
        .args([
            "session",
            "capture",
            "--session",
            &ev.session_id,
            "--event",
            &ev.event,
            "--transcript",
        ])
        .arg(transcript)
        .arg("--cwd")
        .arg(&dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let _ = cmd.spawn();
}

/// The git hook: add `Session: <id>` when a captured session is running in
/// this worktree. Never fails the commit.
fn trailer(ctx: &Ctx, file: &std::path::Path, source: Option<&str>) {
    if matches!(source, Some("merge") | Some("squash")) {
        return;
    }
    let Ok(p) = Project::open(&ctx.dir) else {
        return;
    };
    let Some(id) = p.current_session() else {
        return;
    };
    let _ = p.repo.try_run(&[
        "interpret-trailers",
        "--in-place",
        "--if-exists",
        "addIfDifferent",
        "--trailer",
        &format!("Session: {id}"),
        &file.to_string_lossy(),
    ]);
}

pub fn key(ctx: &Ctx, cmd: KeyCmd) -> anyhow::Result<ExitCode> {
    let p = ctx.project()?;
    let path = p.key_path()?;
    match cmd {
        KeyCmd::Show => {
            let (key, created) = p.project_key()?;
            let v = serde_json::json!({ "path": path, "recipient": key.recipient(), "created": created });
            ctx.emit(&v, |s| {
                format!(
                    "{}\n{}  {}\n",
                    s.bold(&path.display().to_string()),
                    s.dim("public:"),
                    key.recipient()
                ) + if created { "(created just now)\n" } else { "" }
            })?;
        }
        KeyCmd::Export { file } => {
            let (key, _) = p.project_key()?;
            write_secret_file(&file, &key)?;
            ctx.emit(&serde_json::json!({ "written": file }), |s| {
                format!(
                    "{} wrote the {} project key to {}\nGive it to a co-founder privately, then delete the file. Anyone with it can read this project's sessions.\n",
                    s.green("✓"),
                    p.name,
                    file.display()
                )
            })?;
        }
        KeyCmd::Import { file } => {
            if path.exists() {
                bail!(
                    "a key already exists at {}; move it aside first if you really mean to replace it",
                    path.display()
                );
            }
            let text = std::fs::read_to_string(&file)
                .with_context(|| format!("reading {}", file.display()))?;
            let key = inloop_store::ProjectKey::import(text.trim())?;
            key.save(&path)?;
            ctx.emit(
                &serde_json::json!({ "imported": path, "recipient": key.recipient() }),
                |s| format!("{} imported the key to {}\n", s.green("✓"), path.display()),
            )?;
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn write_secret_file(file: &std::path::Path, key: &inloop_store::ProjectKey) -> anyhow::Result<()> {
    use secrecy::ExposeSecret;
    use std::io::Write;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts
        .open(file)
        .with_context(|| format!("creating {} (it must not exist yet)", file.display()))?;
    writeln!(f, "{}", key.export_secret().expose_secret())?;
    Ok(())
}

pub fn list_text(s: &Style, items: &[SessionView]) -> String {
    if items.is_empty() {
        return "No sessions captured yet. Run `inloop init` to install the capture hooks.\n"
            .into();
    }
    let rows: Vec<Vec<String>> = items
        .iter()
        .map(|v| {
            let started = v
                .meta
                .started
                .as_deref()
                .unwrap_or("")
                .chars()
                .take(16)
                .collect::<String>()
                .replace('T', " ");
            vec![
                v.meta.id.chars().take(8).collect(),
                started,
                v.meta.branch.clone().unwrap_or_default(),
                v.meta.summary.title(),
            ]
        })
        .collect();
    let mut out = output::table(s, &["ID", "STARTED", "BRANCH", "ASKED"], &rows);
    out.push_str(&s.dim(&format!("{} sessions\n", items.len())));
    out
}

pub fn show_text(s: &Style, v: &SessionView) -> String {
    let m = &v.meta;
    let mut out = format!(
        "{}  {}\n",
        s.bold(&format!("session {}", m.id)),
        s.dim(&m.source)
    );
    let mut meta = Vec::new();
    if let Some(b) = &m.branch {
        meta.push(format!("branch {b}"));
    }
    if let (Some(a), Some(b)) = (&m.started, &m.ended) {
        meta.push(format!("{a} → {b}"));
    }
    meta.push(format!("{} turns", m.summary.turns));
    out.push_str(&format!("{}\n", s.dim(&meta.join(" · "))));
    let line = v
        .provenance
        .status_line(inloop_core::config::home_dir().as_deref());
    if !line.is_empty() {
        out.push_str(&format!("{}\n", s.yellow(&line)));
    }
    out.push('\n');
    out.push_str(&format!(
        "{}\n{}\n",
        s.bold("Asked"),
        output::wrap_text(&m.summary.asked, s.width, "  ")
    ));
    if !m.summary.later_prompts.is_empty() {
        out.push_str(&format!("{}\n", s.bold("Then")));
        for p in &m.summary.later_prompts {
            out.push_str(&output::wrap_text(&format!("• {p}"), s.width, "  "));
        }
        out.push('\n');
    }
    out.push_str(&format!(
        "{}\n{}\n",
        s.bold("Done"),
        output::wrap_text(&m.summary.done, s.width, "  ")
    ));
    if !m.summary.files_touched.is_empty() {
        out.push_str(&format!("{}\n", s.bold("Files")));
        for f in &m.summary.files_touched {
            out.push_str(&format!("  {f}\n"));
        }
        out.push('\n');
    }
    if !m.summary.decisions.is_empty() {
        out.push_str(&format!(
            "{} {}\n",
            s.bold("Decisions"),
            m.summary.decisions.join(", ")
        ));
    }
    if !m.commits.is_empty() {
        let short: Vec<String> = m
            .commits
            .iter()
            .map(|c| c.chars().take(10).collect())
            .collect();
        out.push_str(&format!("{} {}\n", s.bold("Commits"), short.join(", ")));
    }
    if !m.summary.open_ends.is_empty() {
        out.push_str(&format!("{}\n", s.bold("Open ends")));
        for o in &m.summary.open_ends {
            out.push_str(&format!("  • {o}\n"));
        }
    }
    if !m.redactions.is_empty() {
        let red: Vec<String> = m
            .redactions
            .iter()
            .map(|(k, n)| format!("{k}×{n}"))
            .collect();
        out.push_str(&format!(
            "{} {}\n",
            s.dim("redacted"),
            s.dim(&red.join(" "))
        ));
    }
    if let Some(t) = &v.transcript {
        out.push_str(&format!("\n{}\n", s.bold("Transcript")));
        out.push_str(&render_transcript(s, t));
    }
    out
}

/// A readable view of a JSONL transcript.
fn render_transcript(s: &Style, jsonl: &str) -> String {
    let t = inloop_sessions::Transcript::parse_jsonl(jsonl);
    let mut out = String::new();
    for r in &t.records {
        if let Some(p) = inloop_sessions::Transcript::prompt_text(r) {
            out.push_str(&format!(
                "\n{}\n{}",
                s.cyan("▌ you"),
                output::wrap_text(&p, s.width, "  ")
            ));
        } else if let Some(a) = inloop_sessions::Transcript::assistant_text(r) {
            out.push_str(&format!(
                "\n{}\n{}",
                s.green("▌ agent"),
                output::wrap_text(&a, s.width, "  ")
            ));
        }
        for b in inloop_sessions::Transcript::blocks(r) {
            if b.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                let name = b.get("name").and_then(|n| n.as_str()).unwrap_or("tool");
                let detail = b
                    .get("input")
                    .and_then(|i| {
                        i.get("command")
                            .or_else(|| i.get("file_path"))
                            .or_else(|| i.get("pattern"))
                    })
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                out.push_str(&format!(
                    "  {} {}\n",
                    s.dim(&format!("⚙ {name}")),
                    s.dim(&output::truncate(detail, 100))
                ));
            }
        }
    }
    out
}
