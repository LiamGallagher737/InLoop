# InLoop

A single Rust binary for AI-driven development with a human in the loop. Its core is two things any agent can use, in any workflow: recording a project's decisions and saving its agent sessions. Both are searchable by people (CLI) and agents (MCP), and secrets are redacted from everything InLoop stores. On top of that core sits an optional project manager layer (a work board, conflict checks, briefs and reports) for anyone who runs a PM agent with child sessions. Using InLoop without it is a first-class way to use InLoop (D-N6YH).

**Current phase:** phases 1 to 4 are built (2026-11-15) and waiting on dogfooding: migrating Cirkit and Adpatch, installing the hooks, a real PM batch. See `docs/08-roadmap.md` for what's left. Record decisions with `inloop dec new` (or the `decision_new` MCP tool) now that it exists; InLoop is developed with InLoop (D-NPET).

## How to use these docs

`CLAUDE.md` is the table of contents and the short list of rules. Everything substantive lives in `docs/`. Read the docs relevant to the task before writing code, and update them in the same commit when a decision changes. Docs are the source of truth; code follows docs.

| Doc | What it covers | Read it when |
|---|---|---|
| [00-overview](docs/00-overview.md) | What InLoop is, who it's for, the problems it solves, what it is not | Starting any new piece of work |
| [01-architecture](docs/01-architecture.md) | Cargo workspace, crate boundaries, where data lives (repo, git common dir, session store), worktree discovery | Adding a crate, touching storage |
| [02-decisions](docs/02-decisions.md) | Decision IDs, file format, statuses, the basis rule, cross-worktree visibility, migration from a single file | Anything about decisions |
| [03-sessions](docs/03-sessions.md) | Capturing agent sessions, summaries, storage backends, encryption, purge, commit trailers | Session capture or storage |
| [04-redaction](docs/04-redaction.md) | The redaction layers, the `Redacted<T>` gate, fail closed, the test corpus | Before anything that writes data anywhere |
| [05-pm-layer](docs/05-pm-layer.md) | The optional project manager layer: the work board as data, conflict groups, briefs and reports, team sync later | Board or PM work |
| [06-cli-and-mcp](docs/06-cli-and-mcp.md) | Command layout, human output, `--json`, MCP tools and the provenance every result carries | Adding or changing a command or tool |
| [07-config](docs/07-config.md) | `inloop.toml`: decisions directory, conflict groups, checks, storage, redaction rules | Adding a setting |
| [08-roadmap](docs/08-roadmap.md) | Phases with exit criteria, open questions | Planning the next chunk of work |
| [09-product-and-name](docs/09-product-and-name.md) | The name, domains, licence and the possible business later | Naming, licensing, anything public |
| [10-conventions](docs/10-conventions.md) | Rust style, commit style, doc style, how agents record decisions before the CLI exists | Always, briefly |
| [decisions/](docs/decisions/) | One file per decision (`D-XXXX-slug.md`) | When a design choice is made or reversed |

## Rules that are not negotiable

1. **A decision comes from the human.** Record a decision as `accepted` only when Liam (or another human on the project) made it, or it is unmistakably clear from what they said. Quote them in `basis`. Anything you chose yourself is `proposed`. Details in `docs/02` and D-BY48.
2. **New decisions get a fresh random ID, never the "next number".** Until the CLI exists, generate one with the snippet in `docs/10` and check no file already uses it.
3. **Nothing InLoop stores skips redaction.** Transcripts, summaries, decisions, board records, reports: all go through the redactor, and if it fails nothing is written (D-G1R6). Details in `docs/04`.
4. **Secrets never touch logs, errors or `Debug` output.** Wrap them in `secrecy::SecretString`.
5. **InLoop is a tool for a workflow, not for one project.** Nothing Adpatch- or Cirkit-specific in code or defaults; project specifics go in `inloop.toml` (D-0217).
6. **No `unsafe`, no `unwrap`/`expect` in library crates.** `#![forbid(unsafe_code)]` in every crate.
7. **Update the docs in the same commit as the code.** A changed decision gets a new decision file that supersedes the old one; don't rewrite history in an accepted decision.
8. **Every result an agent sees says where it stands.** Merged on the main branch, or only on another branch or worktree, and which task that worktree belongs to when the PM layer is in use (D-E1S7).
9. **The core never assumes a project manager.** Decisions, sessions, search and redaction work the same for one person with one agent. Core MCP tools and their descriptions never mention a PM, a board or child sessions; PM tools are a separate toolset that is off unless a project turns it on (D-N6YH).

## Quick commands

```sh
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo run -q -- lint        # InLoop's own decisions
```

## Repo layout

```
CLAUDE.md          this file
README.md          short public description
inloop.toml        InLoop's own project config
docs/              all project documentation
  decisions/       one file per decision
crates/            Rust workspace members (docs/01)
tools/             the one-off decisions migration script
```

## Project facts

- Owner: Liam Gallagher. Closed source for now (D-VMRP).
- Language: Rust, edition 2024, latest stable toolchain.
- First users: Liam's own projects (Adpatch, Cirkit) and their co-founders, all on Claude Code.
- Docs are written in NZ English. No em dashes.
