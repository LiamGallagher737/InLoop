# 08 Roadmap

Build order (D-JZT3): decisions first because they are the actual pain and the smallest piece, with a minimal redactor so nothing is ever written outside the gate (D-VH2N); the full redactor before anything stores transcripts; then sessions; then the optional PM layer, built on what the earlier phases create.

## Where things stand (2026-11-15)

Phases 1 to 4 are built, with tests, on main. What is left in each is the part that needs people: running InLoop on real projects. Each phase lists it under **Still to do**.

## Phase 0: design (done)

- Docs in `docs/`, decisions recorded by hand in the InLoop format (D-NPET).
- **Exit:** Liam has read the docs, accepted or rejected the `proposed` decisions, and agreed the phase 1 scope.

## Phase 1: decisions (built)

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
- **Still to do:** run `tools/migrate_decisions.py` on Cirkit and Adpatch (tested on a scratch copy of Adpatch's log only), install `inloop` and run `inloop init` in all three, then the two weeks of use.

## Phase 2: redaction (built)

- The rest of the `redact` crate: context plus entropy, whole outputs, project rules, and the full gitleaks provider ruleset.
- The test corpus, wired into CI.
- **Exit:** the corpus passes with no known misses; redaction runs on every write path that exists.
- **Status:** 15 fixtures, 98/98 fake secrets caught, 172/172 look-alikes intact; decisions, transcripts, session metadata and board records all go through the gate. CI is in `.github/workflows/ci.yml`, waiting for a remote.

## Phase 3: sessions (built)

- Claude Code capture through hooks; the `AgentSource` trait.
- Summaries; the `Session:` commit trailer; decisions linked to sessions.
- `SessionStore` with `local` and `s3` backends; age encryption; purge.
- Session search in the CLI and MCP.
- **Exit:** every Claude Code session in the three projects is captured, searchable and redacted, and a co-founder on Cirkit can search Liam's sessions with the project key.
- **Still to do:** `inloop init` in each project to install the hooks; the `s3` backend against a real bucket (tested only against an in-memory store); a co-founder importing the key with `inloop key import`.

## Phase 4: the PM layer (optional, built)

- Board and task records in the shared store; `board export`.
- Conflict groups and `board_check`; generated briefs; `task_report` and `task_blocked`.
- The PM toolset, off by default; a shipped general PM skill.
- Cirkit's and Adpatch's PM briefs reduced to config plus a template.
- **Exit:** a full PM batch (propose, start children, review, land) runs on Cirkit with no hand-kept board.
- **Still to do:** reduce Cirkit's and Adpatch's PM briefs to config plus a template (their repos weren't touched), then run a real batch. A full batch runs end to end in the test suite.

## Later, only when needed

- Board sync over a git ref; hosted team sync (docs/05, docs/09).
- More agent sources beyond Claude Code.
- Generic collections: other append-only logs (misconceptions, dogfood notes) as one file per entry with the same IDs and search (D-Y1DQ).
- Archive folder for old decisions in big projects.
- Embedding-based search if full-text search feels weak.

## Open questions

- **Who promotes `proposed` decisions?** Always the human, or the PM during review? Settle by dogfooding; the tool records who promoted each one (`promoted_by`, D-B2NX proposed), so the record will show what works.
- **Board sync for teams:** the free git-ref sync, the paid hosted sync, or both? Decide after the local board is in use.
- **How summaries are made:** proposed answer D-6KFG: InLoop builds them from the transcript, no model.
- **Where the age key lives** for each person: proposed answer D-360T: a file in `~/.config/inloop/keys/`, handed over with `inloop key export` and `import`.
