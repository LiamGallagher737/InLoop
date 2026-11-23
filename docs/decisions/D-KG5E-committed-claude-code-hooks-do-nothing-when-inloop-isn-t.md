---
id: D-KG5E
title: "Committed Claude Code hooks do nothing when inloop isn't installed"
status: accepted
date: 2026-11-23
decided_by: Liam Gallagher
basis: "Liam, in chat on 2026-11-23, replying to the proposal to build an install script and make the hooks skip quietly when inloop is not installed: \"Yes, do both\""
session: edc706ca-556a-44c9-86d2-b656eb744f74
supersedes: []
superseded_by: null
aliases: []
tags: [hooks, setup]
---

## Context

inloop init writes the session capture hooks into .claude/settings.json, which projects commit. Anyone who clones such a repo without installing InLoop got a failing hook on every Claude Code session. The git prepare-commit-msg hook already guarded itself with command -v.

## Decision

Each Claude Code hook runs `command -v inloop >/dev/null 2>&1 && inloop session hook || true`. inloop init upgrades the old unguarded `inloop session hook` entries in place. `inloop session hook` always exits successfully (D-FG6G), so `|| true` hides nothing.

## Consequences

People working in a repo that uses InLoop can install it later without Claude Code complaining meanwhile. Hooks assume a POSIX shell, as the git hook already does. Existing projects pick up the guard by running inloop init again and committing .claude/settings.json.
