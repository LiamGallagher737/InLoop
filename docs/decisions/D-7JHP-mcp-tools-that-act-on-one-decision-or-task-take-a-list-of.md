---
id: D-7JHP
title: "MCP tools that act on one decision or task take a list of IDs"
status: accepted
date: 2026-11-15
decided_by: Liam Gallagher
basis: "Liam: \"Please extend this to the MCP as well ... the project manager will present me with all its proposed decisions for me to accept at once ... It would be better if it can then just, in one call, mark these as accepted when I say so.\""
session: 1c319ce9-6f58-4308-9385-4387a4aa58c7
supersedes: []
superseded_by: null
aliases: []
tags: [mcp, ids]
---

## Context

A PM, or any agent, often presents the human with several proposed decisions at once. When the human accepts them all, the agent had to make one decision_accept call per decision. D-3DCF did this for the CLI only.

## Decision

decision_get, decision_accept, decision_reject, task_get, task_update and task_brief take ids, a list of IDs; a single string and the old id field still work. One ID returns what it always did. Several are all checked to exist first (a wrong one changes nothing), then each is tried, and the result is {done, failed}, an error only if nothing worked. The PM skill tells the PM to accept a list in one call.

## Consequences

Accepting a reviewed list is one tool call. Existing callers passing id keep working. The result shape depends on how many IDs were given, which the caller chose.
