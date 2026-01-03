# Current Work: Phase 2 — Distribution Management

This document tracks Phase 2 of the bootc distribution development. The previous phase (Bootstrap) is archived at [docs/history/001-bootstrap.md](docs/history/001-bootstrap.md).

---

## Vision

**Phase 1** established `bkt` as a tool for managing declarative manifests (Flatpaks, extensions, gsettings, shims). Users edit JSON files or use CLI commands, which sync to the system and open PRs to the distribution.

**Phase 2** extends `bkt` to manage the **entire distribution lifecycle** with **full bidirectional sync**:

1. **Apply**: `bkt apply` applies everything from manifests to the running system
2. **Capture**: `bkt capture` imports all system changes back into manifests
3. **Command Punning**: Familiar CLI patterns (`dnf install`, `gsettings set`) that execute immediately AND propagate to the distribution
4. **Context-Aware Execution**: `bkt dev` for toolbox, default for host, `bkt image` for build-time only
5. **Privileged Operations**: Passwordless access to read-only operations via `bkt-admin`
6. **Changelog Management**: Auto-generated, CI-enforced changelog with manual step tracking
7. **Upstream Management**: Unified dependency manifest with semver-aware updates
8. **Drift Detection**: Explicit assumptions about base image, verified in CI

The guiding principle: **You are maintaining your own distribution.** Every local change should persist. Every persistent change should be auditable. The system should protect you from silent breakage.

### The Bidirectional Sync Loop

```
┌─────────────────┐                    ┌─────────────────┐
│   MANIFESTS     │ ──── bkt apply ──→ │     SYSTEM      │
│  (git-tracked)  │ ←── bkt capture ── │  (live state)   │
└─────────────────┘                    └─────────────────┘
```

- **`bkt apply`**: Install flatpaks, enable extensions, set gsettings, install packages from manifests
- **`bkt capture`**: Import flatpaks installed via GNOME Software, extensions enabled via Extension Manager, settings changed via UI, packages layered via rpm-ostree

Both commands support `--dry-run` to preview changes without executing them.

---

## Overview

| ID  | Item                                                | RFC                                                  | Priority  | Status       |
| --- | --------------------------------------------------- | ---------------------------------------------------- | --------- | ------------ |
| 1   | [Command Punning Foundation](#1-command-punning)    | [RFC-0001](docs/rfcs/0001-command-punning.md)        | 🔴 High   | ✅ Complete  |
| 2   | [RPM Package Management](#2-rpm-package-management) | [RFC-0002](docs/rfcs/0002-bkt-dnf.md)                | 🔴 High   | 🔄 Core Done |
| 3   | [Toolbox Commands](#3-toolbox-commands)             | [RFC-0003](docs/rfcs/0003-bkt-dev.md)                | 🔴 High   | 🔄 Core Done |
| 4   | [Privileged Helper](#4-privileged-helper)           | [RFC-0004](docs/rfcs/0004-bkt-admin.md)              | 🟡 Medium | Not Started  |
| 5   | [Changelog Management](#5-changelog-management)     | [RFC-0005](docs/rfcs/0005-changelog.md)              | 🟡 Medium | 🔄 Core Done |
| 6   | [Upstream Management](#6-upstream-management)       | [RFC-0006](docs/rfcs/0006-upstream-management.md)    | 🟡 Medium | 🔄 Core Done |
| 7   | [Base Image Drift Detection](#7-drift-detection)    | [RFC-0007](docs/rfcs/0007-drift-detection.md)        | 🟢 Low    | 🔄 Core Done |
| 8   | [Validation on Add](#8-validation-on-add)           | —                                                    | 🟢 Low    | ✅ Complete  |
| 9   | [Command Infrastructure](#9-command-infrastructure) | [RFC-0008](docs/rfcs/0008-command-infrastructure.md) | 🔴 High   | ✅ Complete  |
| 10  | [Bidirectional Sync](#10-bidirectional-sync)        | —                                                    | 🔴 High   | ✅ Complete  |

> **Status Legend:** ✅ Complete = all deliverables done | 🔄 Core Done = main features work, sub-items remain | Not Started = no implementation

---

## 1. Command Punning Foundation

**RFC:** [0001-command-punning.md](docs/rfcs/0001-command-punning.md)  
**Priority:** 🔴 High  
**Status:** ✅ Complete

### Description

Establish the core infrastructure for command punning: the pattern where `bkt` commands execute immediately AND propagate changes to the distribution.

### Deliverables

- [x] Refactor `bkt` CLI to support execution contexts (host/dev/image)
- [x] Implement `--pr` / `--local` / `--pr-only` flags consistently across all commands
- [x] Add context detection (in-toolbox vs host)
- [x] Standardize manifest update + PR creation pipeline
- [x] Document the punning philosophy in README

### Acceptance Criteria

- ✅ Running `bkt flatpak add org.gnome.Boxes` installs immediately AND opens a PR
- ✅ Running `bkt flatpak add org.gnome.Boxes --local` installs without PR
- ✅ Running `bkt flatpak add org.gnome.Boxes --pr-only` only opens PR (no local install)

---

## 2. RPM Package Management

**RFC:** [0002-bkt-dnf.md](docs/rfcs/0002-bkt-dnf.md)  
**Priority:** 🔴 High  
**Status:** 🔄 Core Done

### Description

Implement `bkt dnf` as a punned command layer for RPM packages on atomic systems.

### Deliverables

- [x] Implement query pass-through (`bkt dnf search`, `info`, `provides`, `list`)
- [x] Create `manifests/system-packages.json` schema and manifest
- [x] Implement `bkt dnf install` (rpm-ostree + manifest + Containerfile PR)
- [x] Implement `bkt dnf remove`
- [ ] Add Containerfile section markers for managed packages
- [x] Implement package validation (check if package exists before adding)

### Acceptance Criteria

- ✅ `bkt dnf search htop` returns results from dnf5
- ✅ `bkt dnf install htop` runs `rpm-ostree install htop` AND updates manifest AND opens PR
- ❌ Containerfile `dnf install` block is auto-regenerated from manifest

---

## 3. Toolbox Commands

**RFC:** [0003-bkt-dev.md](docs/rfcs/0003-bkt-dev.md)  
**Priority:** 🔴 High  
**Status:** 🔄 Core Done

### Description

Implement `bkt dev` prefix for commands that target the development toolbox.

### Deliverables

- [x] Create `manifests/toolbox-packages.json` schema and manifest
- [x] Implement `bkt dev dnf install/remove/list`
- [x] Implement toolbox detection (running in toolbox vs host)
- [x] Implement `bkt dev enter` shortcut
- [x] Implement `bkt dev update` (sync toolbox to manifest)
- [x] Add validation for invalid combinations (`bkt dev flatpak` → error)

### Known Issues

- ⚠️ `bkt dev dnf install` currently updates `system-packages.json` instead of `toolbox-packages.json`

### Acceptance Criteria

- ✅ `bkt dev dnf install gcc` installs gcc in toolbox immediately
- ⚠️ `manifests/toolbox-packages.json` is updated with package entry (bug: wrong manifest updated)
- ✅ Running `bkt dev flatpak add ...` produces helpful error

---

## 4. Privileged Helper

**RFC:** [0004-bkt-admin.md](docs/rfcs/0004-bkt-admin.md)  
**Priority:** 🟡 Medium  
**Status:** Not Started

### Description

Implement `bkt admin` for passwordless privileged operations using **polkit + pkexec** (replaces original setuid approach).

### Approach (Approved)

**For bootc/rpm-ostree** (no D-Bus interface):
- Use `pkexec bootc <cmd>` via `flatpak-spawn --host`
- Polkit rules grant passwordless access to wheel group

**For systemctl** (has D-Bus + polkit):
- Use systemd's D-Bus API with `zbus` crate
- Polkit automatically handles authorization

### Deliverables

- [ ] Create polkit rules file (`system/polkit-1/rules.d/50-bkt-admin.rules`)
- [ ] Implement `bkt admin bootc` commands (status, upgrade, rollback, switch)
- [ ] Implement `bkt admin systemctl` commands via D-Bus (start, stop, restart, status)
- [ ] Add `zbus` dependency for D-Bus integration
- [ ] Update Containerfile to install polkit rules
- [ ] Update RFC-0004 to document polkit approach

### Implementation Plan (Next Sprint)

**Day 1: Polkit rules + pkexec wrapper** (~6-8 hours)
- Create `system/polkit-1/rules.d/50-bkt-admin.rules`
- Create `bkt/src/commands/admin.rs`
- Implement `bkt admin bootc status/upgrade/rollback/switch` via pkexec
- Update Containerfile to copy polkit rules

**Day 2-3: D-Bus systemd integration** (~12-16 hours)
- Add `zbus` dependency to Cargo.toml
- Create `bkt/src/dbus/mod.rs` and `bkt/src/dbus/systemd.rs`
- Implement `bkt admin systemctl start/stop/restart/enable/disable/status`
- Test from toolbox context

**Day 4: Polish + RFC update** (~4 hours)
- Update RFC-0004 for polkit approach
- Add CLI tests
- Document security model

### Polkit Rules (Preview)

```javascript
// 50-bkt-admin.rules
polkit.addRule(function(action, subject) {
    if (action.id == "org.freedesktop.policykit.exec" &&
        subject.isInGroup("wheel")) {
        var program = action.lookup("program");
        if (program == "/usr/bin/bootc" || program == "/usr/bin/rpm-ostree") {
            return polkit.Result.YES;
        }
    }
});
```

### Acceptance Criteria

- `bkt admin bootc status` works without password from toolbox
- `bkt admin bootc upgrade` works with `--confirm` flag
- `bkt admin systemctl restart docker.service` works via D-Bus
- Non-wheel users are denied by polkit

---

## 5. Changelog Management

**RFC:** [0005-changelog.md](docs/rfcs/0005-changelog.md)  
**Priority:** 🟡 Medium  
**Status:** 🔄 Core Done (PR #9)

### Description

Implement structured changelog with auto-generation and CI enforcement.

### Deliverables

- [x] Create changelog YAML schema (ChangelogEntry, VersionMetadata)
- [x] Implement `bkt changelog generate` (preview changelog entries)
- [x] Implement `bkt changelog validate` (check pending entries)
- [x] Implement `bkt changelog show` (display CHANGELOG.md)
- [x] Implement `bkt changelog add` (add pending entries)
- [x] Implement `bkt changelog pending` (list pending entries)
- [x] Implement `bkt changelog list` (list released versions)
- [x] Implement `bkt changelog release` (create version from pending)
- [x] Implement `bkt changelog clear` (admin: clear pending entries)
- [ ] Add CI check: PR must have changelog entry
- [ ] Add CI check: No draft entries on merge
- [ ] Integrate changelog with `bkt status` output
- [ ] Create MOTD integration for first-boot "What's New"

### Acceptance Criteria

- PRs that change manifests get auto-generated changelog drafts
- PRs with `"draft": true` cannot merge
- `bkt status` shows current version changes and pending manual steps

---

## 6. Upstream Management

**RFC:** [0006-upstream-management.md](docs/rfcs/0006-upstream-management.md)  
**Priority:** 🟡 Medium  
**Status:** 🔄 Core Done

### Description

Consolidate scattered version pins into unified upstream manifest with semver policies.

### Deliverables

- [x] Create `upstream/manifest.json` schema
- [x] Migrate existing pins (starship, lazygit, keyd, bibata, whitesur, getnf)
- [x] Implement `bkt upstream list`
- [x] Implement `bkt upstream check` (show available updates)
- [x] Implement `bkt upstream update` (update within policy)
- [x] Implement `bkt upstream lock` (regenerate checksums)
- [x] Implement `bkt upstream verify` (verify all checksums)
- [ ] Update Containerfile to read from manifest (still uses old `.version` files)
- [ ] Generate changelog entries for updates
- [ ] Implement semver update policies
- [ ] Remove old `upstream/*.version` and `*.ref` files

### Acceptance Criteria

- ⚠️ All current `upstream/*.version` files replaced by single manifest (manifest exists, old files remain)
- ✅ `bkt upstream check` shows available updates with policy indicators
- ❌ `bkt upstream update` respects semver policies (not yet implemented)

---

## 7. Base Image Drift Detection

**RFC:** [0007-drift-detection.md](docs/rfcs/0007-drift-detection.md)  
**Priority:** 🟢 Low → 🟡 Medium (next sprint)  
**Status:** ✅ Complete (PRs #10, #18)

### Description

Explicitly declare and verify assumptions about the base image.

### Deliverables

- [x] Create `manifests/base-image-assumptions.json` schema
- [x] Document initial assumptions (bootc, flatpak, rpm-ostree, gnome-shell, polkit, etc.)
- [x] Implement `bkt base verify`
- [x] Implement `bkt base assume <package>`
- [x] Add CI workflow to verify assumptions
- [x] Add scheduled check against `:stable` and `:latest`
- [x] Integrate with changelog when assumptions change

### Implementation Summary

- Captured current system assumptions with `bkt base snapshot`.
- Reviewed and filtered to critical packages (flatpak, rpm-ostree, gnome-shell, polkit, etc.).
- Committed `manifests/base-image-assumptions.json`.
- Created `.github/workflows/verify-assumptions.yml`:
  - Runs `bkt base verify` on every PR/push.
  - Uses `ghcr.io/ublue-os/bazzite-gnome:stable` container.
  - Verifies assumptions BEFORE installing build deps (pristine check).
- Created `.github/workflows/check-upstream-drift.yml`:
  - Weekly scheduled check against `:stable` and `:latest`.
  - Uploads drift reports as artifacts.
  - Opens issues on detected breaking changes.
- Changelog integration via `bkt base assume` / `bkt base unassume`.
- Clarified manifest separation philosophy in RFC 0007 and manifests/README.md.

### Implementation Plan (Completed)

**Day 1: Document Assumptions**
- Run `bkt base snapshot` to capture current system assumptions
- Review and filter to ~20-30 critical packages (flatpak, rpm-ostree, gnome-shell, etc.)
- Commit `manifests/base-image-assumptions.json`

**Day 2-3: CI Workflows**
- Create `.github/workflows/verify-assumptions.yml`
  - Runs `bkt base verify` on every PR/push
  - Uses `ghcr.io/ublue-os/bazzite-gnome:stable` container
- Create `.github/workflows/check-upstream-drift.yml`
  - Weekly scheduled check against `:stable` and `:latest`
  - Uploads drift reports as artifacts
  - Opens issues on detected breaking changes

**Day 4: Changelog Integration**
- Auto-generate changelog entry when assumptions added/removed
- Hook into `bkt base assume` and `bkt base unassume`

### Acceptance Criteria

- CI fails if base image lacks assumed packages
- `bkt base verify` runs locally and shows clear pass/fail
- Scheduled job detects upcoming breaking changes

---

## 8. Validation on Add

**Priority:** 🟢 Low  
**Status:** ✅ Complete (PR #11)

### Description

Validate items before adding to manifests to prevent typos and invalid entries.

### Deliverables

- [x] Flatpak: Query remote to verify app exists (`flatpak remote-info`)
- [x] Extension: Check extensions.gnome.org API for UUID validity
- [x] GSettings: Verify schema exists (`gsettings list-schemas`)
- [x] DNF: Verify package exists before adding to manifest
- [x] Add `--force` flag to bypass validation when needed

### Acceptance Criteria

- `bkt flatpak add org.gnome.Nonexistent` fails with helpful suggestion ✅
- `bkt gsetting set nonexistent.schema key value` fails with schema list ✅
- All add commands validate before modifying manifests ✅

---

## 9. Command Infrastructure

**RFC:** [0008-command-infrastructure.md](docs/rfcs/0008-command-infrastructure.md)  
**Priority:** 🔴 High  
**Status:** ✅ Complete (PR #12)

### Description

Refactor command implementations to use a `Plan`-centric architecture where all operations are first computed as immutable plans, then optionally executed. Plans are first-class citizens that can be inspected, composed, and serialized.

### Core Concepts

```
┌─────────────┐     plan()      ┌─────────────┐    execute()    ┌─────────────┐
│   Command   │ ──────────────▸ │    Plan     │ ──────────────▸ │   Report    │
│  (config)   │   pure/no side  │ (immutable) │   side effects  │  (results)  │
└─────────────┘     effects     └─────────────┘                 └─────────────┘
                                      │
                                      ▼ describe()
                                ┌─────────────┐
                                │  Dry-Run    │
                                │   Output    │
                                └─────────────┘
```

- **Plannable trait**: Commands produce typed plans without side effects
- **Plan trait**: Immutable description of operations with `describe()` and `execute()`
- **CompositePlan**: Combine multiple plans into one (for `bkt apply`)
- **DynPlan**: Type-erased plans for heterogeneous composition

### Deliverables

- [x] Implement `Plannable` trait with associated `Plan` type
- [x] Implement `Plan` trait with `describe()`, `execute()`, `is_empty()`
- [x] Implement `PlanSummary` and `Operation` types for structured output
- [x] Implement `CompositePlan` for heterogeneous plan composition
- [x] Implement `DynPlan` for type-erased plan boxing
- [x] Implement `ExecuteContext` for controlled side effects
- [x] Implement `ExecutionReport` for unified result reporting
- [x] Refactor `flatpak sync` to use Plan pattern
- [x] Refactor `extension sync` to use Plan pattern
- [x] Refactor `gsetting apply` to use Plan pattern
- [x] Refactor `dnf sync` to use Plan pattern
- [x] Refactor `shim sync` to use Plan pattern
- [x] Implement `bkt apply` using CompositePlan with subsystem filtering

### Acceptance Criteria

- ✅ All sync commands implement `Plannable` trait
- ✅ `--dry-run` works uniformly across all commands via the trait
- ✅ Plans can be composed: `ApplyCommand` uses `CompositePlan` for subsystems
- ✅ No command contains `if dry_run { ... } else { ... }` branching
- ✅ Plan output is structured and consistent across all commands

---

## 10. Bidirectional Sync

**Priority:** 🔴 High  
**Status:** ✅ Complete (Apply ✅, Capture ✅, dnf capture ✅)

### Description

Implement the two meta-commands that complete the bidirectional sync loop: `bkt apply` (manifest → system) and `bkt capture` (system → manifest).

### Deliverables

#### Apply (manifest → system)

- [x] Implement `bkt apply` that runs all sync commands:
  - `bkt flatpak sync`
  - `bkt extension sync`
  - `bkt gsetting apply`
  - `bkt dnf sync`
  - `bkt shim sync`
- [x] Add `--dry-run` flag (uses Plan trait)
- [x] Add `--only` / `--exclude` flags for subsystem filtering
- [x] Show unified summary of all changes made via CompositePlan

#### Capture (system → manifest)

- [x] Implement `bkt flatpak capture` - import installed flatpaks not in manifest
- [x] Implement `bkt extension capture` - import enabled extensions not in manifest
- [x] Implement `bkt gsetting capture [schema]` - import changed settings
- [x] Implement `bkt dnf capture` - import rpm-ostree layered packages (PR #15)
- [x] Implement `bkt capture` that runs all capture commands
- [x] Add `--dry-run` flag (uses Plan trait)
- [ ] Add `--select` flag for interactive selection (future: TUI)

#### Status Dashboard (PR #13)

- [x] Integrate OS status from rpm-ostree
- [x] Show manifest status with counts per subsystem
- [x] Inline drift detection (pending sync + pending capture)
- [x] Next actions section with contextual suggestions
- [x] JSON output for scripting

### Acceptance Criteria

- ✅ `bkt apply` applies all manifests to running system in one command
- ✅ `bkt apply --dry-run` shows what would be installed/enabled without doing it
- ✅ `bkt capture` imports all detected system changes to manifests
- ✅ After installing a flatpak via GNOME Software, `bkt capture` adds it to manifest
- ✅ After enabling an extension via Extension Manager, `bkt capture` adds it to manifest

---

## Implementation Order

Recommended order based on dependencies:

```
Phase 2a: Bidirectional Sync ✅ COMPLETE
├── 9. Command Infrastructure ✅ Complete
├── 10a. Apply side (bkt apply) ✅ Complete
├── 10b. Status Dashboard (bkt status) ✅ Complete
├── 10c. Capture side (bkt capture) ✅ Complete (PR #14)
└── 10d. DNF capture (bkt dnf capture) ✅ Complete (PR #15)

Phase 2b: Supporting Infrastructure ← CURRENT SPRINT
│
├── Week 1: Drift Detection (Item 7)
│   ├── 7a. Document base image assumptions ← NEXT
│   ├── 7b. CI workflow (verify-assumptions.yml)
│   ├── 7c. Scheduled drift check workflow
│   └── 7d. Changelog integration for assumptions
│
└── Week 2: Privileged Helper (Item 4) - Polkit Approach
    ├── 4a. Polkit rules + pkexec for bootc/rpm-ostree
    ├── 4b. D-Bus systemd integration (zbus)
    └── 4c. RFC-0004 update + tests

Phase 2c: Polish
├── 5. Changelog sub-items (CI integration, MOTD)
├── 6. Upstream sub-items (Containerfile, semver policies)
└── Future considerations (TUI, multi-machine, etc.)
```

---

## Future Considerations

These items are out of scope for Phase 2 but identified for future phases:

### Multi-Machine Sync

Support managing multiple machines from a single manifest set with machine-specific overrides.

### Interactive TUI Mode

Terminal UI for browsing and toggling packages, extensions, and settings.

### `bkt init` Command

Bootstrap new user configuration with interactive prompts.

### Plugin System

Allow users to define custom manifest types without modifying `bkt` source.

### Remote Management

Manage remote machines via SSH with the same `bkt` commands.

---

## Appendix: RFC Index

| RFC                                                  | Title                            | Status |
| ---------------------------------------------------- | -------------------------------- | ------ |
| [RFC-0001](docs/rfcs/0001-command-punning.md)        | Command Punning Philosophy       | Draft  |
| [RFC-0002](docs/rfcs/0002-bkt-dnf.md)                | `bkt dnf` RPM Package Management | Draft  |
| [RFC-0003](docs/rfcs/0003-bkt-dev.md)                | `bkt dev` Toolbox Commands       | Draft  |
| [RFC-0004](docs/rfcs/0004-bkt-admin.md)              | `bkt-admin` Privileged Helper    | Draft  |
| [RFC-0005](docs/rfcs/0005-changelog.md)              | Changelog Management             | Draft  |
| [RFC-0006](docs/rfcs/0006-upstream-management.md)    | Upstream Dependency Management   | Draft  |
| [RFC-0007](docs/rfcs/0007-drift-detection.md)        | Base Image Drift Detection       | Draft  |
| [RFC-0008](docs/rfcs/0008-command-infrastructure.md) | Command Infrastructure (Plans)   | Draft  |
