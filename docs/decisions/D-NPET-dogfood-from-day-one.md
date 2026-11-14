---
id: D-NPET
title: "InLoop is developed with InLoop, with decisions written by hand until the CLI exists"
status: accepted
date: 2026-11-14
decided_by: Liam
basis: "Set it up to be used with InLoop for development once it's in a good state, so we can use InLoop to develop. InLoop, this means adding discussion decisions preemptively, even though the CLI may not exist, but as the agent, and then we can clean up stuff afterwards"
session: design discussion 2026-11-14, before InLoop existed (not captured)
supersedes: []
superseded_by: null
aliases: []
tags: [process]
---

## Context

The design discussion produced many decisions before any code existed.

## Decision

Decisions from the discussion are recorded now in InLoop's file format. Until the CLI exists, agents write decision files by hand following docs/10.

## Consequences

When phase 1 lands, `inloop lint` checks these files and anything flagged is fixed.
