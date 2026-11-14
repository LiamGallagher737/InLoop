# 00 Overview

## What InLoop is

InLoop is a single Rust binary that helps a small team build software with AI agents while a human stays in charge of the choices that matter. It has a core and an optional layer.

**The core, for everyone:**

- **Decisions.** Agents and people record the project's decisions as one file each, in the repo, with random IDs so parallel work never collides. Decisions are reviewed in pull requests with the code that implements them.
- **Sessions.** Agent sessions are captured, redacted, summarised and stored, so a later agent (or person) can find out why something was done.
- **Search.** One search across decisions and sessions, including decisions still on unmerged branches in other worktrees, clearly labelled as such.
- **Redaction.** Everything InLoop stores passes through one redactor first. If redaction fails, nothing is written.

People reach all of this through a CLI built to be read by humans. Agents reach it through an MCP server that returns the same data.

**The optional project manager (PM) layer:** for people who run one agent as a project manager that hands tasks to child sessions, reviews their work and lands it. InLoop gives that PM a work board stored as data, computed conflict checks between tasks, generated briefs and structured reports. It is off unless a project turns it on, and the core never assumes it (D-N6YH).

## Who it is for

First, Liam and the co-founders on his projects (Adpatch, Cirkit), who do most of their development through Claude Code. Then friends and other small teams who work the same way. Possibly a product later (docs/09).

InLoop is built for a workflow, not for a project: nothing in it is specific to Adpatch or Cirkit (D-0217).

## The problems it solves

1. **Decision numbers collide.** Sequential IDs (D-264, D-265) in one shared file mean every parallel agent appends to the same place and picks the same next number. Today a PM agent pre-allocates blocks of numbers to each child and guesses wrong when unplanned decisions come up. Random IDs and one file per decision remove the problem rather than managing it (D-11JK, D-1371).
2. **A big decisions file is expensive to read.** Agents load hundreds of entries to find the two that matter. Search over MCP returns only those.
3. **Sessions go missing.** The reasoning behind a change lives in a transcript nobody can find a week later.
4. **Stored transcripts leak secrets and customer data** unless redaction is thorough and impossible to bypass.
5. **The PM's bookkeeping eats its context.** Tracking the board, reserved numbers and conflicts by hand leaves less room for judgement (optional layer).

## What it is not

- **Not an agent runner.** InLoop does not start, stop or drive agent sessions. The agent app does that (Claude Code, the Claude desktop app). In the PM workflow, the PM agent supplies the judgement and InLoop supplies the shared state (D-E2GZ).
- **Not an add-on to another tool.** Session capture, storage and search are built in, so InLoop is one binary with no dependency on another product (D-0N56).
- **Not for huge monorepos, yet.** It targets small codebases first (D-GFRF).
- **Not tied to one directory layout.** Worktrees can live anywhere (D-CXS5).
