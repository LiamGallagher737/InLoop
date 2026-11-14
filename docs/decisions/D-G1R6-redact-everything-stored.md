---
id: D-G1R6
title: "Everything InLoop stores is redacted first, and nothing is written if redaction fails"
status: accepted
date: 2026-11-14
decided_by: Liam
basis: "Having very good secret redaction would be a good idea."
session: design discussion 2026-11-14, before InLoop existed (not captured)
supersedes: []
superseded_by: null
aliases: []
tags: [redaction, security]
---

## Context

Stored sessions are only acceptable if secrets aren't kept. Liam listed "the token redaction and anything that this app stores" among the main things InLoop handles.

## Decision

Transcripts, summaries, decisions, board records and reports all pass through one redactor before storage. If it errors, the write doesn't happen.

## Consequences

Redaction is built before session storage (D-JZT3). Details and layers in docs/04.
