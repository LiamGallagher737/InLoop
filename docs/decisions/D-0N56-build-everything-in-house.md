---
id: D-0N56
title: "Session capture and storage are built in, with no dependency on another product"
status: accepted
date: 2026-11-14
decided_by: Liam
basis: "If there is stuff like session storage, I would like to bring that in-house just so it's one binary that does everything that I want. I don't want to be dependent on someone else's code."
session: design discussion 2026-11-14, before InLoop existed (not captured)
supersedes: []
superseded_by: null
aliases: []
tags: [scope, sessions]
---

## Context

Adpatch currently captures sessions with a third-party CLI. The first suggestion was to build the decisions layer on top of it.

## Decision

InLoop captures, stores and searches sessions itself. It does not integrate with or depend on another session tool.

## Consequences

More to build (capture, storage, encryption). Ordinary open-source Rust libraries are still used as normal; this decision is about not depending on another product. (That reading is the agent's: confirm or correct it.)
