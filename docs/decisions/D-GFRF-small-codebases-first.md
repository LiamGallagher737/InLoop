---
id: D-GFRF
title: "Target small codebases first"
status: accepted
date: 2026-11-14
decided_by: Liam
basis: "We can always start only targeting smaller codebases like the ones that I work on and add bigger features later."
session: design discussion 2026-11-14, before InLoop existed (not captured)
supersedes: []
superseded_by: null
aliases: []
tags: [scope]
---

## Context

Very large projects might need archiving, sharding or other scaling features.

## Decision

Build for projects the size of Adpatch and Cirkit. Add scaling features only when a real project needs them.

## Consequences

No archive folders, sharding or embedding search in the first phases.
