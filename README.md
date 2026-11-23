# InLoop

One binary for building software with AI agents while keeping a human in the loop.

- **Decisions** that many agents can record at once without colliding, kept in the repo and reviewed with the code.
- **Sessions** captured from your agents, redacted, encrypted and searchable later.
- **A work board** for a project manager agent that hands tasks to child sessions (optional).
- **One search** across all of it, for people through the CLI and for agents through MCP.

Status: phases 1 to 4 built, being dogfooded. Design and decisions: [CLAUDE.md](CLAUDE.md) and [docs/](docs/).

## Install

You need the GitHub CLI (`gh`), signed in with an account that can read this repo. Then, on macOS or Linux:

```sh
gh api repos/LiamGallagher737/InLoop/contents/install.sh -H "Accept: application/vnd.github.raw" | sh
```

That puts the latest release in `~/.local/bin` and tells you if that isn't on your PATH. Run it again to update. `INLOOP_VERSION=v0.1.0` picks a release, `INLOOP_INSTALL_DIR` another directory (D-AX7W). On Windows, take the `.zip` from the latest release. To build from source instead: `cargo install --path crates/cli`.

## A new project

```sh
cd your-repo
inloop init          # inloop.toml, docs/decisions, .mcp.json, Claude Code hooks, git hook
git add inloop.toml docs/decisions .mcp.json .claude/settings.json
git commit -m "chore: set up InLoop"
```

Open Claude Code in the repo and approve the `inloop` MCP server when asked. Agents now search and record decisions through it; people use `inloop dec new`, `inloop dec list` and `inloop search "why did we pick"`.

## Joining a project that already uses InLoop

```sh
cd the-repo && git pull
inloop init          # keeps the committed config; adds the git hook, which isn't cloned
```

Then open Claude Code and approve the `inloop` MCP server. Decisions are files in the repo, so you share them through git like the code. Nothing else to set up: no key is needed for decisions. Sessions are captured on your own machine; the key only matters if the project later shares sessions through a bucket (docs/03).

## Prompts for your coding agent

Paste one of these into Claude Code (or another coding agent) opened in the repo, and it does the setup for you.

**A new project:**

```text
Set up InLoop in this repo. InLoop records the project's decisions as files in the
repo and makes them searchable by agents over MCP.

1. If `inloop --version` fails, install it with:
   gh api repos/LiamGallagher737/InLoop/contents/install.sh -H "Accept: application/vnd.github.raw" | sh
   It needs the GitHub CLI signed in with access to that repo; if it isn't, stop and
   tell me. If the installer says its directory isn't on my PATH, add it to my shell
   profile and tell me you did.
2. Run `inloop init` at the repo root and show me what it created.
3. Fill in `[checks] commands` in inloop.toml with the commands this project already
   uses to lint, type check and test (look at the package scripts, Makefile and CI).
   Leave everything else at its defaults.
4. If the repo already records decisions somewhere (an ADR folder, a DECISIONS.md, a
   section of the docs), don't move or convert them: tell me where they are and ask.
5. Run `inloop lint`, then commit inloop.toml, the decisions directory, .mcp.json and
   .claude/settings.json as "chore: set up InLoop".
6. Tell me to restart Claude Code and approve the `inloop` MCP server, and add a line
   to CLAUDE.md (create it if missing) saying: search past decisions with the inloop
   MCP tools before making a design choice, and record new ones with decision_new;
   a decision is `accepted` only when a human made it, otherwise `proposed`.
```

**A project that already uses InLoop:**

```text
This repo already uses InLoop (there is an inloop.toml at the root). Set it up on
this machine.

1. If `inloop --version` fails, install it with:
   gh api repos/LiamGallagher737/InLoop/contents/install.sh -H "Accept: application/vnd.github.raw" | sh
   It needs the GitHub CLI signed in with access to that repo; if it isn't, stop and
   tell me. If the installer says its directory isn't on my PATH, add it to my shell
   profile and tell me you did.
2. Pull the latest main, then run `inloop init` at the repo root. It keeps the
   committed config and installs the git hook, which isn't part of a clone.
3. Don't change inloop.toml or anything in the decisions directory. If init reports
   that it updated .claude/settings.json or .mcp.json, show me the diff and ask
   before committing it.
4. Run `inloop status` and `inloop dec list`, and tell me how many decisions there
   are and whether any are still `proposed`.
5. Tell me to restart Claude Code and approve the `inloop` MCP server. No encryption
   key is needed for decisions.
```

`inloop --help` lists everything; every command takes `--json`.
