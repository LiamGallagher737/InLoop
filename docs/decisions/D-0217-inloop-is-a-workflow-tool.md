---
id: D-0217
title: "InLoop is one tool for Liam's AI development workflow, not for one project"
status: accepted
date: 2026-11-14
decided_by: Liam
basis: "I would rather this tool be sort of like a separate thing that I use for my projects, not built for my project, but sort of built for my workflow."
session: design discussion 2026-11-14, before InLoop existed (not captured)
supersedes: []
superseded_by: null
aliases: []
tags: [scope]
---

## Context

Adpatch and Cirkit both keep a decisions file, a PM brief and a work board, and Adpatch uses a separate tool to capture sessions. The pain points (decision ID collisions, lost sessions, PM bookkeeping) are the same in both.

## Decision

Build InLoop: a single Rust binary for AI-driven development, used across Liam's projects and by the people working on them. It is not part of Adpatch or Cirkit and contains nothing specific to either.

## Consequences

Project specifics live in each project's `inloop.toml`. Defaults and examples stay general. InLoop has its own repo, accounts and infrastructure.
