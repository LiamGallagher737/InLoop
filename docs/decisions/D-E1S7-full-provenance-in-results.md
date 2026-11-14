---
id: D-E1S7
title: "Every result says whether it is on main, and which branch, worktree and task it comes from"
status: accepted
date: 2026-11-14
decided_by: Liam
basis: "The MCP should also offer as much information as possible to whoever's asking, making sure it knows that it's not on main and it's from a different work tree"
session: design discussion 2026-11-14, before InLoop existed (not captured)
supersedes: []
superseded_by: null
aliases: []
tags: [mcp, cli]
---

## Context

An agent that can't tell merged from in-progress decisions may build on something that never lands.

## Decision

Every decision and session result carries provenance: merged or unmerged, branch, worktree path, commits ahead of main, and the owning task when the PM layer is in use.

## Consequences

Shown as a status line in the CLI and as fields in MCP and `--json`.
