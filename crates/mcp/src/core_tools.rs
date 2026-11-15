//! The core toolset. Names and descriptions never mention a project
//! manager, a board or child sessions (D-N6YH); they are snapshot-tested.

use inloop_app::decisions::ListFilter;
use inloop_decisions::Status;
use inloop_decisions::ops::NewDecision;
use inloop_index::Kind;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::schemars::{self, JsonSchema};
use rmcp::{tool, tool_router};
use serde::{Deserialize, Serialize};

use crate::InLoopServer;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchParams {
    /// Words to search for. Every word must match, as a prefix; if nothing
    /// matches, any word may.
    pub query: String,
    /// Maximum results (default 10).
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct IdParams {
    /// One or more decision IDs such as D-7KQ2 (or 7KQ2: the prefix is optional), or old aliases such as D-264.
    #[serde(alias = "id", deserialize_with = "crate::one_or_many")]
    pub ids: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListParams {
    /// Only this status: proposed, accepted, rejected or superseded.
    pub status: Option<String>,
    /// Only decisions with this tag.
    pub tag: Option<String>,
    /// Only decisions not merged into the main branch yet.
    pub unmerged_only: Option<bool>,
    /// Only decisions dated on or after this day (YYYY-MM-DD).
    pub since: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct NewParams {
    /// One line, stated as the decision itself, e.g. "Snap writes go through one helper".
    pub title: String,
    /// Why this came up.
    pub context: String,
    /// What was decided, in a few sentences.
    pub decision: String,
    /// What follows from it, including what it rules out.
    pub consequences: String,
    /// "accepted" only when a human made the decision and `basis` quotes them. Otherwise
    /// leave it out (or "proposed").
    pub status: Option<String>,
    /// Required for accepted: the human's words, quoted, or a pointer to where they said it.
    pub basis: Option<String>,
    /// The human who made it, for accepted decisions.
    pub decided_by: Option<String>,
    /// Free-form tags for search.
    pub tags: Option<Vec<String>>,
    /// IDs of decisions this one replaces. They must be in this worktree.
    pub supersedes: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AcceptParams {
    /// The proposed decisions to accept, e.g. ["D-7KQ2", "P5PT"]. The prefix is optional.
    #[serde(alias = "id", deserialize_with = "crate::one_or_many")]
    pub ids: Vec<String>,
    /// The human's words accepting them, quoted.
    pub basis: String,
    /// The human who accepted it.
    pub decided_by: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RejectParams {
    /// The proposed decisions to reject. The prefix is optional.
    #[serde(alias = "id", deserialize_with = "crate::one_or_many")]
    pub ids: Vec<String>,
    /// The human's reason, quoted, if they gave one.
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SupersedeParams {
    /// The decision being replaced.
    pub old: String,
    /// The decision that replaces it.
    pub new: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SessionGetParams {
    /// Session ID, or a unique prefix of it.
    pub id: String,
    /// Include the full redacted transcript. Large: ask only when the summary isn't enough.
    pub transcript: Option<bool>,
    /// With `transcript`, skip this many characters of it (for paging).
    pub offset: Option<usize>,
    /// With `transcript`, return at most this many characters (default 40000).
    pub max_chars: Option<usize>,
}

/// A decision in a list: no body.
#[derive(Serialize)]
pub struct ListItem {
    pub id: String,
    pub title: String,
    pub status: Status,
    pub date: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    pub provenance: inloop_core::Provenance,
}

fn limit(l: Option<u32>) -> usize {
    l.map(|l| l.clamp(1, 100) as usize).unwrap_or(10)
}

#[tool_router(router = core_router, vis = "pub")]
impl InLoopServer {
    #[tool(
        description = "Search this project's decisions: on the main branch, in every worktree and on every unmerged branch. Use it before making a choice a past decision may cover. Results carry provenance: `unmerged` means work in progress elsewhere, not settled fact."
    )]
    pub async fn decision_search(&self, Parameters(p): Parameters<SearchParams>) -> CallToolResult {
        self.with_project(move |proj| proj.search(&p.query, &[Kind::Decision], limit(p.limit)))
            .await
    }

    #[tool(
        description = "Get decisions in full by ID (old aliases such as D-264 also work), with their provenance. Pass several IDs to get them all in one call."
    )]
    pub async fn decision_get(&self, Parameters(p): Parameters<IdParams>) -> CallToolResult {
        self.with_project_each(p.ids, |_, _| Ok(()), |proj, id| proj.decision_get(id))
            .await
    }

    #[tool(
        description = "List decisions, newest first, with optional filters. Includes decisions on other branches and worktrees, marked unmerged."
    )]
    pub async fn decision_list(&self, Parameters(p): Parameters<ListParams>) -> CallToolResult {
        self.with_project(move |proj| {
            let status = match p.status.as_deref() {
                None => None,
                Some(s) => Some(Status::parse(s).ok_or_else(|| {
                    inloop_app::AppError::Invalid(format!(
                        "status `{s}`: use proposed, accepted, rejected or superseded"
                    ))
                })?),
            };
            let items = proj.decision_list(&ListFilter {
                status,
                tag: p.tag,
                unmerged_only: p.unmerged_only.unwrap_or(false),
                since: p.since,
                merged_only: false,
            })?;
            Ok(items
                .into_iter()
                .map(|e| ListItem {
                    id: e.decision.id.to_string(),
                    title: e.decision.title,
                    status: e.decision.status,
                    date: e.decision.date,
                    tags: e.decision.tags,
                    provenance: e.provenance,
                })
                .collect::<Vec<_>>())
        })
        .await
    }

    #[tool(
        description = "Record a decision as a new file in this worktree. The rule: a decision is something a human decided. Record it as `accepted` only when a human on the project made it, or it is unmistakably clear from what they said, and quote them in `basis`. A choice you made yourself, however sensible, is `proposed` (the default); say in your reply that it needs the human's call. Everything written is redacted first."
    )]
    pub async fn decision_new(&self, Parameters(p): Parameters<NewParams>) -> CallToolResult {
        self.with_project(move |proj| {
            let status = match p.status.as_deref() {
                None => None,
                Some(s) => Some(Status::parse(s).ok_or_else(|| {
                    inloop_app::AppError::Invalid(format!("status `{s}`: use proposed or accepted"))
                })?),
            };
            let session = proj.current_session();
            proj.decision_new(NewDecision {
                title: p.title,
                status,
                decided_by: p.decided_by,
                basis: p.basis,
                session,
                tags: p.tags.unwrap_or_default(),
                context: p.context,
                decision: p.decision,
                consequences: p.consequences,
                supersedes: p.supersedes.unwrap_or_default(),
            })
        })
        .await
    }

    #[tool(
        description = "Accept proposed decisions because a human said so. `basis` must quote the human's words; never accept a decision on your own judgement. When the human accepts several at once, pass all their IDs in one call: every ID is checked first, so a wrong one changes nothing; the result lists what was accepted and what failed."
    )]
    pub async fn decision_accept(&self, Parameters(p): Parameters<AcceptParams>) -> CallToolResult {
        let (basis, by) = (p.basis, p.decided_by);
        self.with_project_each(
            p.ids,
            |proj, ids| proj.decisions_exist(ids),
            move |proj, id| {
                proj.decision_promote(id, true, Some(basis.clone()), by.clone(), "agent")
            },
        )
        .await
    }

    #[tool(
        description = "Reject proposed decisions because a human said so. Pass several IDs to reject them in one call; the result lists what was rejected and what failed."
    )]
    pub async fn decision_reject(&self, Parameters(p): Parameters<RejectParams>) -> CallToolResult {
        let reason = p.reason;
        self.with_project_each(
            p.ids,
            |proj, ids| proj.decisions_exist(ids),
            move |proj, id| proj.decision_promote(id, false, reason.clone(), None, "agent"),
        )
        .await
    }

    #[tool(
        description = "Record that one decision replaces another. Both files must be in this worktree; both are updated together."
    )]
    pub async fn decision_supersede(
        &self,
        Parameters(p): Parameters<SupersedeParams>,
    ) -> CallToolResult {
        self.with_project(move |proj| proj.decision_supersede(&p.old, &p.new))
            .await
    }

    #[tool(
        description = "Search summaries of past agent sessions in this project: what was asked, what was done, files touched, decisions made. Use it to find out why something was done."
    )]
    pub async fn session_search(&self, Parameters(p): Parameters<SearchParams>) -> CallToolResult {
        self.with_project(move |proj| proj.search(&p.query, &[Kind::Session], limit(p.limit)))
            .await
    }

    #[tool(
        description = "Get a captured session's summary, commits and decisions, with provenance. Set `transcript` for the full redacted transcript (paged with offset and max_chars) only when the summary isn't enough."
    )]
    pub async fn session_get(&self, Parameters(p): Parameters<SessionGetParams>) -> CallToolResult {
        self.with_project(move |proj| {
            let want = p.transcript.unwrap_or(false);
            let mut v = proj.session_get(&p.id, want)?;
            let mut page = serde_json::Map::new();
            if let Some(t) = v.transcript.take() {
                let total = t.chars().count();
                let offset = p.offset.unwrap_or(0).min(total);
                let max = p.max_chars.unwrap_or(40_000).clamp(1_000, 200_000);
                let chunk: String = t.chars().skip(offset).take(max).collect();
                let next = offset + chunk.chars().count();
                page.insert("transcript_total_chars".into(), total.into());
                if next < total {
                    page.insert("transcript_next_offset".into(), next.into());
                }
                v.transcript = Some(chunk);
            }
            let mut out =
                serde_json::to_value(&v).map_err(|e| inloop_app::AppError::Other(e.to_string()))?;
            if let Some(o) = out.as_object_mut() {
                o.extend(page);
            }
            Ok(out)
        })
        .await
    }

    #[tool(
        description = "Search decisions and session summaries together. Results carry provenance: `unmerged` means work in progress elsewhere, not settled fact."
    )]
    pub async fn search(&self, Parameters(p): Parameters<SearchParams>) -> CallToolResult {
        self.with_project(move |proj| proj.search(&p.query, &[], limit(p.limit)))
            .await
    }

    #[tool(
        description = "Where you are: the project, repo, worktree, branch, the main branch, and how far ahead of or behind main you are."
    )]
    pub async fn context(&self) -> CallToolResult {
        self.with_project(|proj| proj.context()).await
    }
}
