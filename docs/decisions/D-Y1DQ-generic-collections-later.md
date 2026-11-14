---
id: D-Y1DQ
title: "Other append-only logs can become collections later"
status: accepted
date: 2026-11-14
decided_by: Liam
basis: "All of the straightforward ones you recommend accepting, I agree with. They can all be accepted."
session: design discussion 2026-11-14, before InLoop existed (not captured)
supersedes: []
superseded_by: null
aliases: []
tags: [decisions, later]
---

## Context

Shared append-only docs (Cirkit's misconceptions file, for example) have the same merge-conflict problem as decisions.

## Decision

Ship decisions only. Keep decision-specific assumptions out of the ID and one-file-per-entry layer, but build no collection abstraction until a second collection is actually needed.

## Consequences

A later collection should need little new code, without paying for generality now.

Proposed by the agent in the design discussion; accepted by Liam in the phase 0 review, 2026-11-14, in this narrower form.
