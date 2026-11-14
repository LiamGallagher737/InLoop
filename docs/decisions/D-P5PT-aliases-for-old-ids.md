---
id: D-P5PT
title: "Old sequential IDs resolve through an aliases field"
status: accepted
date: 2026-11-14
decided_by: Liam
basis: "All of the straightforward ones you recommend accepting, I agree with. They can all be accepted."
session: design discussion 2026-11-14, before InLoop existed (not captured)
supersedes: []
superseded_by: null
aliases: []
tags: [decisions, migration]
---

## Context

Commit messages can't be rewritten, so after migration they still cite numbers like D-264.

Random IDs can be all digits (this repo has D-0217 and D-1371), so a project with 1,000 or more sequential decisions could have an old `D-1234` that is also a valid random ID.

## Decision

Each decision has an optional `aliases` list. The migration script puts the old number there, and lookups resolve aliases.

To keep aliases and IDs apart: the ID generator never produces an all-digit ID, and `inloop lint` fails if any alias equals any decision's ID. Existing all-digit IDs keep their IDs, since an ID never changes.

## Consequences

One format and one lookup, with no compatibility mode. The same field handles merged or renamed decisions later. Skipping all-digit IDs removes about 1% of the 4-character ID space.

Proposed by the agent in the design discussion; accepted by Liam in the phase 0 review, 2026-11-14, with the all-digit and alias lint rules added.
