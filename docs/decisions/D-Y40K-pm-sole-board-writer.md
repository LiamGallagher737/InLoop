---
id: D-Y40K
title: "Only the PM writes the board; children report"
status: accepted
date: 2026-11-14
decided_by: Liam
basis: "All of the straightforward ones you recommend accepting, I agree with. They can all be accepted."
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

Proposed by the agent in the design discussion; accepted by Liam in the phase 0 review, 2026-11-14.
