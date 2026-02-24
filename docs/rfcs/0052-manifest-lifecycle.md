# RFC 0052: Manifest Lifecycle and Repo Source of Truth

- **Status**: Draft
- **Created**: 2026-02-24
- **Absorbs**: [RFC-0007](0007-drift-detection.md) (drift detection), [RFC-0014](canon/0014-extension-state-management.md) (extension state management)
- **Related**: [RFC-0004](0004-bkt-admin.md) (Tier 1), [RFC-0048](0048-subsystem-and-containerfile-unification.md) (subsystem unification), [RFC-0053](0053-bootstrap-and-repo-discovery.md) (bootstrap), [RFC-0054](0054-change-workflow.md) (change workflow)

## Summary

This RFC defines the manifest lifecycle with a single source of truth: the repo.
All manifests live in `manifests/` and are the authoritative state for every
subsystem. There is no user manifest layer and no system+user merge. `bkt apply`
converges the system to manifests, `bkt capture` converges manifests to the
system, and `bkt drift` reports the gap between them.

Design principles:

- **Single source of truth**: repo manifests are authoritative for all tiers.
- **Visible change**: all state changes result in a `git diff`.
- **Uniform architecture**: every subsystem implements apply, capture, drift.
- **Explicit migration**: no silent state in user config directories.
- **Idempotence**: apply and capture are safe to run repeatedly.

## Motivation

The current system+user merge model creates invisible state:

- User manifests in `~/.config/bootc/` silently override repo manifests.
- Changes can happen without a `git diff`, breaking review and reproducibility.
- Some subsystems read from the repo while others merge system+user, which
  makes behavior inconsistent and hard to reason about.

This breaks expected workflows:

- **Review**: changes do not show up in PRs if they live only in user state.
- **Rebuild**: image rebuilds can diverge from the runtime without notice.
- **Support**: bug reports are missing the real configuration.
- **Automation**: scripts cannot predict whether repo or user state wins.

The repo already exists to store desired state. This RFC makes that explicit:
**the repo is the single source of truth**, and all subsystems share one
architecture.

## The Tier Model

Two tiers of state share a common workflow but have different lifecycles:

| Tier | State Location | Change Mechanism | Drift Possible? |
| --- | --- | --- | --- |
| **Tier 1 (Image-bound)** | Baked into image | PR -> build -> reboot | No (image is deterministic) |
| **Tier 2 (Runtime)** | Live system state | Immediate | Yes (runtime can diverge) |

**Tier 1 (Image-bound)**
- System packages
- Kernel arguments
- Systemd presets

Managed by `bkt system` and `bkt admin`. Changes update manifests and the
Containerfile, but only take effect after image rebuild and reboot.

**Tier 2 (Runtime)**
- Flatpaks
- GNOME extensions
- GSettings
- Shims
- Distrobox
- AppImages
- Homebrew

Changes update manifests and apply immediately. Runtime can drift from manifests.

## Manifest Format

- Canonical location: `manifests/*.json` in the repo.
- All manifests are git-tracked.
- `find_repo_path()` locates the repo root (see RFC-0053).
- `schemas/` defines validation for each manifest.

Manifests are structured per subsystem and remain stable across tiers:

- `manifests/system-packages.json`
- `manifests/kernel-args.json`
- `manifests/systemd-services.json`
- `manifests/flatpak-apps.json`
- `manifests/gnome-extensions.json`
- `manifests/gsettings.json`
- `manifests/shims.json`
- `manifests/distrobox.json`
- `manifests/appimage-apps.json`
- `manifests/homebrew.json`

There is no user manifest layer and no merge logic. Every subsystem reads from
and writes to the repo manifests directly.

## Apply (manifest -> system)

`bkt apply` reads manifests from the repo and makes the system match.
It is idempotent: running it twice yields the same result.

Tier behavior:
- **Tier 1**: `bkt apply` has no effect — Tier 1 changes are already in the
  manifests and Containerfile, but only take effect after image rebuild and
  reboot. `bkt apply` does not trigger rebuilds.
- **Tier 2**: `bkt apply` installs/enables/sets runtime state to match the
  manifest immediately.

Apply is the normal path for reproducibility:

- it does not edit manifests
- it does not guess intent
- it only converges the system toward the declared state

## Capture (system -> manifest)

`bkt capture` reads system state and writes the result to repo manifests.
This is how reality wins. Manual changes show up in `git diff` and can be
reviewed.

Capture is the inverse of apply:
- Apply moves system toward manifests.
- Capture moves manifests toward system.

Capture rules:

- it does not change the system
- it updates only repo manifests
- it is explicit, never implicit

## Drift Detection (the gap)

Drift is the difference between manifest intent and system reality. `bkt drift`
reports that gap without changing anything. Conceptually it is a dry-run of
both apply and capture at the same time.

- **Additive drift**: system has items not in the manifest.
- **Subtractive drift**: manifest declares items not on the system.
- **Modificational drift**: values differ (for example, GSettings).

Drift applies to Tier 2 only. Tier 1 is image-bound; if the image is wrong the
build should fail rather than drift at runtime.

## Extension State

Extension enable/disable is a manifest concept. The manifest supports both
legacy strings (enabled) and explicit objects:

```json
{
  "extensions": [
    "appindicatorsupport@rgcjonas.gmail.com",
    { "id": "dash-to-dock@micxgx.gmail.com", "enabled": false }
  ]
}
```

- String -> enabled by default.
- Object with `"enabled": false` -> installed but disabled.

Commands:
- `bkt extension disable` updates the manifest and disables the extension.
- `bkt extension capture` records enabled/disabled state into the manifest.

## Subsystem Consistency

All subsystems follow the same architecture and read from repo manifests.
There is no split between repo-reading and system+user merge subsystems.

Subsystems include (non-exhaustive):
- Tier 1: system packages, kernel arguments, systemd presets
- Tier 2: flatpak, extension, gsetting, shim, distrobox, appimage, homebrew

There are no special-case manifest layers per subsystem. Every subsystem uses
the same repo path resolution, same manifest storage, and same apply and
capture semantics.

## Implementation: Code Changes Required

Concrete changes to eliminate the user manifest layer:

1. Remove `~/.config/bootc/` manifest discovery and merge logic.
2. Update manifest loaders to read only from `manifests/` in the repo root.
3. Update writers (`bkt add`, `bkt remove`, `bkt capture`, `bkt extension`) to
   write only to repo manifests.
4. Ensure `find_repo_path()` is the single entry point for locating manifests.
5. Remove any CLI flags or env vars that target the user manifest directory.
6. Update documentation and help text to reflect repo-only manifests.
7. Add validation failures when repo manifests are missing or malformed.

Follow-up changes implied by this RFC:

- Remove system+user merge code paths from flatpak, extension, gsettings, shim.
- Align distrobox and appimage subsystems with the same apply and capture flow.
- Update tests to assume repo manifests only.

## Migration

Existing user manifests in `~/.config/bootc/` must be migrated into the repo.

Proposed behavior:

- If user manifests exist, `bkt` warns and offers a migration command.
- Migration copies files into `manifests/`, then removes or archives the user
  copies to avoid shadow state.
- If both repo and user manifests exist, repo wins and a conflict is reported
  for manual resolution.

Migration command shape:

- `bkt migrate manifests` moves `~/.config/bootc/*.json` into the repo.
- `--dry-run` prints the planned changes and expected `git diff`.
- `--force` overwrites conflicting files after explicit confirmation.

After migration, `~/.config/bootc/` is no longer used by any subsystem.

## Deliberately Omitted

The following features from RFC-0007 are not carried forward into this RFC.
They may be proposed separately if needed:

- **Interactive drift resolve** (`bkt drift resolve` with per-item prompts)
- **`.bktignore`** (patterns to suppress drift warnings)
- **Drift monitoring** (systemd timer for periodic drift reports)
- **Drift report format** (structured JSON output for automation)
- **Systemd service state** as a Tier 2 domain (`systemd-services.json`) —
  the concept is valid but needs its own RFC with a concrete command surface

These are useful features but orthogonal to the architectural change (repo as
single source of truth) that this RFC establishes.
