---
id: D-VH2N
title: "A minimal redactor ships in phase 1, so decisions are never written outside the gate"
status: accepted
date: 2026-11-14
decided_by: Liam
basis: "D-VNK6: Yes A"
session: phase 0 review of proposed decisions, 2026-11-14 (not captured)
supersedes: []
superseded_by: null
aliases: []
tags: [redaction, security, roadmap]
---

## Context

The roadmap had phase 1 writing decision files and the redactor arriving in phase 2. That broke D-G1R6 (everything stored is redacted first) for the whole of phase 1. Two options were put to Liam: (a) bring a minimal `redact` crate into phase 1, or (b) record a known gap for phase 1. He chose (a).

## Decision

Phase 1 includes the `redact` crate with the `Redacted<T>` type gate (D-VNK6) and the first two layers: known values and a starter set of provider patterns. Every decision write goes through it from the first release. Phase 2 adds the remaining layers, the full gitleaks ruleset and the test corpus.

## Consequences

No release of InLoop ever writes unredacted data. Phase 1 grows by one small crate. The type gate is in place before any other write path exists, so later write paths can't be built without it.
