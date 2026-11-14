# {{task.id}}: {{task.title}}

You are working on one task for {{project}}, in your own worktree on branch `{{task.branch}}`. Another agent (the project manager) reviews your work, and a human decides when it lands.

## The task

{{task.description}}

## Before you start

- Read `CLAUDE.md` and the docs it points to for the areas you will change.
- Search the project's decisions (`decision_search`) for anything that bears on this task, and follow them.

## Paths you may change

{{task.touches}}

Stay inside these paths. If the task needs changes elsewhere, stop and report it with `task_blocked` rather than spreading out; another task may own those files.

## Decisions relevant to this task

{{decisions}}

## While you work

- Record design choices with `decision_new`. Record one as `accepted` only when a human made it, quoting them in `basis`; anything you chose yourself is `proposed`.
- Keep commits small and on `{{task.branch}}`. Don't push, merge or rebase onto main; the project manager handles landing once the human agrees.

## Checks

Run these before you report, and fix what fails:

{{checks}}

## When you finish

{{report_format}}
