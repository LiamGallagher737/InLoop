//! InLoop's MCP server (docs/06). The core toolset is always registered and
//! never mentions a project manager (D-N6YH). Every decision and session
//! result carries provenance (D-E1S7).
#![forbid(unsafe_code)]

mod core_tools;
mod pm_tools;

use std::path::PathBuf;

use inloop_app::{AppError, Project};
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::model::{CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerConfig};
use rmcp::{ServerHandler, ServiceExt, tool_handler};
use serde::{Deserialize, Deserializer, Serialize};

pub use core_tools::*;
pub use pm_tools::*;

/// Instructions sent to the agent when it connects.
pub const INSTRUCTIONS: &str = "InLoop records this project's decisions and its agent sessions. \
Search before you decide: call decision_search (or search) for anything a past decision may cover. \
Every result has provenance: state `merged` means it is on the main branch and settled; `unmerged` \
means it is work in progress on another branch or worktree, not settled fact. Record decisions with \
decision_new: a decision is `accepted` only when a human made it, with their words quoted in `basis`; \
anything you chose yourself is `proposed`.";

#[derive(Clone)]
pub struct InLoopServer {
    dir: PathBuf,
    tool_router: ToolRouter<Self>,
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for InLoopServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("inloop", env!("CARGO_PKG_VERSION")))
            .with_instructions(INSTRUCTIONS)
    }
}

impl InLoopServer {
    /// A server for the project containing `dir`. The PM toolset is added
    /// only when the project turns the PM layer on.
    pub fn new(dir: PathBuf) -> Self {
        let pm = Project::open(&dir).map(|p| p.pm_enabled()).unwrap_or(false);
        Self::with_pm(dir, pm)
    }

    /// A server with or without the PM toolset, whatever the config says.
    pub fn with_pm(dir: PathBuf, pm: bool) -> Self {
        let mut tool_router = Self::core_router();
        if pm {
            tool_router += Self::pm_router();
        }
        InLoopServer { dir, tool_router }
    }

    /// Names of every registered tool, for tests and `inloop mcp --list`.
    pub fn tool_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .tool_router
            .list_all()
            .into_iter()
            .map(|t| t.name.to_string())
            .collect();
        names.sort();
        names
    }

    /// Every registered tool, for snapshot tests of names, descriptions and
    /// schemas.
    pub fn tools(&self) -> Vec<rmcp::model::Tool> {
        let mut t = self.tool_router.list_all();
        t.sort_by(|a, b| a.name.cmp(&b.name));
        t
    }

    /// Run `f` against the project on a blocking thread, and turn the result
    /// into a tool result: JSON on success, an error result with a message
    /// the agent can act on otherwise.
    pub(crate) async fn with_project<T, F>(&self, f: F) -> CallToolResult
    where
        T: Serialize + Send + 'static,
        F: FnOnce(&Project) -> Result<T, AppError> + Send + 'static,
    {
        let dir = self.dir.clone();
        let res = tokio::task::spawn_blocking(move || {
            let p = Project::open(&dir)?;
            f(&p)
        })
        .await;
        match res {
            Ok(Ok(v)) => match serde_json::to_string(&v) {
                Ok(s) => CallToolResult::success(vec![ContentBlock::text(s)]),
                Err(e) => CallToolResult::error(vec![ContentBlock::text(format!("error: {e}"))]),
            },
            Ok(Err(e)) => CallToolResult::error(vec![ContentBlock::text(format!("error: {e}"))]),
            Err(e) => CallToolResult::error(vec![ContentBlock::text(format!("error: {e}"))]),
        }
    }
}

/// Deserialize `ids` from either one string or a list, so a call written
/// with a single `id` still works.
pub(crate) fn one_or_many<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        One(String),
        Many(Vec<String>),
    }
    Ok(match OneOrMany::deserialize(d)? {
        OneOrMany::One(s) => vec![s],
        OneOrMany::Many(v) => v,
    })
}

/// What a call with several IDs returns: what worked and what didn't.
#[derive(Serialize)]
struct Batch<T> {
    done: Vec<T>,
    failed: Vec<Failed>,
}

#[derive(Serialize)]
struct Failed {
    id: String,
    error: String,
}

impl InLoopServer {
    /// Run `f` for each of `ids`. With one ID the result is exactly what a
    /// single call returns. With several, `check` runs first (so a missing
    /// ID changes nothing), then every ID is tried and the result lists what
    /// was done and what failed; it is an error only if nothing worked.
    pub(crate) async fn with_project_each<T, C, F>(
        &self,
        ids: Vec<String>,
        check: C,
        f: F,
    ) -> CallToolResult
    where
        T: Serialize + Send + 'static,
        C: FnOnce(&Project, &[String]) -> Result<(), AppError> + Send + 'static,
        F: Fn(&Project, &str) -> Result<T, AppError> + Send + 'static,
    {
        match ids.as_slice() {
            [] => CallToolResult::error(vec![ContentBlock::text(
                "error: give at least one ID".to_string(),
            )]),
            [one] => {
                let one = one.clone();
                self.with_project(move |p| f(p, &one)).await
            }
            _ => {
                let dir = self.dir.clone();
                let res = tokio::task::spawn_blocking(move || {
                    let p = Project::open(&dir)?;
                    check(&p, &ids)?;
                    let mut b = Batch {
                        done: Vec::new(),
                        failed: Vec::new(),
                    };
                    for id in &ids {
                        match f(&p, id) {
                            Ok(v) => b.done.push(v),
                            Err(e) => b.failed.push(Failed {
                                id: id.clone(),
                                error: e.to_string(),
                            }),
                        }
                    }
                    Ok::<_, AppError>(b)
                })
                .await;
                let text = |s: String| vec![ContentBlock::text(s)];
                match res {
                    Ok(Ok(b)) => match serde_json::to_string(&b) {
                        Ok(s) if b.done.is_empty() => CallToolResult::error(text(s)),
                        Ok(s) => CallToolResult::success(text(s)),
                        Err(e) => CallToolResult::error(text(format!("error: {e}"))),
                    },
                    Ok(Err(e)) => CallToolResult::error(text(format!("error: {e}"))),
                    Err(e) => CallToolResult::error(text(format!("error: {e}"))),
                }
            }
        }
    }
}

/// Serve over stdio until the client disconnects.
pub async fn serve_stdio(dir: PathBuf) -> Result<(), String> {
    let server = InLoopServer::new(dir);
    let running = server
        .serve(rmcp::transport::stdio())
        .await
        .map_err(|e| e.to_string())?;
    running.waiting().await.map_err(|e| e.to_string())?;
    Ok(())
}

/// The async runtime the CLI uses to run the server.
pub type Runtime = tokio::runtime::Runtime;

pub fn runtime() -> std::io::Result<Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
}
