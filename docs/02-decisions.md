# 02 Decisions

The decision log is the part of InLoop most agents will touch most often. It replaces a single, sequentially numbered `decisions.md` with one file per decision and random IDs.

## IDs

- Format: `D-` followed by 4 Crockford base32 characters, for example `D-7KQ2` (D-11JK).
- Alphabet: `0123456789ABCDEFGHJKMNPQRSTVWXYZ` (no I, L, O or U, so no look-alikes). Input is case-insensitive, and `I`/`L` read as `1`, `O` as `0`, as Crockford specifies.
- Generated randomly where the decision is made. No coordination, no allocation, no "next number".
- Never all digits, so a new ID can't be mistaken for, or clash with, an old sequential number kept as an alias (D-P5PT). IDs created before this rule (D-0217, D-1371) keep their IDs.
- 4 characters gives about a million IDs. At hundreds of decisions per project a collision is very unlikely, and the tool checks every branch and worktree it can see before using an ID, retrying on a clash. `inloop lint` catches anything that slips through on separate machines.
- An ID never changes once created. References in code, docs and commit messages stay valid forever.
- The prefix and length are configurable (`[decisions]` in `inloop.toml`), for projects that want more characters.

## One file per decision

Decisions live in the repo (D-1X24), one Markdown file each (D-1371), in `docs/decisions/` by default (D-SJBG):

```
docs/decisions/D-7KQ2-snap-writes-go-through-one-helper.md
```

The slug after the ID is for humans browsing the folder; the ID alone identifies the decision. Two agents never edit the same file, so recording decisions in parallel never causes a merge conflict.

### Format

```markdown
---
id: D-7KQ2
title: Snap writes go through one helper
status: accepted
date: 2026-11-14
decided_by: Liam
basis: "Keep every Snap write in one helper so the confirm check lives in one place."
session: 9f2c0a1e
supersedes: []
superseded_by: null
aliases: []
tags: [snap, writes]
---

## Context

Why this came up.

## Decision

What was decided, in a few sentences.

## Consequences

What follows from it, including what it rules out.
```

| Field | Meaning |
|---|---|
| `id` | The decision's ID; must match the filename |
| `title` | One line, stated as the decision itself |
| `status` | `proposed`, `accepted`, `rejected` or `superseded` |
| `date` | Date recorded, `YYYY-MM-DD` |
| `decided_by` | The human who made it, or `agent` for a proposal |
| `basis` | Required for `accepted`: a quote of the human's words, or a pointer to where they said it |
| `session` | The session it was recorded in, when known |
| `supersedes` / `superseded_by` | Links between a decision and the one that replaced it |
| `aliases` | Other IDs that resolve to this decision, such as an old sequential number (D-P5PT) |
| `tags` | Free-form, for search and filtering |
| `promoted_by` | Who accepted or rejected a proposed decision: a person's name, or `agent`. Written only on promotion (D-B2NX) |

## The basis rule

A decision is something a human decided, not something an agent chose along the way (D-BY48).

- Any agent can record a decision.
- It is `accepted` only when a human on the project made it, or it is unmistakably clear from what they said. The `basis` field quotes them. The tool refuses to create an `accepted` decision without a `basis`.
- A choice the agent made itself, however sensible, is recorded as `proposed` with `decided_by: agent`. A human accepts or rejects it later.
- The MCP tool description for `decision_new` states this rule, because agents read tool descriptions on every call.

A human promotes proposed decisions, or an agent does it through MCP when a human tells it to, quoting the human in `basis`; every promotion records `promoted_by` (D-B2NX). Whether a PM agent may promote during review is still open (docs/08).

Writes go through the redactor field by field: the body as text, and each frontmatter value on its own, re-quoted if it changed, so a `[REDACTED:...]` marker can never break the YAML.

## Changing a decision

Accepted decisions are not rewritten. A new decision supersedes the old one: the new file lists the old ID in `supersedes`, and the tool sets `superseded_by` and `status: superseded` on the old file in the same change. Small corrections (a typo, a broken link) are fine to edit in place.

## Seeing decisions from other worktrees

Search and listing include decisions that exist only on other branches and worktrees, by default (D-8K5D). They are always marked unmerged, with the branch, the worktree path and (with the PM layer) the task. A decision on an unmerged branch is not true on main yet; agents must treat it as work in progress, not settled fact.

## Linting

`inloop lint` checks:

- every file parses, and its `id` matches its filename
- no two decisions share an ID (across every branch it can see), and no alias equals any decision's ID
- `accepted` decisions have a `basis`
- references (`D-XXXX` in docs and code) point at decisions that exist
- references to superseded decisions, flagged so the citing text can be updated

References are checked in files matching `[decisions] lint_references` (all Markdown by default). IDs used only as examples in docs go in `[decisions] example_ids`.

## Migrating from a single numbered file

Projects already using a single `decisions.md` with sequential numbers (Adpatch and Cirkit both do) move over once, with a throwaway script, not a compatibility mode (D-6QJK):

1. Split each entry into its own file with a new random ID.
2. Put the old number in `aliases`, so `inloop dec show D-264` still works for old commit messages, which can't be rewritten (D-P5PT).
3. Search and replace the old numbers across the repo's docs and code with the new IDs.
4. Delete the old file.

After that the project is fully on InLoop. The script is [`tools/migrate_decisions.py`](../tools/migrate_decisions.py): it reads entries shaped `## D-nnn (YYYY-MM-DD) Title`, turns whole-line `Superseded by D-nnn.` markers into links, writes a `MIGRATION-MAP.txt`, then runs `inloop scan --fix` and `inloop lint`. Run it with `--dry-run` first. Entries default to `accepted` with a basis pointing at the original entry; `--status proposed` is there for logs that mix in choices no human made. It lives outside the main binary and is not maintained after the migrations.

## Scale

Thousands of small files are fine for git and for full-text search. What gets harder at scale is relevance, which is the index's job. If a project ever finds the folder crowded, superseded and rejected decisions can move to an `archive/` subfolder. Not built until needed (D-GFRF).
