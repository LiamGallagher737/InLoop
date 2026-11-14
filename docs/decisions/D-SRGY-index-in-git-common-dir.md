---
id: D-SRGY
title: "The search index and local state live in the git common directory"
status: accepted
date: 2026-11-14
decided_by: Liam
basis: "D-SRGY: Yes"
session: design discussion 2026-11-14, before InLoop existed (not captured)
supersedes: []
superseded_by: null
aliases: []
tags: [architecture, search, pm]
---

## Context

Search must see every worktree, and local state must never be committed or merged.

## Decision

Keep a SQLite FTS5 index and other local state under `$(git rev-parse --git-common-dir)/inloop/`, which every worktree of a clone shares.

## Consequences

The index is derived data: it can always be rebuilt from decision files and the session store.

The PM board (docs/05) also lives here, and it is not derived. It is primary data, local to one clone and not backed up: deleting or re-cloning the repo loses it. `inloop board export` is the way to keep a copy. This is acceptable while the board is one person's working state; board sync (D-V3ZM) is where it would gain a second copy.

Proposed by the agent in the design discussion; accepted by Liam in the phase 0 review, 2026-11-14, with the board's status as primary data stated.
