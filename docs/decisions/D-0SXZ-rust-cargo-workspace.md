---
id: D-0SXZ
title: "Rust, as one Cargo workspace producing one binary"
status: accepted
date: 2026-11-14
decided_by: Liam
basis: "Same with the project structure as a workspace: I like building this way."
session: design discussion 2026-11-14, before InLoop existed (not captured)
supersedes: []
superseded_by: null
aliases: []
tags: [architecture]
---

## Context

Liam asked for the tool in Rust from the start, and builds his other projects as Cargo workspaces.

## Decision

InLoop is written in Rust (edition 2024) as a Cargo workspace of focused crates that build one `inloop` binary. The crate list is in docs/01.

## Consequences

Crate boundaries keep the core independent of the optional PM layer (D-N6YH).
