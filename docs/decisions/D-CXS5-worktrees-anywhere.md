---
id: D-CXS5
title: "Worktrees are found wherever they live"
status: accepted
date: 2026-11-14
decided_by: Liam
basis: "if work trees can be in other places, I'd like to try and support that as much as possible."
session: design discussion 2026-11-14, before InLoop existed (not captured)
supersedes: []
superseded_by: null
aliases: []
tags: [git]
---

## Context

The Claude desktop app puts worktrees in one folder, but other tools and people put them elsewhere.

## Decision

InLoop discovers worktrees with `git worktree list` and keeps shared local state in the git common directory, so nothing depends on where worktrees are.

## Consequences

Separate clones on other machines are reached through remote branches after a fetch.
