---
id: D-FG6G
title: "Session capture runs in a detached background process started by the hook"
status: accepted
date: 2026-11-14
decided_by: Liam Gallagher
basis: "Liam, after reviewing the nine proposed decisions in chat on 2026-11-15: \"Accept all nine\""
session: overnight build of phases 1 to 4, 2026-11-14 (not captured)
supersedes: []
superseded_by: null
aliases: []
tags: [sessions, hooks]
promoted_by: agent
---

## Context

Claude Code's Stop hook fires after every turn, and capture (redact, compress, encrypt, store) of a multi-megabyte transcript takes a noticeable fraction of a second. docs/03 says capture must never block the agent.

## Decision

`inloop session hook` records the active session for the worktree, starts `inloop session capture` as a detached process and returns at once, always successfully. Capture skips transcripts that haven't changed since the last capture, takes a per-session lock, replaces the session's previous blob, and logs failures without content. SessionStart only records the active session; SessionEnd clears it.

## Consequences

The agent never waits on capture. A failed capture is visible in `inloop session status`, and `inloop session capture --backfill` stores anything missed.

Proposed by the agent while building; needs Liam's call.
