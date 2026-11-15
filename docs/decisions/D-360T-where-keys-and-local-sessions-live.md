---
id: D-360T
title: "Each person's project key and local session store live in their home directory"
status: accepted
date: 2026-11-14
decided_by: Liam Gallagher
basis: "Liam, after reviewing the nine proposed decisions in chat on 2026-11-15: \"Accept all nine\""
session: overnight build of phases 1 to 4, 2026-11-14 (not captured)
supersedes: []
superseded_by: null
aliases: []
tags: [sessions, security, storage]
promoted_by: agent
---

## Context

docs/08 left open where each person's age key lives and how a co-founder gets it. The local session backend also needs a home.

## Decision

The project key is an age identity at `~/.config/inloop/keys/<project>.agekey` (mode 0600), created on first capture. `inloop key export <file>` writes it to a new 0600 file to hand to a co-founder privately; `inloop key import <file>` installs it and refuses to replace an existing key. The local backend stores sessions in `~/.local/share/inloop/<project>/`. Both paths can be moved with `INLOOP_CONFIG_DIR` and `INLOOP_DATA_DIR`.

## Consequences

No keychain integration yet, so the key is as safe as the home directory. Losing the key file means losing access to stored sessions, so it is worth backing up. Moving the key into the macOS keychain or a password manager later only changes where ProjectKey loads from.

Proposed by the agent while building; needs Liam's call.
