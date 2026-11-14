# 08 Roadmap

Build order (D-JZT3): decisions first because they are the actual pain and the smallest piece, with a minimal redactor so nothing is ever written outside the gate (D-VH2N); the full redactor before anything stores transcripts; then sessions; then the optional PM layer, built on what the earlier phases create.

## Phase 0: design (now)

- Docs in `docs/`, decisions recorded by hand in the InLoop format (D-NPET).
- **Exit:** Liam has read the docs, accepted or rejected the `proposed` decisions, and agreed the phase 1 scope.

## Phase 1: decisions

- Workspace with `core`, `redact`, `git`, `decisions`, `index`, `mcp`, `cli`.
- A minimal `redact` crate: the `Redacted<T>` gate (D-VNK6) with the known-values layer and a starter set of provider patterns. Every decision write goes through it (D-VH2N).
- ID generation with collision checks across worktrees and remote branches; no all-digit IDs (D-P5PT).
- Parse, validate, create, supersede, accept and reject decision files.
- Worktree discovery anywhere on disk; reading decisions from unchecked-out branches.
- SQLite FTS5 index in the git common directory.
- CLI: `init`, `dec new|show|list|search|accept|reject|supersede`, `lint`, all with `--json`.
- MCP core decision tools with provenance.
- Clean up InLoop's own hand-written decisions with the CLI (`inloop lint`, fix anything it finds).
- The one-off migration script, then migrate Cirkit and Adpatch.
- **Exit:** InLoop, Cirkit and Adpatch all record decisions only through InLoop for two weeks; no decision ID conflicts; agents use `decision_search` instead of reading the decisions folder.

## Phase 2: redaction

- The rest of the `redact` crate: context plus entropy, whole outputs, project rules, and the full gitleaks provider ruleset.
- The test corpus, wired into CI.
- **Exit:** the corpus passes with no known misses; redaction runs on every write path that exists.

## Phase 3: sessions

- Claude Code capture through hooks; the `AgentSource` trait.
- Summaries; the `Session:` commit trailer; decisions linked to sessions.
- `SessionStore` with `local` and `s3` backends; age encryption; purge.
- Session search in the CLI and MCP.
- **Exit:** every Claude Code session in the three projects is captured, searchable and redacted, and a co-founder on Cirkit can search Liam's sessions with the project key.

## Phase 4: the PM layer (optional)

- Board and task records in the shared store; `board export`.
- Conflict groups and `board_check`; generated briefs; `task_report` and `task_blocked`.
- The PM toolset, off by default; a shipped general PM skill.
- Cirkit's and Adpatch's PM briefs reduced to config plus a template.
- **Exit:** a full PM batch (propose, start children, review, land) runs on Cirkit with no hand-kept board.

## Later, only when needed

- Board sync over a git ref; hosted team sync (docs/05, docs/09).
- More agent sources beyond Claude Code.
- Generic collections: other append-only logs (misconceptions, dogfood notes) as one file per entry with the same IDs and search (D-Y1DQ).
- Archive folder for old decisions in big projects.
- Embedding-based search if full-text search feels weak.

## Open questions

- **Who promotes `proposed` decisions?** Always the human, or the PM during review? Settle by dogfooding; the tool records who promoted each one, so the record will show what works.
- **Board sync for teams:** the free git-ref sync, the paid hosted sync, or both? Decide after the local board is in use.
- **How summaries are made:** the agent writes one at session end through MCP, or InLoop calls a model itself. The first needs no API key; the second is more consistent.
- **Where the age key lives** for each person (keychain, file, password manager), and how a new co-founder gets it.
