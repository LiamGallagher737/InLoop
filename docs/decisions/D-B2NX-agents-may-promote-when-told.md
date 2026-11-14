---
id: D-B2NX
title: "Agents may accept or reject a proposed decision when a human tells them to, and the file records who promoted it"
status: proposed
date: 2026-11-14
decided_by: agent
basis: null
session: overnight build of phases 1 to 4, 2026-11-14 (not captured)
supersedes: []
superseded_by: null
aliases: []
tags: [decisions, basis-rule]
---

## Context

docs/08 asks who promotes proposed decisions. In practice the human often says "accept D-X" to their agent in chat rather than running a command.

## Decision

MCP has `decision_accept` (basis required, quoting the human) and `decision_reject`. Every promotion writes `promoted_by` into the decision's frontmatter: the person's name from the CLI, `agent` from MCP. The tool descriptions say never to accept on the agent's own judgement.

## Consequences

The record shows how promotion happens in practice, which is how docs/08 said to settle the question. If agents promote too freely, the MCP tools can be removed without touching the file format.

Proposed by the agent while building; needs Liam's call.
