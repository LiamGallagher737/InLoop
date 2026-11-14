---
id: D-7K5W
title: "Stored transcripts keep the conversation and drop the agent app's bookkeeping"
status: proposed
date: 2026-11-14
decided_by: agent
basis: null
session: overnight build of phases 1 to 4, 2026-11-14 (not captured)
supersedes: []
superseded_by: null
aliases: []
tags: [sessions, redaction]
---

## Context

Claude Code transcripts hold conversation records plus large amounts of app bookkeeping (API request shapes, file history snapshots, UI state). A real session was about 3.4 MB, much of it bookkeeping.

## Decision

Capture keeps records of type user, assistant, system and summary, drops the rest, and redacts everything kept (including tool output in `toolUseResult`). Lines that aren't valid JSON are kept as redacted text.

## Consequences

Smaller stores and less to leak. Anything only in the bookkeeping records (exact API parameters, file history) is not kept; nothing InLoop shows or searches uses it.

Proposed by the agent while building; needs Liam's call.
