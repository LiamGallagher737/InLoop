---
id: D-VNK6
title: "Storage only accepts a Redacted type"
status: proposed
date: 2026-11-14
decided_by: agent
basis: null
session: design discussion 2026-11-14, before InLoop existed (not captured)
supersedes: []
superseded_by: null
aliases: []
tags: [redaction, security]
---

## Context

Redaction must be impossible to skip by accident (D-G1R6).

## Decision

The storage layer only accepts `Redacted<T>`, which only the redactor can construct.

## Consequences

A new write path that skips redaction won't compile.

Proposed by the agent during the design discussion; needs Liam's call.
