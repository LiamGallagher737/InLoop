---
id: D-FK3P
title: "InLoop talks to git through the git command line, not gix"
status: accepted
date: 2026-11-14
decided_by: Liam Gallagher
basis: "Liam, after reviewing the nine proposed decisions in chat on 2026-11-15: \"Accept all nine\""
session: overnight build of phases 1 to 4, 2026-11-14 (not captured)
supersedes: []
superseded_by: null
aliases: []
tags: [git, architecture]
promoted_by: agent
---

## Context

docs/01 allowed either `git worktree list --porcelain` or the gix equivalent. gix's API changes often and is large; everything InLoop needs is a handful of plumbing commands.

## Decision

The `git` crate shells out to `git` (worktree list, for-each-ref, ls-tree, cat-file --batch, rev-list). Many blobs are read in one `cat-file --batch` call; branches already merged into main are skipped with `for-each-ref --no-merged`.

## Consequences

InLoop needs git on PATH, which every user already has. Behaviour matches what people see from git themselves. If scanning ever gets slow on big repos, gix can replace the internals behind the same functions.

Proposed by the agent while building; needs Liam's call.
