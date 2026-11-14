---
id: D-VNK6
title: "Storage only accepts a Redacted type"
status: accepted
date: 2026-11-14
decided_by: Liam
basis: "D-VNK6: Yes A"
session: design discussion 2026-11-14, before InLoop existed (not captured)
supersedes: []
superseded_by: null
aliases: []
tags: [redaction, security]
---

## Context

Redaction must be impossible to skip by accident (D-G1R6).

## Decision

The storage layer only accepts `Redacted<T>`, which only the redactor can construct. `Redacted<T>` has no public constructor and implements none of `Deserialize`, `DerefMut`, `From` or `Default`, since each of those would be a second way to build or change one without the redactor.

## Consequences

A new write path that skips redaction won't compile. Reading stored data back in does not produce a `Redacted<T>`; anything re-stored goes through the redactor again. The gate ships in phase 1 (D-VH2N).

Proposed by the agent in the design discussion; accepted by Liam in the phase 0 review, 2026-11-14, with the rule against other constructors added.
