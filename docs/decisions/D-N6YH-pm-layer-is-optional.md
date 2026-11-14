---
id: D-N6YH
title: "The project manager layer is optional, and the core never assumes it"
status: accepted
date: 2026-11-14
decided_by: Liam
basis: "using InLoop without the project manager workflow should be a first-class citizen. The MCP shouldn't be designed in a way where it's expecting there to be a project manager or anything."
session: design discussion 2026-11-14, before InLoop existed (not captured)
supersedes: []
superseded_by: null
aliases: []
tags: [pm, mcp, scope]
---

## Context

Liam uses a PM workflow, but InLoop should suit people who don't.

## Decision

The core (decisions, sessions, search, redaction) works the same for one person with one agent. The PM layer (board, conflict checks, briefs, reports, a PM skill) is off unless a project enables it. Core MCP tools never mention a PM; PM tools are a separate toolset.

## Consequences

The core crates don't depend on the `pm` crate. Provenance includes a task only when the PM layer is in use.
