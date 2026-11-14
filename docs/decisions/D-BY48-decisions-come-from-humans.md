---
id: D-BY48
title: "A decision is accepted only when a human made it; agent choices are proposals"
status: accepted
date: 2026-11-14
decided_by: Liam
basis: "I think the children should be able to record decisions. One thing to be clear, though, is that a decision should be something that the human developer specifies or is very clear from context, and not something that the session has just chosen"
session: design discussion 2026-11-14, before InLoop existed (not captured)
supersedes: []
superseded_by: null
aliases: []
tags: [decisions, rules]
---

## Context

Agents tend to record their own implementation choices as decisions, which dilutes the log.

## Decision

Any agent can record a decision. It is `accepted` only with a `basis` quoting the human or pointing to where they said it. Choices an agent made itself are recorded as `proposed` with `decided_by: agent`.

## Consequences

The tool refuses an accepted decision without a basis. The `decision_new` tool description states the rule. Who promotes proposals is an open question (docs/08).
