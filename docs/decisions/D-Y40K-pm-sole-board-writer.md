---
id: D-Y40K
title: "Only the PM writes the board; children report"
status: proposed
date: 2026-11-14
decided_by: agent
basis: null
session: design discussion 2026-11-14, before InLoop existed (not captured)
supersedes: []
superseded_by: null
aliases: []
tags: [pm]
---

## Context

A board with many writers has the same conflict problem as a shared decisions file.

## Decision

The PM is the only writer of the board. Children call `task_report` and `task_blocked`, and the PM folds those in.

## Consequences

Matches how the hand-kept boards already work.

Proposed by the agent during the design discussion; needs Liam's call.
