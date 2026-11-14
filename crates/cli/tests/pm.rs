//! The optional PM layer end to end: a whole batch, from proposing tasks to
//! landing them, with no hand-kept board.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{Repo, stderr, stdout};

fn enable_pm(r: &Repo) {
    let cfg = r.root.join("inloop.toml");
    let text = std::fs::read_to_string(&cfg)
        .unwrap()
        .replace("[checks]\n# Commands an agent runs before saying work is done.\ncommands = []", "[checks]\ncommands = [\"cargo test\"]")
        .replace("[pm]\nenabled = false", "[pm]\nenabled = true\n\n[[pm.conflict_groups]]\nname = \"manifests\"\npaths = [\"Cargo.toml\"]");
    std::fs::write(&cfg, text).unwrap();
    std::fs::create_dir_all(r.root.join("crates/core/src")).unwrap();
    std::fs::write(r.root.join("crates/core/src/lib.rs"), "// core\n").unwrap();
    std::fs::write(r.root.join("Cargo.toml"), "[workspace]\n").unwrap();
    r.git(&["add", "."]);
    r.git(&["commit", "-q", "-m", "pm on"]);
}

#[test]
fn pm_commands_refuse_when_the_layer_is_off() {
    let r = Repo::new();
    let out = r.run(&["board"]);
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("[pm] enabled = true"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn a_full_batch_runs_on_the_board() {
    let r = Repo::new();
    enable_pm(&r);
    let init = r.json(&["init", "--no-hooks"]);
    let created = init["created"].to_string();
    assert!(
        created.contains("SKILL.md") && created.contains(".inloop/brief.md"),
        "{init}"
    );

    std::fs::write(
        r.root.join(".env"),
        "PAYMENTS_TOKEN=tok_EXAMPLE_9f8e7d6c5b4a39281706\n",
    )
    .unwrap();
    let a = r.json(&[
        "task",
        "new",
        "Core types",
        "--touch",
        "crates/core/**",
        "--touch",
        "Cargo.toml",
        "--description",
        "Use tok_EXAMPLE_9f8e7d6c5b4a39281706 for the sandbox.",
    ]);
    let a_id = a["id"].as_str().unwrap().to_string();
    assert!(
        !a.to_string().contains("tok_EXAMPLE_9f8e"),
        "secret reached the board: {a}"
    );
    let b = r.json(&[
        "task",
        "new",
        "Core helper",
        "--touch",
        "crates/core/src/lib.rs",
    ]);
    let b_id = b["id"].as_str().unwrap().to_string();
    let c = r.json(&[
        "task",
        "new",
        "Docs for core",
        "--touch",
        "docs/**",
        "--blocked-by",
        &a_id,
    ]);
    let c_id = c["id"].as_str().unwrap().to_string();
    assert_eq!(c["status"], "blocked-task");

    // A and B conflict; A alone is fine.
    let out = r.run(&["--json", "task", "check", &a_id, &b_id]);
    assert!(!out.status.success());
    assert!(
        stdout(&out).contains("crates/core/src/lib.rs"),
        "{}",
        stdout(&out)
    );
    let ok = r.json(&["task", "check", &a_id]);
    assert_eq!(ok["ok"], true, "{ok}");

    // Start A on its own branch and worktree.
    let wt = r.root.parent().unwrap().join("wt-a");
    r.git(&[
        "worktree",
        "add",
        "-q",
        "-b",
        "task-a",
        wt.to_str().unwrap(),
    ]);
    let up = r.json(&[
        "task",
        "update",
        &a_id,
        "--status",
        "running",
        "--branch",
        "task-a",
        "--worktree",
        wt.to_str().unwrap(),
    ]);
    assert_eq!(up["status"], "running");
    let shown = r.json(&["task", "show", &a_id]);
    assert_eq!(shown["claims"].as_array().unwrap().len(), 1);

    // The brief.
    let brief = r.json(&["task", "brief", &a_id]);
    let text = brief["text"].as_str().unwrap();
    assert!(
        text.contains("Core types") && text.contains("cargo test") && text.contains("task_report"),
        "{text}"
    );

    // In A's worktree a decision is recorded; seen from main it names task A.
    let out = r.run_in(
        &wt,
        &[
            "--json",
            "dec",
            "new",
            "--title",
            "Core types are plain structs",
        ],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    let dec: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let d = r.json(&["dec", "show", dec["id"].as_str().unwrap()]);
    assert_eq!(d["provenance"]["task"], a_id.as_str(), "{d}");

    // The child reports from its worktree (task found from the branch).
    let out = r.run_in(
        &wt,
        &[
            "--json",
            "task",
            "report",
            "--summary",
            "Types done",
            "--done",
            "structs",
            "--passed",
            "cargo test",
            "--decision",
            dec["id"].as_str().unwrap(),
        ],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    // A second report before the PM gets to it: neither may be lost.
    let out = r.run_in(
        &wt,
        &[
            "--json",
            "task",
            "report",
            "--summary",
            "Also added docs comments",
        ],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    let board = r.json(&["board"]);
    assert_eq!(board["unfolded_reports"].as_array().unwrap().len(), 2);

    // The PM folds them in: A goes to review with the decision linked.
    let folded = r.json(&["board", "fold"]);
    assert_eq!(
        folded["reports_folded"].as_array().unwrap().len(),
        2,
        "{folded}"
    );
    let shown = r.json(&["task", "show", &a_id]);
    assert_eq!(shown["status"], "review");
    assert_eq!(shown["decisions"][0], dec["id"]);
    assert!(
        r.json(&["board"])["unfolded_reports"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    // Land it: C is newly unblocked.
    r.json(&["task", "update", &a_id, "--status", "merged"]);
    let board = r.json(&["board"]);
    assert_eq!(board["newly_unblocked"], serde_json::json!([c_id]));

    // B gets blocked on a human question.
    r.json(&[
        "task", "update", &b_id, "--status", "running", "--branch", "task-b",
    ]);
    let out = r.run(&[
        "--json",
        "task",
        "blocked",
        "--task",
        &b_id,
        "--kind",
        "human",
        "--on",
        "Which helper name?",
    ]);
    assert!(out.status.success(), "{}", stderr(&out));
    r.json(&["board", "fold"]);
    assert_eq!(r.json(&["task", "show", &b_id])["status"], "blocked-human");

    // Export for people.
    let md = r.json(&["board", "export"]);
    let md = md["markdown"].as_str().unwrap();
    assert!(
        md.contains("Core types") && md.contains("Docs for core"),
        "{md}"
    );

    // Scan covers the board, and finds nothing left.
    let scan = r.json(&["scan"]);
    assert!(
        scan["board_records_checked"].as_u64().unwrap() >= 4,
        "{scan}"
    );
    assert!(scan["findings"].as_array().unwrap().is_empty(), "{scan}");
}
