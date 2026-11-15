//! The MCP server over real stdio, speaking JSON-RPC as Claude Code does.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use common::Repo;
use serde_json::{Value, json};

struct Mcp {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next: u64,
}

impl Mcp {
    fn start(r: &Repo) -> Mcp {
        let mut child = Command::new(env!("CARGO_BIN_EXE_inloop"))
            .arg("mcp")
            .current_dir(&r.root)
            .env("HOME", &r.home)
            .env("INLOOP_CONFIG_DIR", r.home.join("config"))
            .env("INLOOP_DATA_DIR", r.home.join("data"))
            .env("INLOOP_SESSION_ID", "sess-1234")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let mut m = Mcp {
            child,
            stdin,
            stdout,
            next: 1,
        };
        let init = m.request(
            "initialize",
            json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "test", "version": "0"}
            }),
        );
        assert!(
            init["result"]["instructions"]
                .as_str()
                .unwrap()
                .contains("provenance")
        );
        m.send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
        m
    }

    fn send(&mut self, v: Value) {
        writeln!(self.stdin, "{v}").unwrap();
        self.stdin.flush().unwrap();
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next;
        self.next += 1;
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        loop {
            let mut line = String::new();
            assert!(
                self.stdout.read_line(&mut line).unwrap() > 0,
                "server closed"
            );
            let v: Value = serde_json::from_str(&line).unwrap();
            if v["id"] == json!(id) {
                return v;
            }
        }
    }

    /// Call a tool; returns (is_error, parsed JSON or text).
    fn call(&mut self, name: &str, args: Value) -> (bool, Value) {
        let v = self.request("tools/call", json!({"name": name, "arguments": args}));
        let result = &v["result"];
        let text = result["content"][0]["text"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let is_error = result["isError"].as_bool().unwrap_or(false);
        let parsed = serde_json::from_str(&text).unwrap_or(Value::String(text));
        (is_error, parsed)
    }
}

impl Drop for Mcp {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

#[test]
fn core_tools_work_over_stdio() {
    let r = Repo::new();
    let mut m = Mcp::start(&r);

    let tools = m.request("tools/list", json!({}));
    let names: Vec<&str> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    for want in [
        "decision_search",
        "decision_get",
        "decision_list",
        "decision_new",
        "decision_supersede",
        "search",
        "context",
    ] {
        assert!(names.contains(&want), "{want} missing from {names:?}");
    }
    // The core toolset never assumes a project manager (D-N6YH).
    let all = tools["result"]["tools"].to_string().to_lowercase();
    for word in ["board", "child session", "project manager", " pm "] {
        assert!(!all.contains(word), "core tools mention {word:?}");
    }

    // Accepted without basis is refused with a message the agent can act on.
    let (err, msg) = m.call(
        "decision_new",
        json!({"title": "Pick sqlite", "context": "c", "decision": "d", "consequences": "e", "status": "accepted"}),
    );
    assert!(err);
    assert!(msg.as_str().unwrap().contains("basis"));

    let (err, w) = m.call(
        "decision_new",
        json!({"title": "Pick sqlite for the cache", "context": "Need a cache.", "decision": "SQLite.", "consequences": "One file."}),
    );
    assert!(!err, "{w}");
    let id = w["id"].as_str().unwrap().to_string();

    let (_, d) = m.call("decision_get", json!({"id": id}));
    assert_eq!(d["status"], "proposed");
    assert_eq!(d["session"], "sess-1234");
    assert_eq!(d["provenance"]["state"], "unmerged");

    let (_, hits) = m.call("decision_search", json!({"query": "sqlite cache"}));
    assert_eq!(hits[0]["id"], id.as_str());
    assert_eq!(hits[0]["provenance"]["state"], "unmerged");

    let (_, list) = m.call("decision_list", json!({"status": "proposed"}));
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert!(list[0].get("body").is_none());

    let (err, _) = m.call(
        "decision_accept",
        json!({"id": id, "basis": "Liam: \"yes, sqlite\""}),
    );
    assert!(!err);
    let (_, d) = m.call("decision_get", json!({"id": id}));
    assert_eq!(d["status"], "accepted");
    assert_eq!(d["promoted_by"], "agent");

    let (_, ctx) = m.call("context", json!({}));
    assert_eq!(ctx["branch"], "main");
    assert_eq!(ctx["on_main"], true);
}

#[test]
fn decisions_accept_in_one_call() {
    let r = Repo::new();
    let mut m = Mcp::start(&r);
    let new = |m: &mut Mcp, title: &str| {
        let (err, w) = m.call(
            "decision_new",
            json!({"title": title, "context": "c", "decision": "d", "consequences": "e"}),
        );
        assert!(!err, "{w}");
        w["id"].as_str().unwrap().to_string()
    };
    let (a, b, c) = (new(&mut m, "One"), new(&mut m, "Two"), new(&mut m, "Three"));
    let bare = |id: &str| id.trim_start_matches("D-").to_string();

    // A wrong ID in the batch changes nothing.
    let (err, msg) = m.call(
        "decision_accept",
        json!({"ids": [a, "ZZZZ"], "basis": "Liam: \"all of them\""}),
    );
    assert!(err);
    assert!(msg.as_str().unwrap().contains("nothing changed"), "{msg}");
    let (_, d) = m.call("decision_get", json!({"ids": [bare(&a)]}));
    assert_eq!(d["status"], "proposed");

    // Accept two in one call, one without its prefix.
    let (err, out) = m.call(
        "decision_accept",
        json!({"ids": [a, bare(&b)], "basis": "Liam: \"accept both\""}),
    );
    assert!(!err, "{out}");
    assert_eq!(out["done"].as_array().unwrap().len(), 2, "{out}");
    assert!(out["failed"].as_array().unwrap().is_empty());

    // Getting several returns them all; one already accepted fails alone.
    let (_, got) = m.call("decision_get", json!({"ids": [a, b, c]}));
    let statuses: Vec<&str> = got["done"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["status"].as_str().unwrap())
        .collect();
    assert_eq!(statuses, ["accepted", "accepted", "proposed"]);
    let (err, out) = m.call("decision_reject", json!({"ids": [a, c]}));
    assert!(!err, "{out}");
    assert_eq!(out["done"][0]["id"], c.as_str());
    assert_eq!(out["failed"][0]["id"], a.as_str());
}
