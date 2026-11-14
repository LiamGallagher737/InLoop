---
id: D-9QFB
title: "Only the main checkout changes tasks; task sessions file reports and blockers that the PM folds in"
status: proposed
date: 2026-11-14
decided_by: agent
basis: null
session: overnight build of phases 1 to 4, 2026-11-14 (not captured)
supersedes: []
superseded_by: null
aliases: []
tags: [pm]
---

## Context

D-Y40K says the PM is the only writer of the board and children report through `task_report` and `task_blocked`. Every session talks to the same MCP server, so InLoop needs a way to tell the PM from a child, and a way for the PM to fold reports in.

## Decision

Through MCP, `task_new`, `task_update` and `board_fold` work only from the main worktree, where the PM runs. `task_report` and `task_blocked` write their own records (one file each) and never change a task; they find the task from the current branch when no ID is given. `board_fold` (MCP) and `inloop board fold` apply unfolded blockers (setting the blocked status and reason) and reports not yet folded (moving running to review, linking decisions, adding a note), marking each folded. `board_list` shows what is waiting to be folded. The CLI is for the human and isn't restricted.

## Consequences

One writer of task records, as with the hand-kept boards. A PM that runs in a worktree instead of the main checkout would be refused; that can become a config option if anyone needs it.

Proposed by the agent while building; needs Liam's call.
