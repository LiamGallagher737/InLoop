---
id: D-3DCF
title: "ID prefixes are optional and single-record commands take several IDs"
status: accepted
date: 2026-11-15
decided_by: Liam Gallagher
basis: "Liam: \"make the commands for showing a decision and other ones that take a decision ID support the decision ID not having the prefix\" and \"when an agent is presenting me with multiple ... decisions ... I can tell it to just accept all of them. It only has to do one command rather than having to do a bash for loop\""
session: 1c319ce9-6f58-4308-9385-4387a4aa58c7
supersedes: []
superseded_by: null
aliases: []
tags: [cli, ids]
---

## Context

Typing D- or T- before every ID is friction, and accepting a list of decisions an agent presented meant a shell for loop.

## Decision

Wherever a decision or task ID is expected, the prefix may be left off (7KQ2 means D-7KQ2; old alias numbers work the same, 264 means D-264). dec show, dec accept, dec reject, task show, task update and task brief take any number of IDs. Batches that change records check every ID exists before changing anything, then carry on past per-ID failures and exit non-zero if any failed. --json gives the object for one ID and an array for several.

## Consequences

Accepting a reviewed list is one command. Single-ID JSON output is unchanged. MCP tools still take one ID per call but accept bare IDs.
