---
id: D-AX7W
title: "Install with a script that downloads the release binary through the GitHub CLI"
status: accepted
date: 2026-11-23
decided_by: Liam Gallagher
basis: "Liam, in chat on 2026-11-23, replying to the proposal to build an install script and make the hooks skip quietly when inloop is not installed: \"Yes, do both\""
session: edc706ca-556a-44c9-86d2-b656eb744f74
supersedes: []
superseded_by: null
aliases: []
tags: [setup, release]
---

## Context

Installing meant cargo install from a clone, which needs Rust and access to a private repo. Release binaries exist for macOS and Linux, but downloading them by hand from a private repo is awkward and an unnotarised download can be stopped by Gatekeeper.

## Decision

install.sh at the repo root downloads the release archive for the machine with gh release download, checks it against SHA256SUMS, installs it to ~/.local/bin (INLOOP_INSTALL_DIR to change), clears the macOS quarantine flag and warns if the directory isn't on PATH. It is fetched and run in one line with gh api. Windows users take the .zip from the release page.

## Consequences

Setup needs gh signed in with access to the repo, and no Rust toolchain. Running it again updates inloop. If the repo or its releases go public later, the script can switch to plain curl.
