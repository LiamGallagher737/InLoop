# 01 Architecture

## One binary, one workspace

InLoop ships as one binary, `inloop`, built from a Cargo workspace (D-0SXZ). Planned crates:

| Crate | What it holds |
|---|---|
| `core` | IDs (Crockford base32), shared types, the provenance model, errors |
| `redact` | The redactor and the `Redacted<T>` type (docs/04) |
| `git` | Repository and worktree discovery, reading files from branches and remote refs |
| `decisions` | Decision files: parse, validate, create, supersede, lint |
| `index` | The local SQLite full-text index over decisions and session summaries |
| `sessions` | Capture from agent sources, summaries, the storage trait and its backends |
| `pm` | The optional PM layer: board records, conflict groups, briefs, reports |
| `mcp` | The MCP server (rmcp), core toolset and the separate PM toolset |
| `cli` | The `inloop` binary: argument parsing and human output |

The core crates never depend on `pm`. The `mcp` and `cli` crates register PM commands and tools only when the project enables the PM layer (D-N6YH).

## Where data lives

InLoop keeps three kinds of data in three places, each chosen for how it needs to behave.

| Data | Where | Why |
|---|---|---|
| Decisions | In the repo, `docs/decisions/` by default (D-1X24, D-SJBG) | Must sync to everyone through git, follow branches, and be reviewed in PRs |
| Index, board, local state | The git common directory, `$(git rev-parse --git-common-dir)/inloop/` (D-SRGY) | Shared by every worktree of the clone, never committed, never merged |
| Session transcripts | The session store: local disk or an S3-compatible bucket, encrypted (D-T1C2) | Large, private, must be deletable for real |

The index is derived data. It can always be rebuilt from the decision files on every branch and worktree plus the session store's metadata.

## Finding worktrees

InLoop never assumes where worktrees live. It runs `git worktree list --porcelain` (or the `gix` equivalent) to find every worktree of the current clone, wherever it is on disk (D-CXS5). For each one it records the path, branch, HEAD and how far it is ahead of and behind the main branch.

Decisions on branches that are not checked out anywhere (a co-founder's pushed branch, for example) are read straight from git objects on `refs/remotes/origin/*` after a fetch, without a checkout.

## Provenance

Every decision and session result carries a provenance record (D-E1S7):

- `state`: `merged` (on the main branch) or `unmerged`
- `branch`, `worktree` path (if checked out), commits ahead of main
- `task`: the PM task that owns that branch, only when the PM layer is in use
- for decisions, `superseded_by` if a newer decision replaced it

The CLI shows this as a short status line; MCP returns it as fields.

## Data flow, in brief

- **Recording a decision:** agent calls `decision_new` (or a person runs `inloop dec new`) → redactor → new file written in the current worktree → index updated.
- **Searching:** query → index (all worktrees, remote branches, session summaries) → results with provenance.
- **Capturing a session:** agent app hook fires → transcript read → redactor → summary → compress → encrypt → session store; metadata and summary → index.
