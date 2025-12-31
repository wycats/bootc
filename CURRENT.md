# Current Work: Phase 2 — Distribution Management

This document tracks Phase 2 of the bootc distribution development. The previous phase (Bootstrap) is archived at [docs/history/001-bootstrap.md](docs/history/001-bootstrap.md).

---

## Vision

**Phase 1** established `bkt` as a tool for managing declarative manifests (Flatpaks, extensions, gsettings, shims). Users edit JSON files or use CLI commands, which sync to the system and open PRs to the distribution.

**Phase 2** extends `bkt` to manage the **entire distribution lifecycle**:

1. **Command Punning**: Familiar CLI patterns (`dnf install`, `gsettings set`) that execute immediately AND propagate to the distribution
2. **Context-Aware Execution**: `bkt dev` for toolbox, default for host, `bkt image` for build-time only
3. **Privileged Operations**: Passwordless access to read-only operations via `bkt-admin`
4. **Changelog Management**: Auto-generated, CI-enforced changelog with manual step tracking
5. **Upstream Management**: Unified dependency manifest with semver-aware updates
6. **Drift Detection**: Explicit assumptions about base image, verified in CI

The guiding principle: **You are maintaining your own distribution.** Every local change should persist. Every persistent change should be auditable. The system should protect you from silent breakage.

---

## Overview

| ID  | Item                                                    | RFC                                                  | Priority  | Status      |
| --- | ------------------------------------------------------- | ---------------------------------------------------- | --------- | ----------- |
| 1   | [Command Punning Foundation](#1-command-punning)        | [RFC-0001](docs/rfcs/0001-command-punning.md)        | 🔴 High   | Not Started |
| 2   | [RPM Package Management](#2-rpm-package-management)     | [RFC-0002](docs/rfcs/0002-bkt-dnf.md)                | 🔴 High   | Not Started |
| 3   | [Toolbox Commands](#3-toolbox-commands)                 | [RFC-0003](docs/rfcs/0003-bkt-dev.md)                | 🔴 High   | Not Started |
| 4   | [Privileged Helper](#4-privileged-helper)               | [RFC-0004](docs/rfcs/0004-bkt-admin.md)              | 🟡 Medium | Not Started |
| 5   | [Changelog Management](#5-changelog-management)         | [RFC-0005](docs/rfcs/0005-changelog.md)              | 🟡 Medium | Not Started |
| 6   | [Upstream Management](#6-upstream-management)           | [RFC-0006](docs/rfcs/0006-upstream-management.md)    | 🟡 Medium | Not Started |
| 7   | [Base Image Drift Detection](#7-drift-detection)        | [RFC-0007](docs/rfcs/0007-drift-detection.md)        | 🟢 Low    | Not Started |
| 8   | [Validation on Add](#8-validation-on-add)               | —                                                    | 🟢 Low    | Not Started |

---

## 1. Command Punning Foundation

**RFC:** [0001-command-punning.md](docs/rfcs/0001-command-punning.md)  
**Priority:** 🔴 High  
**Status:** Not Started

### Description

Establish the core infrastructure for command punning: the pattern where `bkt` commands execute immediately AND propagate changes to the distribution.

### Deliverables

- [ ] Refactor `bkt` CLI to support execution contexts (host/dev/image)
- [ ] Implement `--pr` / `--local` / `--pr-only` flags consistently across all commands
- [ ] Add context detection (in-toolbox vs host)
- [ ] Standardize manifest update + PR creation pipeline
- [ ] Document the punning philosophy in README

### Acceptance Criteria

- Running `bkt flatpak add org.gnome.Boxes` installs immediately AND opens a PR
- Running `bkt flatpak add org.gnome.Boxes --local` installs without PR
- Running `bkt flatpak add org.gnome.Boxes --pr-only` only opens PR (no local install)

---

## 2. RPM Package Management

**RFC:** [0002-bkt-dnf.md](docs/rfcs/0002-bkt-dnf.md)  
**Priority:** 🔴 High  
**Status:** Not Started

### Description

Implement `bkt dnf` as a punned command layer for RPM packages on atomic systems.

### Deliverables

- [ ] Implement query pass-through (`bkt dnf search`, `info`, `provides`, `list`)
- [ ] Create `manifests/system-packages.json` schema and manifest
- [ ] Implement `bkt dnf install` (rpm-ostree + manifest + Containerfile PR)
- [ ] Implement `bkt dnf remove`
- [ ] Add Containerfile section markers for managed packages
- [ ] Implement package validation (check if package exists before adding)

### Acceptance Criteria

- `bkt dnf search htop` returns results from dnf5
- `bkt dnf install htop` runs `rpm-ostree install htop` AND updates manifest AND opens PR
- Containerfile `dnf install` block is auto-regenerated from manifest

---

## 3. Toolbox Commands

**RFC:** [0003-bkt-dev.md](docs/rfcs/0003-bkt-dev.md)  
**Priority:** 🔴 High  
**Status:** Not Started

### Description

Implement `bkt dev` prefix for commands that target the development toolbox.

### Deliverables

- [ ] Create `manifests/toolbox-packages.json` schema and manifest
- [ ] Implement `bkt dev dnf install/remove/list`
- [ ] Implement toolbox detection (running in toolbox vs host)
- [ ] Implement `bkt dev enter` shortcut
- [ ] Implement `bkt dev update` (sync toolbox to manifest)
- [ ] Add validation for invalid combinations (`bkt dev flatpak` → error)

### Acceptance Criteria

- `bkt dev dnf install gcc` installs gcc in toolbox immediately
- `manifests/toolbox-packages.json` is updated with package entry
- Running `bkt dev flatpak add ...` produces helpful error

---

## 4. Privileged Helper

**RFC:** [0004-bkt-admin.md](docs/rfcs/0004-bkt-admin.md)  
**Priority:** 🟡 Medium  
**Status:** Not Started

### Description

Create `bkt-admin`, a setuid helper for passwordless privileged operations.

### Deliverables

- [ ] Implement `bkt-admin` binary in Rust
- [ ] Implement bootc operations: `status`, `upgrade`, `rollback`, `switch`
- [ ] Implement systemctl operations with service allowlist
- [ ] Create `/usr/share/bootc/allowed-services.txt`
- [ ] Update Containerfile to install `bkt-admin` with setuid
- [ ] Integrate with `bkt` CLI (auto-use helper when available)

### Acceptance Criteria

- `bkt status` works without password from toolbox
- `bkt upgrade` works with `--confirm` flag
- Attempting to manage unlisted services fails with clear error

---

## 5. Changelog Management

**RFC:** [0005-changelog.md](docs/rfcs/0005-changelog.md)  
**Priority:** 🟡 Medium  
**Status:** Not Started

### Description

Implement structured changelog with auto-generation and CI enforcement.

### Deliverables

- [ ] Create `changelog.json` schema
- [ ] Implement `bkt changelog generate` (diff manifests → draft entries)
- [ ] Implement `bkt changelog validate`
- [ ] Implement `bkt changelog show`
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
**Status:** Not Started

### Description

Consolidate scattered version pins into unified upstream manifest with semver policies.

### Deliverables

- [ ] Create `upstream/manifest.json` schema
- [ ] Migrate existing pins (starship, lazygit, keyd, bibata, whitesur, getnf)
- [ ] Implement `bkt upstream list`
- [ ] Implement `bkt upstream check` (show available updates)
- [ ] Implement `bkt upstream update` (update within policy)
- [ ] Implement `bkt upstream lock` (regenerate checksums)
- [ ] Implement `bkt upstream verify` (verify all checksums)
- [ ] Update Containerfile to read from manifest
- [ ] Generate changelog entries for updates

### Acceptance Criteria

- All current `upstream/*.version` files replaced by single manifest
- `bkt upstream check` shows available updates with policy indicators
- `bkt upstream update` respects semver policies

---

## 7. Base Image Drift Detection

**RFC:** [0007-drift-detection.md](docs/rfcs/0007-drift-detection.md)  
**Priority:** 🟢 Low  
**Status:** Not Started

### Description

Explicitly declare and verify assumptions about the base image.

### Deliverables

- [ ] Create `manifests/base-image-assumptions.json` schema
- [ ] Document initial assumptions (adw-gtk3-theme, gnome-shell, flatpak, etc.)
- [ ] Implement `bkt base verify`
- [ ] Implement `bkt base assume <package>`
- [ ] Add CI workflow to verify assumptions
- [ ] Add scheduled check against `:stable` and `:latest`
- [ ] Integrate with changelog when assumptions change

### Acceptance Criteria

- CI fails if base image lacks assumed packages
- `bkt base verify` runs locally and shows clear pass/fail
- Scheduled job detects upcoming breaking changes

---

## 8. Validation on Add

**Priority:** 🟢 Low  
**Status:** Not Started

### Description

Validate items before adding to manifests to prevent typos and invalid entries.

### Deliverables

- [ ] Flatpak: Query remote to verify app exists (`flatpak search`)
- [ ] Extension: Check extensions.gnome.org API for UUID validity
- [ ] GSettings: Verify schema exists (`gsettings list-schemas`)
- [ ] DNF: Verify package exists before adding to manifest

### Acceptance Criteria

- `bkt flatpak add org.gnome.Nonexistent` fails with helpful suggestion
- `bkt gsetting set nonexistent.schema key value` fails with schema list
- All add commands validate before modifying manifests

---

## Implementation Order

Recommended order based on dependencies:

```
Phase 2a: Core Infrastructure
├── 1. Command Punning Foundation (required by all)
├── 4. Privileged Helper (independent, enables better UX)
└── 6. Upstream Management (independent, addresses PR feedback)

Phase 2b: Package Management  
├── 2. RPM Package Management (depends on #1)
└── 3. Toolbox Commands (depends on #1)

Phase 2c: Lifecycle Management
├── 5. Changelog Management (depends on #4)
└── 7. Drift Detection (depends on #5)

Phase 2d: Polish
└── 8. Validation on Add (can be done incrementally)
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

| RFC | Title | Status |
|-----|-------|--------|
| [RFC-0001](docs/rfcs/0001-command-punning.md) | Command Punning Philosophy | Draft |
| [RFC-0002](docs/rfcs/0002-bkt-dnf.md) | `bkt dnf` RPM Package Management | Draft |
| [RFC-0003](docs/rfcs/0003-bkt-dev.md) | `bkt dev` Toolbox Commands | Draft |
| [RFC-0004](docs/rfcs/0004-bkt-admin.md) | `bkt-admin` Privileged Helper | Draft |
| [RFC-0005](docs/rfcs/0005-changelog.md) | Changelog Management | Draft |
| [RFC-0006](docs/rfcs/0006-upstream-management.md) | Upstream Dependency Management | Draft |
| [RFC-0007](docs/rfcs/0007-drift-detection.md) | Base Image Drift Detection | Draft |
