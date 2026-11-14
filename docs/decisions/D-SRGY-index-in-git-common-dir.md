---
id: D-SRGY
title: "The search index and local state live in the git common directory"
status: proposed
date: 2026-11-14
decided_by: agent
basis: null
session: design discussion 2026-11-14, before InLoop existed (not captured)
supersedes: []
superseded_by: null
aliases: []
tags: [architecture, search]
---

## Context

Search must see every worktree, and local state must never be committed or merged.

## Decision

Keep a SQLite FTS5 index and other local state under `$(git rev-parse --git-common-dir)/inloop/`, which every worktree of a clone shares.

## Consequences

Derived data only: it can always be rebuilt.

Proposed by the agent during the design discussion; needs Liam's call.
