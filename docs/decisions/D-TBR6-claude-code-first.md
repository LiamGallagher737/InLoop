---
id: D-TBR6
title: "Claude Code is the first agent source"
status: accepted
date: 2026-11-14
decided_by: Liam
basis: "All of the straightforward ones you recommend accepting, I agree with. They can all be accepted."
session: design discussion 2026-11-14, before InLoop existed (not captured)
supersedes: []
superseded_by: null
aliases: []
tags: [sessions]
---

## Context

Liam and his co-founders use Claude Code.

## Decision

Capture Claude Code sessions through its hooks and JSONL transcripts first, behind an `AgentSource` trait.

## Consequences

Other agents can be added later without changing storage or search.

Proposed by the agent in the design discussion; accepted by Liam in the phase 0 review, 2026-11-14.
