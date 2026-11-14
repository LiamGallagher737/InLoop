---
id: D-6QJK
title: "Existing decision files are migrated once by a throwaway script, with no compatibility mode"
status: accepted
date: 2026-11-14
decided_by: Liam
basis: "Ideally, we'd just make a throwaway script that converts my existing huge markdown file into the split ones using sort of search and replace across the codebase. That way, we're not having to support two different things"
session: design discussion 2026-11-14, before InLoop existed (not captured)
supersedes: []
superseded_by: null
aliases: []
tags: [decisions, migration]
---

## Context

Adpatch and Cirkit each have a single numbered decisions file.

## Decision

A one-off script splits the file into decision files with new IDs and replaces old references across the repo. InLoop itself supports only the new format.

## Consequences

Projects are fully on InLoop after migrating, which makes for honest dogfooding. Old commit messages still cite old numbers; see D-P5PT for how they resolve.
