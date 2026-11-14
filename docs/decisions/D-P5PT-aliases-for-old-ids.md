---
id: D-P5PT
title: "Old sequential IDs resolve through an aliases field"
status: proposed
date: 2026-11-14
decided_by: agent
basis: null
session: design discussion 2026-11-14, before InLoop existed (not captured)
supersedes: []
superseded_by: null
aliases: []
tags: [decisions, migration]
---

## Context

Commit messages can't be rewritten, so after migration they still cite numbers like D-264.

## Decision

Each decision has an optional `aliases` list. The migration script puts the old number there, and lookups resolve aliases.

## Consequences

One format and one lookup, with no compatibility mode. The same field handles merged or renamed decisions later.

Proposed by the agent during the design discussion; needs Liam's call.
