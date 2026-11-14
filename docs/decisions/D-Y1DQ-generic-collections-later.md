---
id: D-Y1DQ
title: "Other append-only logs can become collections later"
status: proposed
date: 2026-11-14
decided_by: agent
basis: null
session: design discussion 2026-11-14, before InLoop existed (not captured)
supersedes: []
superseded_by: null
aliases: []
tags: [decisions, later]
---

## Context

Shared append-only docs (Cirkit's misconceptions file, for example) have the same merge-conflict problem as decisions.

## Decision

Design the ID and one-file-per-entry layer so it could serve other collections. Ship decisions only.

## Consequences

A later collection would be mostly configuration.

Proposed by the agent during the design discussion; needs Liam's call.
