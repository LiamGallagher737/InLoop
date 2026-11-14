---
id: D-V3ZM
title: "Board data is stored as mergeable records, local first"
status: accepted
date: 2026-11-14
decided_by: Liam
basis: "All of the straightforward ones you recommend accepting, I agree with. They can all be accepted."
session: design discussion 2026-11-14, before InLoop existed (not captured)
supersedes: []
superseded_by: null
aliases: []
tags: [pm, sync]
---

## Context

Team sync isn't decided, but two people on one project could take the same task.

## Decision

Store each task and claim as its own record with a stable ID, owner and timestamps, in the local shared store. Build no sync until the local board has been dogfooded.

## Consequences

Sync (a git ref, or a hosted service) can be added later without a rewrite.

Proposed by the agent in the design discussion; accepted by Liam in the phase 0 review, 2026-11-14.
