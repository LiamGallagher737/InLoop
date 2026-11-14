//! Tool names, descriptions and schemas are snapshot-tested so any change
//! shows up in review (docs/06).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use inloop_mcp::InLoopServer;

#[test]
fn core_toolset_snapshot() {
    let server = InLoopServer::with_pm(std::env::temp_dir(), false);
    let tools: Vec<serde_json::Value> = server
        .tools()
        .into_iter()
        .map(|t| {
            serde_json::json!({
                "name": t.name,
                "description": t.description,
                "input_schema": serde_json::Value::Object(t.input_schema.as_ref().clone()),
            })
        })
        .collect();
    insta::assert_json_snapshot!(tools);
}

#[test]
fn core_tools_never_mention_a_project_manager() {
    let server = InLoopServer::with_pm(std::env::temp_dir(), false);
    let text = serde_json::to_string(&server.tools())
        .unwrap()
        .to_lowercase();
    for word in ["board", "child session", "project manager", "pm ", "task_"] {
        assert!(!text.contains(word), "core tools mention {word:?}");
    }
}

#[test]
fn pm_toolset_only_when_enabled_snapshot() {
    let core = InLoopServer::with_pm(std::env::temp_dir(), false).tool_names();
    let all = InLoopServer::with_pm(std::env::temp_dir(), true);
    let names = all.tool_names();
    for t in [
        "board_list",
        "board_check",
        "board_fold",
        "task_get",
        "task_new",
        "task_update",
        "task_brief",
        "task_report",
        "task_blocked",
    ] {
        assert!(names.contains(&t.to_string()), "{t}");
        assert!(!core.contains(&t.to_string()), "{t}");
    }
    let pm: Vec<serde_json::Value> = all
        .tools()
        .into_iter()
        .filter(|t| !core.contains(&t.name.to_string()))
        .map(|t| serde_json::json!({"name": t.name, "description": t.description, "input_schema": serde_json::Value::Object(t.input_schema.as_ref().clone())}))
        .collect();
    insta::assert_json_snapshot!(pm);
}
