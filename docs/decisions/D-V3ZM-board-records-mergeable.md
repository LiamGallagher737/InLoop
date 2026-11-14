---
id: D-V3ZM
title: "Board data is stored as mergeable records, local first"
status: proposed
date: 2026-11-14
decided_by: agent
basis: null
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

Proposed by the agent during the design discussion; needs Liam's call.
