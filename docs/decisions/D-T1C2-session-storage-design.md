---
id: D-T1C2
title: "Session storage is pluggable, with client-side encryption"
status: accepted
date: 2026-11-14
decided_by: Liam
basis: "All of the straightforward ones you recommend accepting, I agree with. They can all be accepted."
session: design discussion 2026-11-14, before InLoop existed (not captured)
supersedes: []
superseded_by: null
aliases: []
tags: [sessions, storage, security]
---

## Context

InLoop may become a product, so storage shouldn't be tied to any one provider or to Adpatch's infrastructure.

## Decision

A `SessionStore` trait with `local` (default) and S3-compatible backends. Transcripts are redacted, compressed and encrypted with age, a key per project, before they leave the machine.

## Consequences

Deleting really deletes. A hosted service later would never see plaintext.

Proposed by the agent in the design discussion; accepted by Liam in the phase 0 review, 2026-11-14.
