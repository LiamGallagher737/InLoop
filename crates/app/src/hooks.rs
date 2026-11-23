//! Installing Claude Code hooks (session capture) and the git
//! `prepare-commit-msg` hook (the `Session:` trailer). Both merge with what
//! is already there and never remove anything else.

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Value, json};

use crate::project::AppError;

/// The command Claude Code runs for every InLoop hook. The settings file is
/// committed, so the guard keeps Claude Code quiet for anyone working in the
/// repo who hasn't installed InLoop (D-KG5E). `inloop session hook` always
/// exits successfully, so `|| true` hides nothing.
pub const HOOK_COMMAND: &str = "command -v inloop >/dev/null 2>&1 && inloop session hook || true";

/// What InLoop's hooks ran before the guard. `init` upgrades it in place.
const LEGACY_HOOK_COMMAND: &str = "inloop session hook";

/// Claude Code hook events InLoop listens to.
pub const HOOK_EVENTS: &[&str] = &["SessionStart", "Stop", "PreCompact", "SessionEnd"];

/// The line InLoop adds to the git hook.
pub const GIT_HOOK_LINE: &str =
    "command -v inloop >/dev/null 2>&1 && inloop session trailer \"$1\" \"$2\" || true # inloop";

#[derive(Clone, Debug, Default, Serialize)]
pub struct HookReport {
    pub created: Vec<String>,
    pub updated: Vec<String>,
    pub unchanged: Vec<String>,
    pub warnings: Vec<String>,
}

fn settings_path(root: &Path) -> PathBuf {
    root.join(".claude").join("settings.json")
}

fn has_our_hook(groups: &Value) -> bool {
    groups.as_array().is_some_and(|gs| {
        gs.iter().any(|g| {
            g.get("hooks").and_then(Value::as_array).is_some_and(|hs| {
                hs.iter().any(|h| {
                    h.get("command")
                        .and_then(Value::as_str)
                        .is_some_and(|c| c.contains(LEGACY_HOOK_COMMAND))
                })
            })
        })
    })
}

/// True if every InLoop Claude Code hook is in the project's settings.
pub fn claude_hooks_installed(root: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(settings_path(root)) else {
        return false;
    };
    let Ok(v) = serde_json::from_str::<Value>(&text) else {
        return false;
    };
    HOOK_EVENTS.iter().all(|e| {
        v.get("hooks")
            .and_then(|h| h.get(*e))
            .is_some_and(has_our_hook)
    })
}

/// Replace the unguarded command from older installs with [`HOOK_COMMAND`].
fn upgrade_legacy(groups: &mut Value) -> bool {
    let mut changed = false;
    for g in groups.as_array_mut().into_iter().flatten() {
        let Some(hs) = g.get_mut("hooks").and_then(Value::as_array_mut) else {
            continue;
        };
        for h in hs {
            if h.get("command").and_then(Value::as_str).map(str::trim) == Some(LEGACY_HOOK_COMMAND)
            {
                h["command"] = json!(HOOK_COMMAND);
                changed = true;
            }
        }
    }
    changed
}

/// Add InLoop's hooks to `.claude/settings.json`, keeping everything else.
pub fn install_claude_hooks(root: &Path, report: &mut HookReport) -> Result<(), AppError> {
    let path = settings_path(root);
    let existed = path.exists();
    let existing = std::fs::read_to_string(&path).ok();
    let mut doc: Value = match &existing {
        Some(t) => serde_json::from_str(t).map_err(|e| {
            AppError::Invalid(format!(
                ".claude/settings.json is not valid JSON ({e}); fix it and run init again"
            ))
        })?,
        None => json!({}),
    };
    let obj = doc
        .as_object_mut()
        .ok_or_else(|| AppError::Invalid(".claude/settings.json is not a JSON object".into()))?;
    let hooks = obj.entry("hooks").or_insert_with(|| json!({}));
    let hooks = hooks
        .as_object_mut()
        .ok_or_else(|| AppError::Invalid(".claude/settings.json: hooks is not an object".into()))?;
    let mut changed = false;
    for event in HOOK_EVENTS {
        let groups = hooks.entry(event.to_string()).or_insert_with(|| json!([]));
        changed |= upgrade_legacy(groups);
        if has_our_hook(groups) {
            continue;
        }
        let Some(arr) = groups.as_array_mut() else {
            report.warnings.push(format!(
                ".claude/settings.json: hooks.{event} is not a list; left alone"
            ));
            continue;
        };
        let mut hook = json!({ "type": "command", "command": HOOK_COMMAND, "timeout": 30 });
        if *event == "SessionStart" {
            hook["timeout"] = json!(10);
        }
        arr.push(json!({ "hooks": [hook] }));
        changed = true;
    }
    let name = ".claude/settings.json".to_string();
    if !changed {
        report.unchanged.push(name);
        return Ok(());
    }
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).map_err(|e| AppError::io("creating .claude", e))?;
    }
    let text = crate::json::to_string_like(&doc, existing.as_deref())
        .map_err(|e| AppError::Other(e.to_string()))?;
    std::fs::write(&path, text).map_err(|e| AppError::io("writing .claude/settings.json", e))?;
    if existed {
        report.updated.push(name)
    } else {
        report.created.push(name)
    }
    Ok(())
}

/// Add the `Session:` trailer line to the repo's `prepare-commit-msg` hook.
pub fn install_git_hook(repo: &inloop_git::Repo, report: &mut HookReport) -> Result<(), AppError> {
    let hooks_dir = repo
        .run(&["rev-parse", "--git-path", "hooks"])
        .map(|s| s.trim().to_string())?;
    let hooks_dir = if Path::new(&hooks_dir).is_absolute() {
        PathBuf::from(hooks_dir)
    } else {
        repo.root.join(hooks_dir)
    };
    let path = hooks_dir.join("prepare-commit-msg");
    let name = "git prepare-commit-msg hook".to_string();
    match std::fs::read_to_string(&path) {
        Ok(text) if text.contains("inloop session trailer") => {
            report.unchanged.push(name);
            return Ok(());
        }
        Ok(text) => {
            let first = text.lines().next().unwrap_or_default();
            if !(first.starts_with("#!")
                && (first.contains("sh") || first.contains("bash") || first.contains("zsh")))
            {
                report.warnings.push(format!(
                    "{} is not a shell script; add this line to it yourself: {GIT_HOOK_LINE}",
                    path.display()
                ));
                return Ok(());
            }
            let mut text = text;
            if !text.ends_with('\n') {
                text.push('\n');
            }
            text.push_str(GIT_HOOK_LINE);
            text.push('\n');
            std::fs::write(&path, text).map_err(|e| AppError::io("updating the git hook", e))?;
            report.updated.push(name);
        }
        Err(_) => {
            std::fs::create_dir_all(&hooks_dir)
                .map_err(|e| AppError::io("creating the git hooks dir", e))?;
            std::fs::write(&path, format!("#!/bin/sh\n{GIT_HOOK_LINE}\n"))
                .map_err(|e| AppError::io("writing the git hook", e))?;
            report.created.push(name);
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_into_existing_settings() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join(".claude")).unwrap();
        std::fs::write(
            root.join(".claude/settings.json"),
            r#"{"permissions":{"allow":["Bash(ls)"]},"hooks":{"Stop":[{"hooks":[{"type":"command","command":"say done"}]}]}}"#,
        )
        .unwrap();
        let mut r = HookReport::default();
        install_claude_hooks(root, &mut r).unwrap();
        assert_eq!(r.updated, vec![".claude/settings.json"]);
        assert!(claude_hooks_installed(root));
        let v: Value = serde_json::from_str(
            &std::fs::read_to_string(root.join(".claude/settings.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(v["permissions"]["allow"][0], "Bash(ls)");
        assert_eq!(v["hooks"]["Stop"].as_array().unwrap().len(), 2);
        // Idempotent.
        let mut r = HookReport::default();
        install_claude_hooks(root, &mut r).unwrap();
        assert_eq!(r.unchanged, vec![".claude/settings.json"]);
    }

    #[test]
    fn upgrades_unguarded_hooks() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join(".claude")).unwrap();
        let old = HOOK_EVENTS
            .iter()
            .map(|e| format!(r#""{e}":[{{"hooks":[{{"type":"command","command":"inloop session hook","timeout":30}}]}}]"#))
            .collect::<Vec<_>>()
            .join(",");
        std::fs::write(
            root.join(".claude/settings.json"),
            format!(r#"{{"hooks":{{{old}}}}}"#),
        )
        .unwrap();
        let mut r = HookReport::default();
        install_claude_hooks(root, &mut r).unwrap();
        assert_eq!(r.updated, vec![".claude/settings.json"]);
        let text = std::fs::read_to_string(root.join(".claude/settings.json")).unwrap();
        let v: Value = serde_json::from_str(&text).unwrap();
        for e in HOOK_EVENTS {
            let groups = v["hooks"][*e].as_array().unwrap();
            assert_eq!(groups.len(), 1, "{e}");
            assert_eq!(groups[0]["hooks"][0]["command"], HOOK_COMMAND, "{e}");
        }
    }

    #[test]
    fn upgrade_keeps_the_files_indentation() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join(".claude")).unwrap();
        let groups = |cmd: &str| {
            HOOK_EVENTS
                .iter()
                .map(|e| {
                    format!(
                        "\t\t\"{e}\": [\n\t\t\t{{\n\t\t\t\t\"hooks\": [\n\t\t\t\t\t{{\n\t\t\t\t\t\t\"type\": \"command\",\n\t\t\t\t\t\t\"command\": {}\n\t\t\t\t\t}}\n\t\t\t\t]\n\t\t\t}}\n\t\t]",
                        serde_json::to_string(cmd).unwrap()
                    )
                })
                .collect::<Vec<_>>()
                .join(",\n")
        };
        let file = |cmd: &str| format!("{{\n\t\"hooks\": {{\n{}\n\t}}\n}}\n", groups(cmd));
        std::fs::write(
            root.join(".claude/settings.json"),
            file(LEGACY_HOOK_COMMAND),
        )
        .unwrap();
        let mut r = HookReport::default();
        install_claude_hooks(root, &mut r).unwrap();
        let text = std::fs::read_to_string(root.join(".claude/settings.json")).unwrap();
        assert_eq!(text, file(HOOK_COMMAND));
    }
}
