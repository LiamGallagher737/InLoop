# 05 The project manager layer (optional)

This layer is for people who run one agent as a project manager (PM) that hands tasks to child sessions. It is off unless a project sets `[pm] enabled = true`. InLoop without it is a first-class way to use InLoop, and nothing in the core assumes it (D-N6YH).

## The workflow it supports

A PM agent (for example Fable) runs in the main checkout. It doesn't write code itself. It:

1. keeps the plan on a work board
2. decides which tasks can run side by side without conflicting
3. starts child sessions (for example Opus, each in its own worktree) with a brief
4. reviews what each child delivers against its brief, and sends fixes back to the same child
5. lands the work when the human says so, then proposes what is newly unblocked

The human stays in the loop: they approve batches, decide facts only they know, and say when work lands.

## Who does what

| Part | Done by |
|---|---|
| Judgement: what's unblocked, what conflicts, whether work is actually done | The PM agent |
| Starting, messaging and stopping sessions | The agent app (for example the Claude desktop app) |
| Shared state: the board, task records, conflict checks, briefs, reports | InLoop |
| Decisions, session capture, search | InLoop's core, used by the PM and children like any other agent |

InLoop doesn't start sessions or try to be the PM (D-E2GZ). The PM is an agent because its value is judgement, and starting sessions belongs to the agent app.

## What InLoop gives the PM

1. **No more reserved decision numbers.** Random IDs mean children record decisions freely. The "reserved numbers" column and the block in every brief go away.
2. **The board as data.** Each task is a record: ID (`T-XXXX`), title, status, what it's blocked by, the paths it touches, the branch and session that own it, and timestamps. The board lives in the shared local store, not in a Markdown file committed to main, so board updates don't create commits (or, in projects where pushing main deploys, deploys). `inloop board` shows it; `inloop board export` writes Markdown when a human or co-founder wants to read it.
3. **Computed conflict checks.** `inloop.toml` lists conflict groups as path globs (files only one running task may touch at a time). Before the PM proposes a batch, `board_check` reports any overlap between tasks' `touches` and the groups or other running tasks.
4. **Generated briefs.** Most of a child brief is the same every time: docs to read, rules, checks to run, "don't push", the report format. `task_brief` builds it from a per-project template plus the task and the decisions relevant to it.
5. **Structured reports.** A child calls `task_report` at the end with what's done, what isn't, check results, decisions recorded and anything that should change the plan. The PM reads that instead of a truncated note or the whole transcript.

## Who writes the board

The PM is the only writer of the board. Children report through `task_report` and `task_blocked`, and the PM folds those into the board (D-Y40K). This keeps one writer, which is what made the hand-kept board work.

In practice (D-9QFB): task changes through MCP are accepted only from the main checkout; reports and blockers are records of their own that never change a task, and `board_fold` (or `inloop board fold`) applies them. `board_list` shows what is waiting to be folded and which blocked tasks are newly unblocked.

The board lives in `$(git rev-parse --git-common-dir)/inloop/board/` as one JSON file per record (`tasks/`, `claims/`, `reports/`, `blockers/`), each written through the redactor.

## Statuses

`ready`, `running`, `review`, `merged`, `blocked (external)`, `blocked (task)`, `blocked (human)`. The same set the hand-kept boards in Adpatch and Cirkit use. In JSON and on the command line they are `ready`, `running`, `review`, `merged`, `blocked-external`, `blocked-task`, `blocked-human`.

Transitions and claims follow D-KX03: `running` needs a branch and opens a claim; a claim by someone else is refused; `ready` and `merged` release claims; blocked statuses need a reason or a blocking task.

## The PM toolset

The PM tools are a separate MCP toolset, registered only when the PM layer is enabled (docs/06). They never change how the core tools behave; the CLI's `board` and `task` commands are hidden from help when the layer is off and refuse to run. A shipped PM skill (a general PM brief) explains the workflow; `inloop init` writes it to `.claude/skills/inloop-pm/SKILL.md`, and the default brief template to `.inloop/brief.md`, when the layer is on. Each project's specifics (conflict groups, checks, merge style, brief template) go in `inloop.toml` and the template, so a project's own PM brief shrinks to configuration.

The brief template understands `{{task.id}}`, `{{task.title}}`, `{{task.description}}`, `{{task.touches}}`, `{{task.branch}}`, `{{checks}}`, `{{decisions}}` (the decisions most relevant to the task, unmerged ones marked as not settled), `{{project}}` and `{{report_format}}`.

## Teams and sync (later)

With more than one person on a project, two people (each with their own PM) could pick up the same task. Within one person's PM that never happens; across people it can. So:

- **Now:** the board is local, but stored as one record per task and per claim, each with a stable ID, owner and timestamps, so records merge cleanly and adding sync later isn't a rewrite (D-V3ZM).
- **Later, free:** sync the board through a custom git ref (`refs/inloop/board`) that travels with the repo but isn't a branch. It only updates on fetch, so a race between fetches is still possible.
- **Later, paid:** a hosted service with instant claims that refuses a duplicate claim immediately, a combined view across everyone's PMs, notifications and a web board (docs/09).

Neither sync is built until the local board has been dogfooded.
