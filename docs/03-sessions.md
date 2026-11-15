# 03 Sessions

InLoop captures agent sessions so the reasoning behind a change can be found later, by a person or by an agent. Capture, storage and search are all built in (D-0N56).

## Capture

The first agent source is Claude Code, which both Liam and his co-founders use (D-TBR6). Claude Code already writes each session as JSONL under `~/.claude/projects/<project-slug>/`. InLoop:

1. installs Claude Code hooks (`SessionStart`, `Stop`, `PreCompact`, `SessionEnd`) in `.claude/settings.json` with `inloop init`, all calling `inloop session hook`
2. on `SessionStart` (and every later event) records which session is running in which worktree, for commit trailers and for linking decisions; `SessionEnd` clears it
3. on `Stop`, `PreCompact` and `SessionEnd` starts `inloop session capture` as a detached background process and returns at once (D-FG6G)
4. capture reads the transcript, keeps only conversation records (user, assistant, system, summary) and drops the app's bookkeeping (D-7K5W), sends it through the redactor (docs/04), then on to storage

Capture skips a transcript that hasn't changed since the last capture, holds a per-session lock, and replaces the session's previous blob.

Sources sit behind an `AgentSource` trait, so another agent app can be added later without touching storage or search. Only Claude Code is built until someone needs another.

Capture must be fast and never block the agent: if capture fails, the hook logs the failure locally (without content) and exits successfully, and `inloop session status` shows recent failures and transcripts that were never captured. `inloop session capture --backfill` stores those.

## What is stored

For each session:

- **Metadata:** session ID, project, branch, worktree, start and end time, commits made, decisions recorded, and (PM layer) the task.
- **A summary:** a short structured summary (what was asked, what was done, decisions, open ends). Search hits summaries first, so agents rarely need a full transcript.
- **The full transcript:** redacted, compressed, encrypted. Fetched only when someone asks for it.

Summaries are built by InLoop from the redacted transcript with no model call (D-6KFG): the first prompt, later prompts, the agent's last message, files edited, tool use counts, open to-do items, decisions recorded in the session and commits carrying its trailer.

## Linking sessions to commits and decisions

- A `prepare-commit-msg` hook (installed by `inloop init`, merged into any existing shell hook) adds a `Session: <id>` trailer to each commit made while a session is running in that worktree. A session not heard from for 12 hours is treated as ended.
- `decision_new` records the current session in the decision's `session` field.

Together these give the chain from a decision, to the conversation where it was made, to the commits that came out of it.

A session's provenance follows its commits: it is `merged` only when it made commits and all of them are on the main branch (whichever of local `main` and `origin/main` is further ahead), and `unmerged` otherwise. It is worked out when results are read, never stored. The trailer hook skips commits replayed by rebase, cherry-pick or revert.

## Storage

Storage is designed as if InLoop may become a product used by people outside Liam's projects, so it is not tied to any one provider or to anything Adpatch uses (D-T1C2).

- **A `SessionStore` trait** with two backends:
  - `local`: a directory on disk. The default, no setup.
  - `s3`: any S3-compatible bucket, which covers Cloudflare R2, AWS S3, Backblaze B2, MinIO and others with one implementation.
- **Client-side encryption** with [age](https://age-encryption.org/), a key per project, kept at `~/.config/inloop/keys/<project>.agekey` and created on first capture. `inloop key export <file>` and `inloop key import <file>` hand it to a co-founder (D-360T). Transcripts are encrypted before they leave the machine, so a bucket provider (or a hosted InLoop service later) never sees plaintext. A co-founder gets a project's key, and only that project's.
- **Content-addressed blobs:** each transcript is stored under the hash of its encrypted bytes, which is checked before decrypting. age is randomised, so storing the same transcript twice gives two hashes; metadata keeps the current one.
- **Compression:** zstd, inside the encryption.
- **Metadata and summaries** go in the local index; with a shared bucket, a small encrypted metadata file per session lets other machines rebuild their index with `inloop session sync`.

### Options considered

| Option | Good | Bad |
|---|---|---|
| A separate git repo for transcripts | Simple, syncs anywhere | History is permanent, so purging a leak means rewriting every clone; grows forever |
| Local SQLite only | Fast, private | Co-founders can't see sessions; lost with the disk |
| Encrypted blobs in object storage plus a local index | Real deletion, per-project access, cheap | Needs a bucket and key management |

The chosen design is the third, with `local` as the zero-setup default.

## Purge

`inloop session purge <id>` deletes the blob, its metadata and its index entry. If a secret is found in a stored transcript, purge it and rotate the secret; rotate the project's age key too if the transcript may have been read. No history rewrite is ever needed.
