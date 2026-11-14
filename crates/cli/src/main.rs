//! The `inloop` command (docs/06).
#![forbid(unsafe_code)]

mod dec;
mod hooks;
mod init;
mod output;
mod pm;
mod session;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};
use inloop_app::Project;
use serde::Serialize;

use crate::output::Style;

#[derive(Parser)]
#[command(
    name = "inloop",
    version,
    about = "Decisions and agent sessions for AI-driven development, with a human in the loop",
    propagate_version = true
)]
pub struct Cli {
    /// Print JSON with the same fields MCP returns, instead of human output.
    #[arg(long, global = true)]
    pub json: bool,
    /// Run as if started in this directory.
    #[arg(short = 'C', global = true, value_name = "DIR")]
    pub dir: Option<PathBuf>,
    #[command(subcommand)]
    pub cmd: Cmd,
}

#[derive(Subcommand)]
pub enum Cmd {
    /// Set up a repo: inloop.toml, the decisions directory, MCP registration and hooks.
    Init(init::InitArgs),
    /// Decisions: record, show, list, search, accept, reject, supersede.
    #[command(alias = "decision", subcommand)]
    Dec(dec::DecCmd),
    /// Captured agent sessions: list, show, search, status, purge.
    #[command(subcommand)]
    Session(session::SessionCmd),
    /// This project's session encryption key.
    #[command(subcommand)]
    Key(session::KeyCmd),
    /// Re-run redaction over everything stored, fixing what it finds.
    Scan {
        /// Also rewrite decision files in this worktree (sessions are always fixed).
        #[arg(long)]
        fix: bool,
    },
    /// The PM work board (only when [pm] enabled = true).
    Board {
        #[command(subcommand)]
        cmd: Option<pm::BoardCmd>,
    },
    /// PM tasks: new, show, update, brief, check, report, blocked (only when [pm] enabled = true).
    #[command(subcommand)]
    Task(pm::TaskCmd),
    /// Search decisions and sessions together.
    Search {
        /// Words to search for.
        #[arg(required = true)]
        query: Vec<String>,
        #[arg(long, default_value_t = 10)]
        limit: usize,
    },
    /// Check decisions: parsing, IDs, basis, links and references.
    Lint,
    /// Where you are: repo, worktree, branch, main, ahead and behind.
    Status,
    /// Run the MCP server over stdio.
    Mcp {
        /// List the tools that would be registered, and exit.
        #[arg(long)]
        list: bool,
    },
}

/// Shared state for commands.
pub struct Ctx {
    pub json: bool,
    pub style: Style,
    pub dir: PathBuf,
}

impl Ctx {
    pub fn project(&self) -> anyhow::Result<Project> {
        Ok(Project::open(&self.dir)?)
    }

    /// Print `value` as JSON when --json, else run `human`.
    pub fn emit<T: Serialize>(
        &self,
        value: &T,
        human: impl FnOnce(&Style) -> String,
    ) -> anyhow::Result<()> {
        if self.json {
            println!("{}", serde_json::to_string_pretty(value)?);
        } else {
            let text = human(&self.style);
            output::page(&self.style, &text);
        }
        Ok(())
    }
}

fn main() -> ExitCode {
    // The PM commands only show in help when the project turns the layer on
    // (D-N6YH). They still parse, and refuse with a clear message.
    let pm_on = std::env::current_dir()
        .ok()
        .and_then(|d| Project::open(&d).ok())
        .is_some_and(|p| p.pm_enabled());
    let mut command = Cli::command();
    if !pm_on {
        command = command
            .mut_subcommand("board", |c| c.hide(true))
            .mut_subcommand("task", |c| c.hide(true));
    }
    let cli = match Cli::from_arg_matches(&command.get_matches()) {
        Ok(c) => c,
        Err(e) => e.exit(),
    };
    let dir = match cli.dir.clone() {
        Some(d) => d,
        None => match std::env::current_dir() {
            Ok(d) => d,
            Err(e) => {
                eprintln!("error: can't read the current directory: {e}");
                return ExitCode::FAILURE;
            }
        },
    };
    let ctx = Ctx {
        json: cli.json,
        style: Style::detect(),
        dir,
    };
    match run(&ctx, cli.cmd) {
        Ok(code) => code,
        Err(e) => {
            if ctx.json {
                println!("{}", serde_json::json!({ "error": format!("{e:#}") }));
            }
            eprintln!("{} {e:#}", ctx.style.red("error:"));
            ExitCode::FAILURE
        }
    }
}

fn run(ctx: &Ctx, cmd: Cmd) -> anyhow::Result<ExitCode> {
    match cmd {
        Cmd::Init(a) => init::run(ctx, a),
        Cmd::Dec(c) => dec::run(ctx, c),
        Cmd::Session(c) => session::run(ctx, c),
        Cmd::Key(c) => session::key(ctx, c),
        Cmd::Scan { fix } => scan(ctx, fix),
        Cmd::Board { cmd } => pm::board(ctx, cmd),
        Cmd::Task(c) => pm::task(ctx, c),
        Cmd::Search { query, limit } => dec::search_all(ctx, &query.join(" "), limit),
        Cmd::Lint => dec::lint(ctx),
        Cmd::Status => status(ctx),
        Cmd::Mcp { list } => {
            let server = inloop_mcp::InLoopServer::new(ctx.dir.clone());
            if list {
                for n in server.tool_names() {
                    println!("{n}");
                }
                return Ok(ExitCode::SUCCESS);
            }
            let rt = tokio_runtime()?;
            rt.block_on(inloop_mcp::serve_stdio(ctx.dir.clone()))
                .map_err(anyhow::Error::msg)?;
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn tokio_runtime() -> anyhow::Result<inloop_mcp::Runtime> {
    inloop_mcp::runtime().map_err(|e| anyhow::anyhow!("could not start the async runtime: {e}"))
}

fn status(ctx: &Ctx) -> anyhow::Result<ExitCode> {
    let p = ctx.project()?;
    let c = p.context()?;
    ctx.emit(&c, |s| {
        let mut out = String::new();
        out.push_str(&format!(
            "{}  {}\n",
            s.bold(&c.project),
            s.dim(&c.worktree.display().to_string())
        ));
        let branch = c.branch.clone().unwrap_or_else(|| "(detached)".into());
        let pos = match (c.ahead_of_main, c.behind_main) {
            (Some(a), Some(b)) if !c.on_main => {
                format!(" · {a} ahead, {b} behind {}", c.main_branch)
            }
            _ => String::new(),
        };
        out.push_str(&format!("branch {}{pos}\n", s.cyan(&branch)));
        if let Some(t) = &c.task {
            out.push_str(&format!("task {t}\n"));
        }
        if c.worktrees.len() > 1 {
            out.push_str(&format!("\n{}\n", s.dim("worktrees")));
            for w in &c.worktrees {
                out.push_str(&format!(
                    "{} {}  {}\n",
                    if w.current { "*" } else { " " },
                    output::pad(w.branch.as_deref().unwrap_or("(detached)"), 24),
                    s.dim(&w.path.display().to_string())
                ));
            }
        }
        out
    })?;
    Ok(ExitCode::SUCCESS)
}

fn scan(ctx: &Ctx, fix: bool) -> anyhow::Result<ExitCode> {
    let p = ctx.project()?;
    let r = p.scan(fix)?;
    ctx.emit(&r, |s| {
        let mut out = format!(
            "checked {} decisions, {} sessions, {} board records\n",
            r.decisions_checked, r.sessions_checked, r.board_records_checked
        );
        for f in &r.findings {
            let kinds: Vec<String> = f.kinds.iter().map(|(k, n)| format!("{k}×{n}")).collect();
            out.push_str(&format!(
                "{} {}: {}{}\n",
                if f.fixed {
                    s.green("fixed")
                } else {
                    s.yellow("found")
                },
                f.what,
                kinds.join(" "),
                if f.fixed {
                    ""
                } else {
                    " (run with --fix to rewrite it)"
                }
            ));
        }
        if r.findings.is_empty() {
            out.push_str(&format!("{} nothing found\n", s.green("✓")));
        }
        out
    })?;
    Ok(if r.findings.iter().any(|f| !f.fixed) {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}
