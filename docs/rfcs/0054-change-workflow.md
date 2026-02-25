# RFC 0054: Change Workflow

- **Status**: Draft
- **Created**: 2026-02-24
- **Absorbs**: [RFC-0001](0001-command-punning.md) (command punning), [RFC-0021](0021-local-change-management.md) (local change management)
- **Related**: [RFC-0052](0052-manifest-lifecycle.md) (manifest lifecycle), [RFC-0053](0053-bootstrap-and-repo-discovery.md) (bootstrap), [RFC-0019](0019-cron-able-sync.md) (sync)

## Summary

This RFC defines a single change workflow: **all changes write to repo manifests
and flow through git**. Every `bkt` command that mutates state updates
`manifests/*.json` and, for Tier 2, applies the change immediately. The result
is always visible in `git diff manifests/`. You commit when ready; the PR
workflow makes changes permanent.

## Motivation

The old `--local` and ephemeral-manifest workflow created hidden state and
extra steps:

- Changes could exist only in `~/.local/share/bkt/` and never show up in PRs.
- Promotion required special commands instead of normal git tools.
- Users had two sources of truth: repo manifests and a local overlay.
- Review and automation were unreliable because `git diff` was incomplete.

Git already solves this. If every change goes to the repo, `git status` and
`git diff manifests/` become the single, reliable change surface.

## Command Punning (Dual-Write)

Command punning is the default behavior for Tier 2 subsystems:

- Update the repo manifest in `manifests/*.json`.
- Apply the change to the running system immediately.

Examples:

- `bkt flatpak add` updates `manifests/flatpak-apps.json` and installs the app.
- `bkt extension disable` updates `manifests/gnome-extensions.json` and disables it.
- `bkt gsetting set` updates `manifests/gsettings.json` and applies the value.

The manifest change is always visible in `git diff manifests/`.

## The PR Workflow

The repo is the source of truth, and git is the workflow engine.

- **Default**: change is written to the working tree; you commit manually.
- **`--pr`**: change is written on a new branch and a PR is created.
- **`--pr-only`**: change is written on a new branch and a PR is created, but
  the change is not applied locally.

The PR workflow uses the `gh` CLI. `bkt doctor` validates that `gh auth` is
configured before attempting PR creation.

## Why `--local` and the Ephemeral Manifest Are Gone

`--local` is removed. It previously meant "apply but do not PR" and required a
separate ephemeral manifest. In the new design:

- Every change writes to repo manifests, always.
- If you do not want a PR, simply do not create one.
- Use git for lifecycle control: `git status`, `git diff manifests/`,
  `git add manifests/`, and `git checkout manifests/`.

There is no separate "local changes" concept. There are only uncommitted
changes in your repo.

## Capture as Git Diff

`bkt capture` reads system state and writes it to repo manifests. The output is
a normal `git diff` showing drift:

- Review changes with `git diff manifests/`.
- Commit what you want.
- Discard what you do not with `git checkout manifests/`.

This replaces the old "capture to user manifest, then promote" flow.

## Sync (Bidirectional Convergence)

`bkt sync` is a convenience command that runs `bkt capture` followed by
`bkt apply`:

1. Capture reality into manifests (reality wins for what changed on the system).
2. Apply manifests to the system (manifest wins for what did not change).

When there is no drift, `bkt sync` is idempotent and does nothing. It is safe
to run frequently (cron or timer) and leaves a `git diff` when changes occur.

## Implementation: Code Changes Required

1. Remove the ephemeral manifest storage and readers.
2. Remove the `--local` flag from all commands and help text.
3. Remove the `bkt local` subcommand and its documentation.
4. Update all subsystem writers to target `manifests/*.json` in the repo.
5. Ensure all manifest writes resolve the repo via `find_repo_path()`.
6. Implement `--pr-only` across punning commands and `bkt capture`.
7. Define `bkt sync` as `capture` followed by `apply` with clear ordering.
8. Update tests to assert git-visible diffs rather than ephemeral files.

## Migration

Existing ephemeral files may be present at:

- `~/.local/share/bkt/ephemeral.json`
- `~/.local/state/bkt/ephemeral.json`

Migration behavior:

- `bkt` ignores these files and warns once if they exist.
- The warning explains that ephemeral manifests are deprecated and can be
  removed.
- Users who want to preserve local changes should re-apply them as normal
  `bkt` commands so they land in `manifests/`.
