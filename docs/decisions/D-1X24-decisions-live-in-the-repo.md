---
id: D-1X24
title: "Decisions live in the project's repo"
status: accepted
date: 2026-11-14
decided_by: Liam
basis: "Decisions are definitely something that should be in the repo because that's something I want synced across every person working on the project."
session: design discussion 2026-11-14, before InLoop existed (not captured)
supersedes: []
superseded_by: null
aliases: []
tags: [decisions, storage]
---

## Context

Decisions could live in the repo or in InLoop's own store.

## Decision

Decision files are committed in the project's repo.

## Consequences

They sync to everyone through git, follow branches (a decision on an unmerged branch isn't true on main yet) and are reviewed in pull requests with the code.
