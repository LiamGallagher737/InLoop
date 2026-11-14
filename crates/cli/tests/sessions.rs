//! Session capture end to end: hooks, redaction, encryption, search,
//! commit trailers, purge.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use common::{Repo, stderr};

const SECRET: &str = "sk_live_51HxEXAMPLEEXAMPLEEXAMPLEzz99";

fn transcript(cwd: &Path) -> String {
    let cwd = cwd.display();
    [
        format!(r#"{{"type":"user","message":{{"role":"user","content":"Wire up billing; the key is {SECRET}"}},"timestamp":"2026-11-14T10:00:00Z","cwd":"{cwd}"}}"#),
        r#"{"type":"file-history-snapshot","snapshot":{"x":"bookkeeping"}}"#.to_string(),
        format!(r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"tool_use","id":"t1","name":"Edit","input":{{"file_path":"{cwd}/src/billing.rs"}}}},{{"type":"tool_use","id":"t2","name":"Bash","input":{{"command":"cat .env"}}}}]}},"timestamp":"2026-11-14T10:01:00Z"}}"#),
        r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t2","content":"BILLING_PASSWORD=hunter2hunter2xyz"}]},"timestamp":"2026-11-14T10:01:01Z"}"#.to_string(),
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Billing is wired up with retries."}]},"timestamp":"2026-11-14T10:02:00Z"}"#.to_string(),
    ]
    .join("\n")
        + "\n"
}

fn bin_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_inloop"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn hook(r: &Repo, event: &str, session: &str, transcript: &Path) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_inloop"))
        .args(["session", "hook"])
        .current_dir(&r.root)
        .env("HOME", &r.home)
        .env("INLOOP_CONFIG_DIR", r.home.join("config"))
        .env("INLOOP_DATA_DIR", r.home.join("data"))
        .env("INLOOP_CAPTURE_SYNC", "1")
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    let input = serde_json::json!({
        "session_id": session,
        "transcript_path": transcript,
        "cwd": r.root,
        "hook_event_name": event,
    });
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.to_string().as_bytes())
        .unwrap();
    assert!(child.wait().unwrap().success());
}

fn all_bytes_under(dir: &Path) -> Vec<u8> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                out.extend(all_bytes_under(&p));
            } else if let Ok(b) = std::fs::read(&p) {
                out.extend(b);
            }
        }
    }
    out
}

fn contains(hay: &[u8], needle: &str) -> bool {
    hay.windows(needle.len()).any(|w| w == needle.as_bytes())
}

#[test]
fn captures_redacts_encrypts_searches_and_purges() {
    let r = Repo::new();
    std::fs::write(r.root.join(".env"), "BILLING_PASSWORD=hunter2hunter2xyz\n").unwrap();
    let tdir = r.home.join(".claude/projects/x");
    std::fs::create_dir_all(&tdir).unwrap();
    let tpath = tdir.join("abc123.jsonl");
    std::fs::write(&tpath, transcript(&r.root)).unwrap();

    hook(&r, "SessionStart", "abc123", &tpath);
    hook(&r, "Stop", "abc123", &tpath);

    // Stored, and nothing on disk is plaintext or holds a secret.
    let data = all_bytes_under(&r.home.join("data"));
    assert!(!data.is_empty());
    assert!(!contains(&data, SECRET));
    assert!(!contains(&data, "Wire up billing"));
    assert!(r.home.join("config/keys").exists());

    let list = r.json(&["session", "list"]);
    assert_eq!(list.as_array().unwrap().len(), 1);
    let s = &list[0];
    assert_eq!(s["id"], "abc123");
    assert!(
        s["summary"]["asked"]
            .as_str()
            .unwrap()
            .starts_with("Wire up billing")
    );
    assert!(!s.to_string().contains(SECRET));
    assert_eq!(
        s["summary"]["files_touched"],
        serde_json::json!(["src/billing.rs"])
    );
    assert_eq!(s["summary"]["done"], "Billing is wired up with retries.");

    // The transcript: redacted, `cat .env` output dropped, bookkeeping gone.
    let full = r.json(&["session", "show", "abc", "--transcript"]);
    let t = full["transcript"].as_str().unwrap();
    assert!(!t.contains(SECRET), "{t}");
    assert!(!t.contains("hunter2hunter2xyz"), "{t}");
    assert!(t.contains("[REDACTED:command-output]"), "{t}");
    assert!(!t.contains("file-history-snapshot"));

    // Search finds it; a second Stop with no change is skipped.
    let hits = r.json(&["session", "search", "billing"]);
    assert_eq!(hits[0]["id"], "abc123");
    let again = r.json(&[
        "session",
        "capture",
        "--transcript",
        tpath.to_str().unwrap(),
        "--session",
        "abc123",
    ]);
    assert_eq!(again["captured"], false);

    // The index can be rebuilt from the store alone.
    std::fs::remove_file(r.root.join(".git/inloop/index.sqlite")).unwrap();
    let synced = r.json(&["session", "sync"]);
    assert_eq!(synced["indexed"], 1);

    // Scan finds nothing left to fix.
    let scan = r.json(&["scan"]);
    assert_eq!(scan["sessions_checked"], 1);
    assert!(scan["findings"].as_array().unwrap().is_empty(), "{scan}");

    // Purge really deletes.
    r.json(&["session", "purge", "abc123"]);
    assert!(r.json(&["session", "list"]).as_array().unwrap().is_empty());
    let files: Vec<_> = walk(&r.home.join("data"));
    assert!(files.is_empty(), "{files:?}");
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                out.extend(walk(&p));
            } else {
                out.push(p);
            }
        }
    }
    out
}

#[test]
fn commits_and_decisions_link_to_the_running_session() {
    let r = Repo::new();
    let out = r.run(&["init"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let settings = std::fs::read_to_string(r.root.join(".claude/settings.json")).unwrap();
    assert!(settings.contains("inloop session hook"));

    let tdir = r.home.join(".claude/projects/x");
    std::fs::create_dir_all(&tdir).unwrap();
    let tpath = tdir.join("sess42.jsonl");
    std::fs::write(&tpath, transcript(&r.root)).unwrap();
    hook(&r, "SessionStart", "sess42", &tpath);

    // A decision recorded now names the session.
    let w = r.json(&["dec", "new", "--title", "Retry billing calls"]);
    let d = r.json(&["dec", "show", w["id"].as_str().unwrap()]);
    assert_eq!(d["session"], "sess42");

    // A commit gets the trailer (the git hook finds inloop on PATH).
    std::fs::write(r.root.join("x.txt"), "x").unwrap();
    let path = format!(
        "{}:{}",
        bin_dir().display(),
        std::env::var("PATH").unwrap_or_default()
    );
    for args in [&["add", "."][..], &["commit", "-q", "-m", "Add x"][..]] {
        let out = Command::new("git")
            .arg("-C")
            .arg(&r.root)
            .args(args)
            .env("PATH", &path)
            .env("HOME", &r.home)
            .env("INLOOP_CONFIG_DIR", r.home.join("config"))
            .env("INLOOP_DATA_DIR", r.home.join("data"))
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let msg = Command::new("git")
        .arg("-C")
        .arg(&r.root)
        .args(["log", "-1", "--format=%B"])
        .output()
        .unwrap();
    let msg = String::from_utf8_lossy(&msg.stdout);
    assert!(msg.contains("Session: sess42"), "{msg}");

    // Capture links the commit and the decision.
    hook(&r, "Stop", "sess42", &tpath);
    let s = r.json(&["session", "show", "sess42"]);
    assert_eq!(s["commits"].as_array().unwrap().len(), 1);
    assert_eq!(s["summary"]["decisions"], serde_json::json!([w["id"]]));

    // SessionEnd clears the active session: later decisions aren't linked.
    hook(&r, "SessionEnd", "sess42", &tpath);
    let w2 = r.json(&["dec", "new", "--title", "Unrelated later choice"]);
    let d2 = r.json(&["dec", "show", w2["id"].as_str().unwrap()]);
    assert!(d2["session"].is_null(), "{d2}");

    let st = r.json(&["session", "status"]);
    assert_eq!(st["hooks_installed"], true);
    assert_eq!(st["captured"], 1);
}

#[test]
fn keys_export_and_import() {
    let r = Repo::new();
    let shown = r.json(&["key", "show"]);
    assert!(shown["recipient"].as_str().unwrap().starts_with("age1"));
    let file = r.home.join("shared.key");
    r.json(&["key", "export", file.to_str().unwrap()]);
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains("AGE-SECRET-KEY-1"));
    // Importing over an existing key is refused.
    let out = r.run(&["key", "import", file.to_str().unwrap()]);
    assert!(!out.status.success());
    // A co-founder with no key can import it.
    std::fs::remove_dir_all(r.home.join("config/keys")).unwrap();
    let imported = r.json(&["key", "import", file.to_str().unwrap()]);
    assert_eq!(imported["recipient"], shown["recipient"]);
}
