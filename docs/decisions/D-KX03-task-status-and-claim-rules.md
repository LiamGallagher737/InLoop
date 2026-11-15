---
id: D-KX03
title: "Task status and claim rules for the board"
status: accepted
date: 2026-11-14
decided_by: Liam Gallagher
basis: "Liam, after reviewing the nine proposed decisions in chat on 2026-11-15: \"Accept all nine\""
session: overnight build of phases 1 to 4, 2026-11-14 (not captured)
supersedes: []
superseded_by: null
aliases: []
tags: [pm, sync]
promoted_by: agent
---

## Context

docs/05 lists the statuses and D-V3ZM asks for claim records so two people can't take the same task once sync exists. The transitions themselves weren't specified.

## Decision

`running` and `review` need a branch. Moving to `running` opens a claim for the task's owner (or whoever made the change, who becomes the owner); an open claim by someone else is refused. Moving to `ready` or `merged` releases open claims; blocked statuses and review keep them. `blocked-task` needs `blocked_by`; `blocked-external` and `blocked-human` need a reason, cleared when the task leaves the blocked status. A new task blocked by unmerged tasks starts as `blocked-task`. A blocked task whose blockers have all merged is listed as newly unblocked. The conflict check treats a plain directory as everything under it and is conservative about globs that match no file yet.

## Consequences

The board can't hold a running task with no branch or a blocked task with no reason. The duplicate-claim refusal is local today and becomes the cross-person check when sync arrives.

Proposed by the agent while building; needs Liam's call.
