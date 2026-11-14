# InLoop

One binary for building software with AI agents while keeping a human in the loop.

- **Decisions** that many agents can record at once without colliding, kept in the repo and reviewed with the code.
- **Sessions** captured from your agents, redacted, encrypted and searchable later.
- **A work board** for a project manager agent that hands tasks to child sessions (optional).
- **One search** across all of it, for people through the CLI and for agents through MCP.

Status: phases 1 to 4 built, being dogfooded. Design and decisions: [CLAUDE.md](CLAUDE.md) and [docs/](docs/).

## Try it

```sh
cargo install --path crates/cli     # installs `inloop`
cd your-repo
inloop init                         # inloop.toml, docs/decisions, .mcp.json, capture hooks
inloop dec new                      # record a decision
inloop dec list
inloop search "why did we pick"
inloop session list                 # after a Claude Code session in this repo
```

`inloop --help` lists everything; every command takes `--json`.
