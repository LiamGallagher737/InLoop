---
id: D-X618
title: "Release builds run only on version tags"
status: accepted
date: 2026-11-15
decided_by: Liam Gallagher
basis: "Please setup a github action for releases. Build for mac windows and linux and create a realease in github. only when tagged with a realse version, not all pushed"
session: 0c7a6174-01da-45a7-b432-70cb66c5f340
supersedes: []
superseded_by: null
aliases: []
tags: [release, ci]
---

## Context

InLoop needs downloadable binaries for Liam's projects and co-founders, who shouldn't have to build from source.

## Decision

A GitHub Actions workflow builds inloop for macOS, Windows and Linux and creates a GitHub release, triggered only by pushing a vX.Y.Z tag (optionally with a prerelease suffix), never by ordinary pushes. The tag must match the workspace version. The exact target list (Linux x86_64 and arm64, macOS arm64 and x86_64, Windows x86_64) was the agent's choice.

## Consequences

Cutting a release is bumping the workspace version and pushing a matching tag. Unsigned binaries for now; macOS users may need to clear the quarantine flag.
