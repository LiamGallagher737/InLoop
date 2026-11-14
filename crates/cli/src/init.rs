//! `inloop init`: set up a repo.

use std::path::Path;
use std::process::ExitCode;

use anyhow::Context as _;
use clap::Args;
use serde::Serialize;

use crate::Ctx;

#[derive(Args)]
pub struct InitArgs {
    /// Don't register the MCP server in .mcp.json.
    #[arg(long)]
    pub no_mcp: bool,
    /// Don't install Claude Code session capture hooks or the git commit hook.
    #[arg(long)]
    pub no_hooks: bool,
}

#[derive(Serialize, Default)]
pub struct InitReport {
    pub created: Vec<String>,
    pub updated: Vec<String>,
    pub unchanged: Vec<String>,
    pub warnings: Vec<String>,
}

pub const CONFIG_TEMPLATE: &str = r#"# InLoop project config. Format: docs/07-config.md in the InLoop docs.

[project]
# name = "my-project"        # defaults to the repo directory name
# main_branch = "main"       # defaults to origin/HEAD, else main

[decisions]
dir = "docs/decisions"
id_prefix = "D"
id_length = 4

[checks]
# Commands an agent runs before saying work is done.
commands = []

[sessions]
backend = "local"            # "local" or "s3"

[redaction]
known_value_files = [".env", ".env.*"]
personal_data = false

# The optional project manager layer. Off unless enabled.
[pm]
enabled = false
"#;

pub fn run(ctx: &Ctx, a: InitArgs) -> anyhow::Result<ExitCode> {
    let repo = inloop_git::Repo::discover(&ctx.dir)?;
    let root = repo.root.clone();
    let mut r = InitReport::default();

    let cfg_path = root.join(inloop_core::config::CONFIG_FILE);
    if cfg_path.exists() {
        r.unchanged.push("inloop.toml".into());
    } else {
        std::fs::write(&cfg_path, CONFIG_TEMPLATE).context("writing inloop.toml")?;
        r.created.push("inloop.toml".into());
    }
    let cfg = inloop_core::ProjectConfig::load(&root)?;
    let dec_dir = root.join(&cfg.decisions.dir);
    if dec_dir.is_dir() {
        r.unchanged.push(cfg.decisions.dir.clone());
    } else {
        std::fs::create_dir_all(&dec_dir).context("creating the decisions directory")?;
        std::fs::write(dec_dir.join(".gitkeep"), "").context("creating .gitkeep")?;
        r.created.push(cfg.decisions.dir.clone());
    }

    if !a.no_mcp {
        register_mcp(&root, &mut r)?;
    }
    if !a.no_hooks {
        crate::hooks::install(&repo, &mut r)?;
    }

    ctx.emit(&r, |s| {
        let mut out = String::new();
        for c in &r.created {
            out.push_str(&format!("{} created {c}\n", s.green("+")));
        }
        for c in &r.updated {
            out.push_str(&format!("{} updated {c}\n", s.yellow("~")));
        }
        for c in &r.unchanged {
            out.push_str(&format!("{} {c} already set up\n", s.dim("=")));
        }
        for w in &r.warnings {
            out.push_str(&format!("{} {w}\n", s.yellow("!")));
        }
        out.push_str(
            "\nNext: `inloop dec new` to record a decision, `inloop lint` to check them.\n",
        );
        out
    })?;
    Ok(ExitCode::SUCCESS)
}

/// Add `inloop mcp` to the project's `.mcp.json`, keeping other servers.
fn register_mcp(root: &Path, r: &mut InitReport) -> anyhow::Result<()> {
    let path = root.join(".mcp.json");
    let mut doc: serde_json::Value = match std::fs::read_to_string(&path) {
        Ok(t) => serde_json::from_str(&t)
            .context(".mcp.json is not valid JSON; fix it and run init again")?,
        Err(_) => serde_json::json!({}),
    };
    let existed = path.exists();
    let want = serde_json::json!({ "command": "inloop", "args": ["mcp"] });
    let Some(obj) = doc.as_object_mut() else {
        anyhow::bail!(".mcp.json is not a JSON object");
    };
    let servers = obj
        .entry("mcpServers")
        .or_insert_with(|| serde_json::json!({}));
    let Some(servers) = servers.as_object_mut() else {
        anyhow::bail!(".mcp.json: mcpServers is not an object");
    };
    if servers.get("inloop") == Some(&want) {
        r.unchanged.push(".mcp.json".into());
        return Ok(());
    }
    servers.insert("inloop".into(), want);
    std::fs::write(&path, serde_json::to_string_pretty(&doc)? + "\n")
        .context("writing .mcp.json")?;
    if existed {
        r.updated.push(".mcp.json".into());
    } else {
        r.created.push(".mcp.json".into());
    }
    Ok(())
}
