//! End-to-end tests of the `inloop` binary against throwaway repos.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{Repo, stderr, stdout};

#[test]
fn records_accepts_and_supersedes_decisions() {
    let r = Repo::new();

    // Accepted needs a basis.
    let out = r.run(&["dec", "new", "--title", "Use tabs", "--status", "accepted"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("basis"), "{}", stderr(&out));

    // A proposal.
    let w = r.json(&[
        "dec",
        "new",
        "--title",
        "Use tabs",
        "--context",
        "Spaces vs tabs.",
        "--decision",
        "Tabs.",
        "--consequences",
        "Wider diffs.",
        "--tag",
        "style",
    ]);
    let a = w["id"].as_str().unwrap().to_string();
    assert!(a.starts_with("D-") && a.len() == 6);
    let d = r.json(&["dec", "show", &a]);
    assert_eq!(d["status"], "proposed");
    assert_eq!(d["decided_by"], "agent");
    assert_eq!(d["provenance"]["state"], "unmerged");
    assert_eq!(d["provenance"]["uncommitted"], true);

    // A human accepts it.
    r.json(&["dec", "accept", &a, "--basis", "Tabs, final answer."]);
    let d = r.json(&["dec", "show", &a]);
    assert_eq!(d["status"], "accepted");
    assert_eq!(d["basis"], "Tabs, final answer.");
    assert_eq!(d["decided_by"], "Test Person");
    assert_eq!(d["promoted_by"], "Test Person");

    // Commit: now it's merged.
    r.git(&["add", "."]);
    r.git(&["commit", "-q", "-m", "tabs"]);
    let d = r.json(&["dec", "show", &a]);
    assert_eq!(d["provenance"], serde_json::json!({"state": "merged"}));

    // A replacement, accepted at creation, superseding the first.
    let w = r.json(&[
        "dec",
        "new",
        "--title",
        "Use spaces after all",
        "--basis",
        "Spaces. Sorry.",
        "--supersedes",
        &a,
    ]);
    let b = w["id"].as_str().unwrap().to_string();
    let old = r.json(&["dec", "show", &a]);
    assert_eq!(old["status"], "superseded");
    assert_eq!(old["superseded_by"], b.as_str());
    let new = r.json(&["dec", "show", &b]);
    assert_eq!(new["supersedes"], serde_json::json!([a]));

    // Lists, search and lint all agree.
    let list = r.json(&["dec", "list", "--status", "superseded"]);
    assert_eq!(list.as_array().unwrap().len(), 1);
    let hits = r.json(&["search", "spaces"]);
    assert_eq!(hits[0]["id"], b.as_str());
    let lint = r.json(&["lint"]);
    assert_eq!(lint["errors"], 0, "{lint}");

    // Human output.
    let out = r.run(&["dec", "list"]);
    let text = stdout(&out);
    assert!(
        text.contains("Use spaces after all") && text.contains("NOT COMMITTED"),
        "{text}"
    );
}

#[test]
fn sees_decisions_in_other_worktrees_and_flags_collisions() {
    let r = Repo::new();
    let wt = r.root.parent().unwrap().join("elsewhere").join("feature");
    r.git(&[
        "worktree",
        "add",
        "-q",
        "-b",
        "feature",
        wt.to_str().unwrap(),
    ]);
    let out = r.run_in(&wt, &["--json", "dec", "new", "--title", "Feature choice"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let id = serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Seen from main, marked as being elsewhere.
    let d = r.json(&["dec", "show", &id]);
    assert_eq!(d["provenance"]["state"], "unmerged");
    assert_eq!(d["provenance"]["branch"], "feature");
    assert!(
        d["provenance"]["worktree"]
            .as_str()
            .unwrap()
            .ends_with("elsewhere/feature")
    );
    let merged_only = r.json(&["dec", "list", "--merged"]);
    assert!(merged_only.as_array().unwrap().is_empty());

    // Accepting it from main is refused: it lives in the other worktree.
    let out = r.run(&["dec", "accept", &id, "--basis", "ok"]);
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("not in this worktree"),
        "{}",
        stderr(&out)
    );

    // Same ID, different decision, on main: lint catches it.
    let dir = r.root.join("docs/decisions");
    let text = std::fs::read_to_string(
        std::fs::read_dir(wt.join("docs/decisions"))
            .unwrap()
            .filter_map(|e| e.ok())
            .find(|e| e.file_name().to_string_lossy().starts_with(&id))
            .unwrap()
            .path(),
    )
    .unwrap()
    .replace("Feature choice", "Something else entirely");
    std::fs::write(dir.join(format!("{id}-something-else-entirely.md")), text).unwrap();
    let out = r.run(&["lint"]);
    assert!(!out.status.success());
    assert!(stdout(&out).contains("duplicate-id"), "{}", stdout(&out));
}

#[test]
fn init_registers_the_mcp_server_without_clobbering() {
    let r = Repo::new();
    let mcp: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(r.root.join(".mcp.json")).unwrap()).unwrap();
    assert_eq!(
        mcp["mcpServers"]["inloop"]["args"],
        serde_json::json!(["mcp"])
    );
    std::fs::write(
        r.root.join(".mcp.json"),
        r#"{"mcpServers":{"other":{"command":"x"}}}"#,
    )
    .unwrap();
    let rep = r.json(&["init", "--no-hooks"]);
    assert_eq!(rep["updated"], serde_json::json!([".mcp.json"]));
    let mcp: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(r.root.join(".mcp.json")).unwrap()).unwrap();
    assert!(mcp["mcpServers"]["other"].is_object());
    assert!(mcp["mcpServers"]["inloop"].is_object());
}

#[test]
fn secrets_never_reach_a_decision_file() {
    let r = Repo::new();
    std::fs::write(
        r.root.join(".env"),
        "STRIPE_KEY=sk_live_51HxEXAMPLEEXAMPLEEXAMPLEab12\n",
    )
    .unwrap();
    let w = r.json(&[
        "dec",
        "new",
        "--title",
        "Billing uses the live key",
        "--context",
        "The key sk_live_51HxEXAMPLEEXAMPLEEXAMPLEab12 was pasted here.",
    ]);
    let text = std::fs::read_to_string(w["path"].as_str().unwrap()).unwrap();
    assert!(!text.contains("sk_live_51HxEXAMPLE"), "{text}");
    assert!(text.contains("[REDACTED:"), "{text}");
}
