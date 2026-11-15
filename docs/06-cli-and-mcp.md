# 06 CLI and MCP

People and agents get the same data. The CLI is designed for humans to read; MCP returns the same data as structured fields (D-C21Z).

## CLI

### Commands (planned)

Core, always available:

```
inloop init                       set up a repo: inloop.toml, decisions dir, Claude Code hooks
inloop dec new                    record a decision (interactive, or flags)
inloop dec show <id>              render one decision
inloop dec list [filters]         table of decisions: --status, --tag, --unmerged, --since
inloop dec search <query>         ranked results with matching snippets
inloop dec accept|reject <id>     promote or reject a proposed decision
inloop dec supersede <old> <new>  link a replacement
inloop session list|show|search   find captured sessions (show --transcript for the full text)
inloop session capture            capture now (--backfill for anything missed)
inloop session status|purge|sync  capture health; delete a session; rebuild the index from the store
inloop key show|export|import     this project's session encryption key
inloop search <query>             decisions and sessions together
inloop lint                       check decisions (docs/02)
inloop scan [--fix]               re-run redaction over everything stored
inloop status                     where you are: branch, main, worktrees
inloop mcp                        run the MCP server over stdio
```

PM layer, only when enabled:

```
inloop board [export|fold]        show the board, write it as Markdown, or fold in reports and blockers
inloop task new|show|update       manage task records
inloop task brief <id>            print the generated brief
inloop task check [ids]           conflict check: the given (or ready) tasks plus running ones
inloop task report|blocked        what a task session reports (usually through MCP)
```

`dec` is short for `decision`; both work.

### Output rules

- **Readable by default.** `dec show` renders the Markdown in the terminal with headings, emphasis and wrapped text. Lists are aligned tables. Colour when the output is a terminal, none when piped or when `NO_COLOR` is set.
- **Status at a glance.** Every decision shows its status, and anything not on main shows where it is:

  ```
  D-7KQ2  Snap writes go through one helper                    accepted
          NOT ON MAIN · branch snap-writes · ~/code/x/snap-writes (3 ahead)
  ```

- **A pager for long output**, as git does, unless piped.
- **`--json` on every command**, with the same fields MCP returns. Scripts and agents never parse human output.
- **Errors say what to do next**, never just what failed.

## MCP

`inloop mcp` runs an MCP server over stdio, built with rmcp.

### Core toolset

Always registered. Tool names and descriptions never mention a PM, a board or child sessions (D-N6YH).

| Tool | Does |
|---|---|
| `decision_search` | Full-text search over decisions on every branch and worktree |
| `decision_get` | One decision by ID (aliases resolve) |
| `decision_list` | Filtered list |
| `decision_new` | Record a decision; states the basis rule (docs/02) |
| `decision_accept` / `decision_reject` | Promote a proposed decision because a human said so; `basis` quotes them (D-B2NX) |
| `decision_supersede` | Record a replacement |
| `session_search` | Search session summaries |
| `session_get` | A session's summary, or its full transcript on request, paged by `offset` and `max_chars` |
| `search` | Decisions and sessions together |
| `context` | Where this agent is: repo, branch, worktree, main branch, ahead/behind, and its task when the PM layer is on |

### PM toolset

Registered only when `[pm] enabled = true`.

| Tool | Does |
|---|---|
| `board_list` | The board, plus reports and blockers not folded in yet and newly unblocked tasks |
| `board_check` | Conflict check for a set of tasks |
| `board_fold` | Fold reports and blockers into the board (PM only, D-9QFB) |
| `task_get` / `task_new` / `task_update` | Task records (PM only writes, from the main checkout) |
| `task_brief` | Generated brief for a task |
| `task_report` | A child's structured end-of-work report |
| `task_blocked` | A child reporting it is blocked, and on what |

### Provenance on every result

Every decision and session result includes (D-E1S7):

```json
{
  "id": "D-7KQ2",
  "status": "accepted",
  "provenance": {
    "state": "unmerged",
    "branch": "snap-writes",
    "worktree": "/Users/someone/code/x/snap-writes",
    "ahead_of_main": 3,
    "task": "T-4M1P"
  }
}
```

`task` is present only when the PM layer is in use and the branch belongs to a task. Tool descriptions tell the agent that an `unmerged` decision is work in progress on another branch, not settled fact.

### Description style

Tool descriptions are short, say when to use the tool, and state the rules that matter (the basis rule, unmerged means not settled). They are snapshot-tested so changes show up in review.
