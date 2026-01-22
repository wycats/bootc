# Current Work: Phase 4 — Manifest Fidelity & Workflow Gaps

This document tracks Phase 4 of the bootc distribution development. Phase 3 is archived at [docs/history/003-phase3-complete-the-loop.md](docs/history/003-phase3-complete-the-loop.md).

Quality backlog: [QUALITY.md](QUALITY.md)
Distrobox integration RFC: [docs/rfcs/0017-distrobox-integration.md](docs/rfcs/0017-distrobox-integration.md)

---

## Vision

**Phase 3** closed the manifest→Containerfile loop: auto-generation, post-reboot automation, drift visibility, and ephemeral tracking all work.

**Phase 4** addresses the gaps between what users expect and what `bkt` actually captures:

1. **Manifest Fidelity**: Capture the _full_ configuration state, not just "is it installed?"
2. **Command Punning Completion**: `bkt dnf install` should work for host packages, not just toolbox
3. **Development Environment**: `bkt dev` commands should actually execute, not just update manifests
4. **Upstream Dependencies**: Track themes, icons, fonts with pinned versions

The guiding principle: **If you configured it, `bkt` should capture it.**

### The Fidelity Gap

```
What the system knows          What bkt captures
─────────────────────          ─────────────────
Extension: installed ✓         ✅ Captured
Extension: enabled/disabled    ❌ Not captured ← Gap
Flatpak: installed ✓           ✅ Captured
Flatpak: permissions (Flatseal)❌ Not captured ← Gap
GSettings: current values      ⚠️ Manual schema only
Themes/Icons/Fonts             ❌ Not tracked  ← Gap
```

Phase 4 closes these fidelity gaps.

---

## Overview

| ID  | Item                                                            | Size | Deps | Status      |
| --- | --------------------------------------------------------------- | ---- | ---- | ----------- |
| 1   | [Extension Enabled State](#1-extension-enabled-state)           | M    | —    | Not Started |
| 2   | [Flatpak Override Capture](#2-flatpak-override-capture)         | M    | —    | Not Started |
| 3   | [Host Package Install](#3-host-package-install)                 | L    | —    | Not Started |
| 4   | [Dev Command Execution](#4-dev-command-execution)               | M    | —    | Not Started |
| 5   | [GSettings Auto-Discovery](#5-gsettings-auto-discovery)         | M    | —    | Not Started |
| 6   | [Upstream Dependency Tracking](#6-upstream-dependency-tracking) | XL   | —    | Not Started |
| 7   | [Drift Resolution](#7-drift-resolution)                         | M    | 1, 2 | Not Started |
| 8   | [Dev Toolchain Management](#8-dev-toolchain-management)         | L    | 4    | Not Started |

**Size Legend:** S = Small, M = Medium, L = Large, XL = Extra Large

---

## 1. Extension Enabled State

**Source:** Gap Analysis  
**Size:** M  
**Dependencies:** None  
**Status:** Not Started

### Problem

Extensions track installation but not enabled/disabled state. When you disable an extension via Extension Manager, `bkt capture` doesn't notice. On next `bkt apply`, disabled extensions get re-enabled.

### Current Behavior

```bash
# User disables extension via Extension Manager
gnome-extensions disable blur-my-shell@aunetx

# bkt capture sees it's still installed, does nothing
bkt extension capture  # No change detected

# bkt apply re-enables it (not what user wanted)
bkt apply
```

### Solution

1. Schema already supports `enabled` field via `ExtensionConfig` object
2. Update capture to query `gnome-extensions list --enabled`
3. Store extensions as objects with `enabled: true/false`
4. Update apply to respect enabled state

### Deliverables

- [ ] Update `bkt extension capture` to detect enabled/disabled state
- [ ] Store captured extensions as `ExtensionConfig` objects (not just UUID strings)
- [ ] Update `bkt extension sync` to enable/disable based on manifest
- [ ] Update `bkt extension enable/disable` to update manifest enabled state
- [ ] Add tests for enabled state round-trip

### Acceptance Criteria

- Disabling extension via Extension Manager → captured as `enabled: false`
- `bkt apply` respects enabled state (doesn't re-enable disabled extensions)
- `bkt extension disable blur-my-shell` updates manifest AND disables on system

---

## 2. Flatpak Override Capture

**Source:** Gap Analysis  
**Size:** M  
**Dependencies:** None  
**Status:** Not Started

### Problem

Flatseal changes (filesystem access, device permissions, environment variables) are stored in `~/.local/share/flatpak/overrides/` but not captured by `bkt`. These changes are lost on image rebuild.

### Current Behavior

```bash
# User grants Discord microphone access via Flatseal
# Creates ~/.local/share/flatpak/overrides/com.discordapp.Discord

# bkt capture sees Discord is installed, but ignores overrides
bkt flatpak capture  # Overrides not captured

# After image rebuild, Flatseal changes are gone
```

### Solution

1. Schema already supports `overrides` field in `FlatpakApp`
2. Read override files from `~/.local/share/flatpak/overrides/`
3. Parse and store in manifest
4. Apply overrides during `bkt flatpak sync`

### Deliverables

- [ ] Add `parse_flatpak_overrides(app_id: &str) -> Option<FlatpakOverrides>`
- [ ] Update `bkt flatpak capture` to include overrides
- [ ] Update `bkt flatpak sync` to write override files
- [ ] Add `bkt flatpak override show <app>` command
- [ ] Add tests for override round-trip

### Acceptance Criteria

- Flatseal permission changes → captured in manifest overrides field
- `bkt apply` restores Flatseal permissions
- Override changes create proper PR diff

---

## 3. Host Package Install

**Source:** RFC-0002, Gap Analysis  
**Size:** L  
**Dependencies:** None  
**Status:** Not Started

### Problem

`bkt dnf install htop` only works in toolbox context. For host packages, users must manually edit the Containerfile, violating the command punning promise.

### Current Behavior

```bash
# In toolbox: works
bkt dnf install htop  # Updates toolbox manifest

# On host: doesn't work as expected
bkt dnf install htop  # Updates system-packages.json but NOT Containerfile
                      # User must manually edit Containerfile
```

### Solution

When running on host (not toolbox):

1. Update `manifests/system-packages.json`
2. Regenerate Containerfile SYSTEM_PACKAGES section
3. Create PR with both changes

### Deliverables

- [ ] Detect host vs toolbox context in `bkt dnf install`
- [ ] For host: update system-packages.json AND Containerfile
- [ ] Integrate with PR workflow (both files in same commit)
- [ ] Add `--containerfile-only` flag to skip manifest (for manual use)
- [ ] Update help text to clarify host vs toolbox behavior

### Acceptance Criteria

- `bkt dnf install htop` (on host) updates manifest + Containerfile + creates PR
- `bkt dnf remove htop` (on host) removes from both
- PR contains atomic commit with both changes

---

## 4. Dev Command Execution

**Source:** RFC-0003, Gap Analysis  
**Size:** M  
**Dependencies:** None  
**Status:** Not Started

### Problem

`bkt dev dnf install gcc` updates `toolbox-packages.json` but doesn't actually execute the install in the toolbox. User must manually run the install.

### Current Behavior

```bash
bkt dev dnf install gcc
# Output: Added gcc to toolbox-packages.json
# But gcc is NOT installed in toolbox!
# User must also run: dnf install gcc
```

### Solution

Execute the package installation in addition to updating the manifest:

1. Run `dnf install` in current toolbox
2. Update `toolbox-packages.json`
3. Optionally create PR

### Deliverables

- [ ] Execute `dnf install` when `bkt dev dnf install` is run
- [ ] Handle install failures gracefully (rollback manifest change?)
- [ ] Add `--manifest-only` flag to skip execution
- [ ] Add `--no-pr` flag to skip PR creation
- [ ] Update error messages for failed installs

### Acceptance Criteria

- `bkt dev dnf install gcc` installs gcc AND updates manifest
- Failed install doesn't corrupt manifest
- `--manifest-only` skips execution (current behavior, for scripting)

---

## 5. GSettings Auto-Discovery

**Source:** Gap Analysis  
**Size:** M  
**Dependencies:** None  
**Status:** Not Started

### Problem

`bkt gsetting capture` requires knowing the exact schema and key. Users can't discover which settings have drifted from defaults.

### Current Behavior

```bash
# User changes font size in Settings app
# Which schema was that? User doesn't know.

# Must guess the schema:
bkt gsetting capture org.gnome.desktop.interface text-scaling-factor

# No way to find all changed settings
```

### Solution

1. Dump current dconf state
2. Compare against baseline (GNOME defaults or saved baseline)
3. Show changed schemas/keys
4. Allow selective capture

### Deliverables

- [ ] Add `bkt gsetting diff` command to show changed settings
- [ ] Create baseline snapshot on first run (`~/.local/share/bkt/gsettings-baseline.txt`)
- [ ] Add `bkt gsetting capture --all-changed` to capture all drifted settings
- [ ] Filter out transient/unimportant schemas (window positions, recent files, etc.)

### Acceptance Criteria

- `bkt gsetting diff` shows settings that differ from baseline
- `bkt gsetting capture --all-changed` captures all meaningful changes
- Baseline can be reset with `bkt gsetting baseline reset`

---

## 6. Upstream Dependency Tracking

**Source:** RFC-0006, Gap Analysis  
**Size:** XL  
**Dependencies:** None  
**Status:** Not Started

### Problem

Themes, icons, fonts, and external tools are not tracked or versioned. Users manually download these, and they're lost on image rebuild.

### Current State

- `upstream/manifest.json` exists but is unused
- `bkt upstream` command exists as stub
- No implementation

### Solution

Full implementation of RFC-0006:

1. `bkt upstream add github:vinceliuice/Colloid-gtk-theme`
2. Pin to specific release/commit
3. SHA256 verification
4. Containerfile generation for downloads
5. Update checking

### Deliverables

- [ ] Implement `bkt upstream add <source>` with GitHub support
- [ ] Implement version pinning (release tag or commit SHA)
- [ ] Implement SHA256 verification
- [ ] Generate Containerfile UPSTREAM section with curl/extract commands
- [ ] Implement `bkt upstream check` for available updates
- [ ] Implement `bkt upstream update <name>` to bump versions
- [ ] Add tests for GitHub API integration

### Acceptance Criteria

- `bkt upstream add github:vinceliuice/Colloid-gtk-theme` pins and downloads
- Theme installed in image at build time
- `bkt upstream check` shows available updates
- SHA256 verification prevents tampered downloads

---

## 7. Drift Resolution

**Source:** RFC-0007, Gap Analysis  
**Size:** M  
**Dependencies:** 1 (Extension Enabled State), 2 (Flatpak Override Capture)  
**Status:** Not Started

### Problem

`bkt drift check` exists (via Python script) but there's no `bkt drift resolve` for interactive resolution. Users must manually decide what to capture vs apply.

### Solution

1. Rewrite drift detection in Rust (using existing manifest types)
2. Add `bkt drift resolve` with interactive prompts
3. For each drift item: capture to manifest, apply from manifest, or skip

### Deliverables

- [ ] Rewrite drift detection in Rust (replace Python script)
- [ ] Implement `bkt drift resolve` with interactive mode
- [ ] Show clear diff for each item (system state vs manifest)
- [ ] Support batch operations (capture all, apply all)
- [ ] Add `--dry-run` flag

### Acceptance Criteria

- `bkt drift resolve` walks through each drifted item
- User can choose: capture, apply, skip for each
- Batch mode: `bkt drift resolve --capture-all`

---

## 8. Dev Toolchain Management

**Source:** RFC-0003, Gap Analysis  
**Size:** L  
**Dependencies:** 4 (Dev Command Execution)  
**Status:** Not Started

### Problem

`bkt dev rustup` and `bkt dev npm` don't exist. Rust toolchains and global npm packages aren't managed declaratively.

### Solution

Extend `bkt dev` with toolchain-specific subcommands:

1. `bkt dev rustup default stable` - Sets default toolchain
2. `bkt dev npm install -g typescript` - Installs global npm package
3. Update toolbox Containerfile accordingly

### Deliverables

- [ ] Implement `bkt dev rustup` subcommand
- [ ] Implement `bkt dev npm` subcommand
- [ ] Store toolchain config in `toolbox-packages.json` or new manifest
- [ ] Generate toolbox Containerfile with toolchain setup
- [ ] Add `bkt dev script add <url>` with SHA256 verification (curl-pipe scripts)

### Acceptance Criteria

- `bkt dev rustup default stable` installs stable Rust and updates manifest
- `bkt dev npm install -g typescript` installs and tracks in manifest
- Toolbox Containerfile includes toolchain setup commands

---

## Dependency Graph

```
┌──────────────────┐
│ 1. Extension     │
│    Enabled State │──────┐
└──────────────────┘      │
                          ▼
┌──────────────────┐   ┌──────────────────┐
│ 2. Flatpak       │──▶│ 7. Drift         │
│    Overrides     │   │    Resolution    │
└──────────────────┘   └──────────────────┘

┌──────────────────┐   ┌──────────────────┐
│ 4. Dev Command   │──▶│ 8. Dev Toolchain │
│    Execution     │   │    Management    │
└──────────────────┘   └──────────────────┘

Independent:
┌──────────────────┐   ┌──────────────────┐   ┌──────────────────┐
│ 3. Host Package  │   │ 5. GSettings     │   │ 6. Upstream      │
│    Install       │   │    Discovery     │   │    Dependencies  │
└──────────────────┘   └──────────────────┘   └──────────────────┘
```

---

## Suggested Implementation Order

### Wave 1: Daily Pain Points (No Dependencies)

Start with items that fix daily workflow friction:

1. **Item 1: Extension Enabled State** (M) — Fixes re-enabling disabled extensions
2. **Item 2: Flatpak Override Capture** (M) — Fixes losing Flatseal changes
3. **Item 3: Host Package Install** (L) — Fixes manual Containerfile editing

### Wave 2: Development Workflow

4. **Item 4: Dev Command Execution** (M) — Fixes `bkt dev dnf` not executing
5. **Item 5: GSettings Auto-Discovery** (M) — Reduces guesswork for settings

### Wave 3: Dependent Features

6. **Item 7: Drift Resolution** (M) — Requires 1, 2 for full fidelity
7. **Item 8: Dev Toolchain Management** (L) — Requires 4 for execution pattern

### Wave 4: Large Feature

8. **Item 6: Upstream Dependency Tracking** (XL) — Independent but large scope

---

## Immediate Follow-ups: Workflow Visibility

These items emerged from investigating why upstream Bazzite updates weren't being detected properly.

### Completed Fixes

- ✅ **BASE_IMAGE mismatch**: Fixed workflow checking wrong image (`bazzite:stable` vs `bazzite-gnome:stable`)
- ✅ **Race condition**: Workflow now pins Containerfile to the exact detected digest

### Near-term Follow-ups

| Item | Description | Reference |
|------|-------------|-----------|
| **Build-info base image section** | `bkt build-info` should show base image package diffs when upstream changes. Currently queries OCI labels; needs to compare current vs previous image labels. | [RFC-0013](docs/rfcs/0013-build-descriptions.md) |
| **`bkt status` visibility** | `bkt status` should surface: last build date, current vs latest upstream digest, pending changes. Prevents "silent failure" where update loop breaks unnoticed. | — |
| **Pinned tool update checking** | Scheduled workflow to run `bkt upstream check` and create PRs for available updates (starship, lazygit, keyd, etc.) | [RFC-0006](docs/rfcs/0006-upstream-management.md) |
| **Release changelog completeness** | GitHub Releases should include full upstream change info when base image updated | [RFC-0013](docs/rfcs/0013-build-descriptions.md) |

### Design Decision: Digest Tracking via OCI Labels

The workflow stores `org.wycats.bootc.base.digest` as an OCI label on each published image. This is the source of truth for "what Bazzite digest did we build against?"

- `upstream/bazzite-stable.digest` file is **documentation only** (not used by workflow)
- `bkt build-info` should query OCI labels from current and previous images to detect base changes
- No git commits needed for digest tracking (avoids permissions complexity)

---

## Deferred to Phase 5+

- Multi-machine sync
- Interactive TUI mode
- `bkt init` command for new distributions
- Plugin system
- Remote management
- Automatic changelog generation (RFC-0005)
- Monitoring via systemd timer

---

## Questions to Resolve

1. **Override format**: Should we use Flatpak's native override format or normalize to JSON?
2. **GSettings baseline**: Ship a baseline with the image, or create on first run?
3. **Toolchain manifests**: Separate `rustup.json`/`npm.json` or extend `toolbox-packages.json`?
4. **Upstream verification**: SHA256 of archive or individual files?
