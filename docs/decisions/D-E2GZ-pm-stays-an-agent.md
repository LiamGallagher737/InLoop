---
id: D-E2GZ
title: "The project manager stays an agent; InLoop holds its state"
status: accepted
date: 2026-11-14
decided_by: Liam
basis: "Okay, this seems much more like what I had in my head."
session: design discussion 2026-11-14, before InLoop existed (not captured)
supersedes: []
superseded_by: null
aliases: []
tags: [pm]
---

## Context

Liam runs a PM agent that starts child sessions, reviews their work and lands it. The question was whether InLoop should take that role. The agent recommended keeping it an agent, and Liam agreed with the overall direction above; he later added that InLoop's features "should be tools that can be used by the project manager".

## Decision

InLoop doesn't start sessions or make PM judgements. It provides shared state and bookkeeping (board, conflict checks, briefs, reports) that a PM agent uses.

## Consequences

Starting sessions stays with the agent app. The PM's context goes on judgement rather than bookkeeping.
