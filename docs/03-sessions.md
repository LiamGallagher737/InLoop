# 03 Sessions

InLoop captures agent sessions so the reasoning behind a change can be found later, by a person or by an agent. Capture, storage and search are all built in (D-0N56).

## Capture

The first agent source is Claude Code, which both Liam and his co-founders use (D-TBR6, proposed). Claude Code already writes each session as JSONL under `~/.claude/projects/<project-slug>/`. InLoop:

1. installs Claude Code hooks (`Stop`, `PreCompact`, `SessionEnd`) with `inloop init`, which call `inloop session capture`
2. reads the session's transcript from the hook input
3. sends it through the redactor (docs/04), then on to storage

Sources sit behind an `AgentSource` trait, so another agent app can be added later without touching storage or search. Only Claude Code is built until someone needs another.

Capture must be fast and never block the agent: if capture fails, the hook logs the failure locally (without content) and exits successfully, and `inloop session status` shows sessions that weren't captured.

## What is stored

For each session:

- **Metadata:** session ID, project, branch, worktree, start and end time, commits made, decisions recorded, and (PM layer) the task.
- **A summary:** a short structured summary (what was asked, what was done, decisions, open ends). Search hits summaries first, so agents rarely need a full transcript.
- **The full transcript:** redacted, compressed, encrypted. Fetched only when someone asks for it.

How summaries are generated (by the agent at session end through an MCP call, or by InLoop calling a model) is an open question (docs/08).

## Linking sessions to commits and decisions

- A `prepare-commit-msg` hook adds a `Session: <id>` trailer to each commit made during a captured session.
- `decision_new` records the current session in the decision's `session` field.

Together these give the chain from a decision, to the conversation where it was made, to the commits that came out of it.

## Storage

Storage is designed as if InLoop may become a product used by people outside Liam's projects, so it is not tied to any one provider or to anything Adpatch uses (D-T1C2, proposed).

- **A `SessionStore` trait** with two backends:
  - `local`: a directory on disk. The default, no setup.
  - `s3`: any S3-compatible bucket, which covers Cloudflare R2, AWS S3, Backblaze B2, MinIO and others with one implementation.
- **Client-side encryption** with [age](https://age-encryption.org/), a key per project. Transcripts are encrypted before they leave the machine, so a bucket provider (or a hosted InLoop service later) never sees plaintext. A co-founder gets a project's key, and only that project's.
- **Content-addressed blobs:** each transcript is stored under the hash of its encrypted bytes.
- **Metadata and summaries** go in the local index; with a shared bucket, a small encrypted metadata file per session lets other machines rebuild their index.

### Options considered

| Option | Good | Bad |
|---|---|---|
| A separate git repo for transcripts | Simple, syncs anywhere | History is permanent, so purging a leak means rewriting every clone; grows forever |
| Local SQLite only | Fast, private | Co-founders can't see sessions; lost with the disk |
| Encrypted blobs in object storage plus a local index | Real deletion, per-project access, cheap | Needs a bucket and key management |

The chosen design is the third, with `local` as the zero-setup default.

## Purge

`inloop session purge <id>` deletes the blob and its metadata. If a secret is found in a stored transcript, purge it and rotate the secret; rotate the project's age key too if the transcript may have been read. No history rewrite is ever needed.
