//! Tool names, descriptions and schemas are snapshot-tested so any change
//! shows up in review (docs/06).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use inloop_mcp::InLoopServer;

#[test]
fn core_toolset_snapshot() {
    let server = InLoopServer::new(std::env::temp_dir());
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
    let server = InLoopServer::new(std::env::temp_dir());
    let text = serde_json::to_string(&server.tools())
        .unwrap()
        .to_lowercase();
    for word in ["board", "child session", "project manager", "pm ", "task_"] {
        assert!(!text.contains(word), "core tools mention {word:?}");
    }
}
