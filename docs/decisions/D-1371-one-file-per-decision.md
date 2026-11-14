---
id: D-1371
title: "One file per decision"
status: accepted
date: 2026-11-14
decided_by: Liam
basis: "one file per decision is also good."
session: design discussion 2026-11-14, before InLoop existed (not captured)
supersedes: []
superseded_by: null
aliases: []
tags: [decisions]
---

## Context

Appending to one shared file conflicts on every parallel merge, even with unique IDs, because every branch edits the same place.

## Decision

Each decision is its own Markdown file named `<id>-<slug>.md`, with YAML frontmatter and Context, Decision and Consequences sections (docs/02).

## Consequences

Parallel decisions never conflict. A readable list comes from the CLI, not from a hand-kept index file.
