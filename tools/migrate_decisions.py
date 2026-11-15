#!/usr/bin/env python3
"""One-off migration from a single numbered decisions.md to InLoop's one file
per decision (D-6QJK). A throwaway script, not part of the inloop binary, and
not maintained once the migrations are done.

Expects entries shaped like:

    ## D-264 (2026-10-31) Title of the decision

    Body...
    Superseded by D-301.

What it does, in one run (use --dry-run first):

1. Splits each entry into docs/decisions/D-XXXX-slug.md with a new random ID
   (never all digits), keeping the old number in `aliases` (D-P5PT) so
   `inloop dec show D-264` still works for old commit messages.
2. Turns whole-line "Superseded by D-nnn." markers into supersede links.
3. Rewrites D-nnn references across the repo's tracked text files, expanding
   "D-nnn to D-nnn" ranges into the IDs they covered, and rebases relative
   links in each entry onto the new folder.
4. Deletes the old file.
5. Runs `inloop scan --fix` so every new file has been through the redactor
   (D-G1R6), then `inloop lint`.

Usage:
    python3 tools/migrate_decisions.py /path/to/repo [--source docs/decisions.md]
        [--dir docs/decisions] [--status accepted|proposed] [--decided-by NAME]
        [--dry-run]

Status: entries in an existing log are the project's settled decisions, so
the default is `accepted` with a basis that points at the original entry.
Use --status proposed if the log mixes in choices no human made.
"""

import argparse
import json
import os
import re
import secrets
import subprocess
import sys

ALPHABET = "0123456789ABCDEFGHJKMNPQRSTVWXYZ"
ENTRY = re.compile(r"^## (D-\d+) \((\d{4}-\d{2}-\d{2})\) (.+?)\s*$")
SUPERSEDED = re.compile(r"^\s*Superseded by (D-\d+)\.?\s*$", re.IGNORECASE)


def new_id(taken):
    while True:
        body = "".join(secrets.choice(ALPHABET) for _ in range(4))
        if body.isdigit():
            continue
        i = "D-" + body
        if i not in taken:
            taken.add(i)
            return i


def slugify(title, limit=60):
    words, n = [], 0
    for w in re.split(r"[^A-Za-z0-9]+", title):
        if not w:
            continue
        w = w.lower()
        extra = len(w) + (1 if words else 0)
        if words and n + extra > limit:
            break
        words.append(w)
        n += extra
    return "-".join(words)


def yaml_str(s):
    return json.dumps(s, ensure_ascii=False)


def parse(text):
    entries, cur = [], None
    for line in text.splitlines():
        m = ENTRY.match(line)
        if m:
            cur = {"old": m.group(1), "date": m.group(2), "title": m.group(3), "body": []}
            entries.append(cur)
        elif line.startswith("## ") and cur is not None:
            # A heading that isn't an entry ends the current entry's body.
            cur = None
        elif cur is not None:
            cur["body"].append(line)
    for e in entries:
        e["body"] = "\n".join(e["body"]).strip("\n")
    return entries


def git(repo, *args):
    return subprocess.run(["git", "-C", repo, *args], check=True, capture_output=True, text=True).stdout


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("repo")
    ap.add_argument("--source", default="docs/decisions.md")
    ap.add_argument("--dir", default="docs/decisions")
    ap.add_argument("--status", choices=["accepted", "proposed"], default="accepted")
    ap.add_argument("--decided-by", default=None, help="defaults to the repo's git user.name")
    ap.add_argument("--inloop", default="inloop", help="the inloop binary to run afterwards")
    ap.add_argument("--dry-run", action="store_true")
    a = ap.parse_args()

    repo = os.path.abspath(a.repo)
    src = os.path.join(repo, a.source)
    text = open(src, encoding="utf-8").read()
    entries = parse(text)
    if not entries:
        sys.exit(f"no entries like '## D-001 (2026-01-01) Title' found in {src}")
    olds = [e["old"] for e in entries]
    dupes = {o for o in olds if olds.count(o) > 1}
    if dupes:
        sys.exit(f"duplicate numbers in {a.source}: {', '.join(sorted(dupes))}; fix them first")

    out_dir = os.path.join(repo, a.dir)
    taken = set()
    if os.path.isdir(out_dir):
        taken |= {f[:6] for f in os.listdir(out_dir) if f.startswith("D-")}
    mapping = {e["old"]: new_id(taken) for e in entries}

    decided_by = a.decided_by
    if decided_by is None:
        try:
            decided_by = git(repo, "config", "user.name").strip() or "unknown"
        except subprocess.CalledProcessError:
            decided_by = "unknown"

    # Supersede links from whole-line markers.
    superseded_by, supersedes = {}, {}
    for e in entries:
        for line in e["body"].splitlines():
            m = SUPERSEDED.match(line)
            if m and m.group(1) in mapping:
                superseded_by[e["old"]] = m.group(1)
                supersedes.setdefault(m.group(1), []).append(e["old"])

    ref = re.compile(r"\bD-(\d{3,})\b")

    # "D-077 to D-087" means nothing once IDs are random: expand a range into
    # the migrated IDs it covered, leaving it alone if it covered none.
    span = re.compile(r"\bD-(\d{3,}) to D-(\d{3,})\b")
    by_num = {int(o[2:]): n for o, n in mapping.items()}

    def expand(m):
        lo, hi = int(m.group(1)), int(m.group(2))
        ids = [by_num[k] for k in range(lo, hi + 1) if k in by_num]
        return ", ".join(ids) if lo <= hi and ids else m.group(0)

    def rewrite(s):
        return ref.sub(lambda m: mapping.get(m.group(0), m.group(0)), span.sub(expand, s))

    # Relative links in an entry were written from the old file's folder; the
    # new files live in out_dir, so rebase them.
    link = re.compile(r"(\]\()([^)\s]+)(\))")
    src_dir = os.path.dirname(src)

    def rebase(target):
        if re.match(r"^(?:[a-z][a-z0-9+.-]*:|#|/)", target, re.I):
            return target
        path, frag = (target.split("#", 1) + [""])[:2]
        new = os.path.relpath(os.path.normpath(os.path.join(src_dir, path)), out_dir)
        return new + ("#" + frag if frag else "")

    def rebase_links(s):
        return link.sub(lambda m: m.group(1) + rebase(m.group(2)) + m.group(3), s)

    files = {}
    for e in entries:
        nid = mapping[e["old"]]
        if e["old"] in superseded_by:
            status = "superseded"
        else:
            status = a.status
        basis = (
            f"Migrated from {a.source} entry {e['old']} ({e['date']}), recorded there as a settled decision."
            if a.status == "accepted"
            else None
        )
        fm = [
            "---",
            f"id: {nid}",
            f"title: {yaml_str(e['title'])}",
            f"status: {status}",
            f"date: {e['date']}",
            f"decided_by: {yaml_str(decided_by) if a.status == 'accepted' else 'agent'}",
            f"basis: {yaml_str(basis) if basis else 'null'}",
            "session: null",
            "supersedes: [" + ", ".join(mapping[o] for o in supersedes.get(e["old"], [])) + "]",
            f"superseded_by: {mapping[superseded_by[e['old']]] if e['old'] in superseded_by else 'null'}",
            f"aliases: [{e['old']}]",
            "tags: [migrated]",
            "---",
            "",
        ]
        body = rebase_links(rewrite(e["body"])).strip("\n")
        name = f"{nid}-{slugify(e['title'])}.md"
        files[os.path.join(out_dir, name)] = "\n".join(fm) + body + "\n"

    # References elsewhere in the repo.
    tracked = [p for p in git(repo, "ls-files", "-z").split("\0") if p and p != a.source]
    changed = {}
    for p in tracked:
        full = os.path.join(repo, p)
        try:
            if os.path.getsize(full) > 2_000_000:
                continue
            s = open(full, encoding="utf-8").read()
        except (UnicodeDecodeError, OSError):
            continue
        r = rewrite(s)
        if r != s:
            changed[full] = r

    print(f"{len(entries)} decisions, {len(superseded_by)} supersede links, {len(changed)} other files with references")
    for e in entries[:5]:
        print(f"  {e['old']} -> {mapping[e['old']]}  {e['title'][:70]}")
    if len(entries) > 5:
        print(f"  ... and {len(entries) - 5} more")
    for p in sorted(changed)[:20]:
        print(f"  rewrite {os.path.relpath(p, repo)}")
    if a.dry_run:
        print("dry run: nothing written")
        return

    os.makedirs(out_dir, exist_ok=True)
    for p, s in files.items():
        with open(p, "x", encoding="utf-8") as f:
            f.write(s)
    for p, s in changed.items():
        with open(p, "w", encoding="utf-8") as f:
            f.write(s)
    os.remove(src)
    with open(os.path.join(out_dir, "MIGRATION-MAP.txt"), "w", encoding="utf-8") as f:
        f.write("# Old number -> new ID. Delete this file once nobody needs it.\n")
        for e in entries:
            f.write(f"{e['old']} {mapping[e['old']]}\n")

    # Everything written goes through the redactor, then lint.
    for args in (["scan", "--fix"], ["lint"]):
        r = subprocess.run([a.inloop, "-C", repo, *args])
        if r.returncode != 0:
            print(f"`inloop {' '.join(args)}` reported problems; see above", file=sys.stderr)
    print("done: review with `git status` and `inloop dec list`, then commit")


if __name__ == "__main__":
    main()
