---
id: D-30V6
title: "CLI and MCP share one app crate, and storage gets its own store crate"
status: proposed
date: 2026-11-14
decided_by: agent
basis: null
session: overnight build of phases 1 to 4, 2026-11-14 (not captured)
supersedes: []
superseded_by: null
aliases: []
tags: [architecture]
---

## Context

docs/01 planned the CLI and MCP crates side by side. Both need the same operations (open the project, scan decisions, write through the redactor, capture sessions), and the session store is large enough to stand alone.

## Decision

Add an `app` crate holding every operation, which `cli` and `mcp` call, so people and agents always get the same data (D-C21Z). Add a `store` crate for the SessionStore trait, its local and S3-compatible backends and age encryption; `sessions` keeps sources, transcripts and summaries.

## Consequences

Two more crates than docs/01 listed; docs/01 is updated to match. The core crates still never depend on `pm`.

Proposed by the agent while building; needs Liam's call.
