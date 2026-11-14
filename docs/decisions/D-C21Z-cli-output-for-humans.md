---
id: D-C21Z
title: "The CLI is first-class and built for humans to read"
status: accepted
date: 2026-11-14
decided_by: Liam
basis: "I also want it to have really good output for humans to read it if I wanted to see a decision rather than having to search through a file tree. I'd like to use a CLI, so making sure that works really nicely."
session: design discussion 2026-11-14, before InLoop existed (not captured)
supersedes: []
superseded_by: null
aliases: []
tags: [cli]
---

## Context

MCP serves agents; people need their own way in.

## Decision

The CLI renders decisions and lists for reading in a terminal: rendered Markdown, aligned tables, colour, a pager, and status lines that show where each decision stands. Every command also has `--json` with the same fields as MCP.

## Consequences

Human and agent views never drift apart because they share one data model (docs/06).
