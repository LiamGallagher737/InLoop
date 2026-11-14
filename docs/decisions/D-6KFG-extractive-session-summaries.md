---
id: D-6KFG
title: "Session summaries are built by InLoop from the transcript, with no model call"
status: proposed
date: 2026-11-14
decided_by: agent
basis: null
session: overnight build of phases 1 to 4, 2026-11-14 (not captured)
supersedes: []
superseded_by: null
aliases: []
tags: [sessions, summaries]
---

## Context

docs/08 left open whether the agent writes a summary at session end through MCP, or InLoop calls a model. The first relies on agents remembering to call a tool; the second needs an API key and sends transcripts to a model provider.

## Decision

InLoop builds a structured summary itself from the redacted transcript: the first prompt (what was asked), later prompts, the agent's last message (what was done), files edited, tool use counts, to-do items still open, decisions recorded and commits with the session's trailer. No model, no API key.

## Consequences

Capture works offline and costs nothing. Summaries are consistent but not prose; if they prove too thin in dogfooding, a model-written summary can be added on top as an optional field without changing storage.

Proposed by the agent while building; needs Liam's call.
