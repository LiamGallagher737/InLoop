---
id: D-11JK
title: "Decision IDs are random Crockford base32, like D-7KQ2"
status: accepted
date: 2026-11-14
decided_by: Liam
basis: "Ideas based on the Crockford Base32 make a lot of sense to me"
session: design discussion 2026-11-14, before InLoop existed (not captured)
supersedes: []
superseded_by: null
aliases: []
tags: [decisions, ids]
---

## Context

Sequential IDs collide when parallel agents each take the next number. The PM workaround, reserving blocks of numbers per child, breaks when a child makes more decisions than planned.

## Decision

Each decision gets a random ID made where it is recorded: `D-` plus Crockford base32 characters. No allocation or coordination.

## Consequences

IDs never change, so references in code, docs and commits stay valid. Order comes from the date field, not the ID. The default length of 4 characters (about a million IDs, with a collision check) is the agent's suggestion and is configurable.
