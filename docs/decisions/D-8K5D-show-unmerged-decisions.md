---
id: D-8K5D
title: "Decisions on other worktrees and branches are shown by default, clearly marked unmerged"
status: accepted
date: 2026-11-14
decided_by: Liam
basis: "I think yes, as long as it's very clear that they are not a merged decision and that they are on another work tree being developed currently in parallel."
session: design discussion 2026-11-14, before InLoop existed (not captured)
supersedes: []
superseded_by: null
aliases: []
tags: [decisions, search]
---

## Context

Parallel sessions can make related decisions without knowing about each other.

## Decision

Search and lists include decisions that exist only on other branches and worktrees, always marked unmerged with their branch and worktree.

## Consequences

Agents can spot overlap early. Tool descriptions must say that unmerged decisions are work in progress, not settled fact.
