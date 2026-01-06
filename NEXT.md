# Next Phase: Phase 3 — Complete the Loop

This document sketches Phase 3 of the bootc distribution development. Phase 2 is tracked in [CURRENT.md](CURRENT.md).

---

## Vision

**Phase 2** established the bidirectional sync infrastructure: `bkt apply` and `bkt capture` work, command punning is implemented, and the Plan/Execute architecture provides a clean foundation.

**Phase 3** closes the remaining gaps between intention and automation:

1. **Containerfile Auto-Generation**: When you run `bkt dnf install htop`, the Containerfile updates automatically
2. **Post-Reboot Automation**: Manifest changes apply without manual intervention after image deployment
3. **Drift Visibility**: ✅ `bkt status` shows exactly what's out of sync (completed in Phase 2)
4. **Ephemeral Tracking**: `--local` changes are tracked and promotable to PRs

The guiding principle: **Install things however you want, and `bkt` keeps the distribution in sync.**

### The Complete Loop

```
┌─────────────────┐                    ┌─────────────────┐
│   MANIFESTS     │ ──── bkt apply ──→ │     SYSTEM      │
│  (git-tracked)  │ ←── bkt capture ── │  (live state)   │
└────────┬────────┘                    └─────────────────┘
         │
         ▼ auto-generate
┌─────────────────┐
│  CONTAINERFILE  │ ──── podman build ──→ New Image
└─────────────────┘
```

Phase 2 established the top half. Phase 3 closes the manifest→Containerfile loop.

---

## Overview

| ID  | Item                                                    | Source    | Priority  | Status      |
| --- | ------------------------------------------------------- | --------- | --------- | ----------- |
| 1   | [Containerfile Auto-Generation](#1-containerfile-auto)  | RFC-0002  | 🔴 High   | Not Started |
| 2   | [Post-Reboot Automation](#2-post-reboot)                | Workflow  | 🔴 High   | Not Started |
| 3   | [Drift Visibility in Status](#3-drift-visibility)       | Workflow  | ✅ Done   | Completed   |
| 4   | [Ephemeral Manifest](#4-ephemeral-manifest)             | RFC-0001  | 🟡 Medium | Not Started |
| 5   | [Image-Time Configuration](#5-image-time-config)        | RFC-0004  | 🟡 Medium | Not Started |
| 6   | [RFC Audit & Cleanup](#6-rfc-audit)                     | Housekeep | ✅ Done   | Completed   |

---

## 1. Containerfile Auto-Generation

**Source:** RFC-0002 (incomplete deliverable)  
**Priority:** 🔴 High  
**Status:** Not Started

### Problem

`bkt dnf install htop` updates `manifests/system-packages.json` but does NOT update the Containerfile. Users must manually sync these, breaking the command punning promise.

### Solution

Add section markers to Containerfile and auto-regenerate managed sections:

```dockerfile
# === SYSTEM PACKAGES (managed by bkt) ===
RUN dnf install -y \
    htop \
    neovim
# === END SYSTEM PACKAGES ===
```

### Design

#### Module Structure

```
bkt/src/
├── containerfile.rs          # New module
│   ├── Section              # Enum: SystemPackages, CoprRepos, etc.
│   ├── ManagedBlock         # Start marker, content, end marker
│   ├── ContainerfileEditor  # Parse, update, write
│   └── generate_*()         # Per-section generators
```

#### Section Markers

```dockerfile
# === SECTION_NAME (managed by bkt) ===
# Content here is auto-generated
# Manual edits will be overwritten
# === END SECTION_NAME ===
```

Supported sections:
- `COPR REPOSITORIES` - `dnf copr enable` commands
- `SYSTEM PACKAGES` - `dnf install` with sorted package list
- `HOST SHIMS` - `COPY` and symlink commands for shims

#### API

```rust
pub struct ContainerfileEditor {
    path: PathBuf,
    sections: Vec<(Section, ManagedBlock)>,
    unmanaged: Vec<String>,  // Lines outside managed sections
}

impl ContainerfileEditor {
    pub fn load(path: &Path) -> Result<Self>;
    pub fn update_section(&mut self, section: Section, content: &str);
    pub fn write(&self) -> Result<()>;
}

// Generator functions
pub fn generate_system_packages(manifest: &SystemPackagesManifest) -> String;
pub fn generate_copr_repos(manifest: &CoprManifest) -> String;
```

#### Integration Points

1. **`bkt dnf install/remove`** - After updating manifest, regenerate SYSTEM PACKAGES section
2. **`bkt dnf copr enable/disable`** - Regenerate COPR REPOSITORIES section
3. **`bkt containerfile sync`** - Manual full regeneration command
4. **`bkt containerfile check`** - Dry-run showing what would change

### Deliverables

- [ ] Create `bkt/src/containerfile.rs` module
- [ ] Implement section marker parsing
- [ ] Implement `generate_system_packages()`
- [ ] Implement `generate_copr_repos()`
- [ ] Hook into `bkt dnf install/remove` commands
- [ ] Hook into `bkt dnf copr enable/disable` commands
- [ ] Add `bkt containerfile sync` command
- [ ] Add `bkt containerfile check` command
- [ ] Preserve manual content outside managed sections
- [ ] Add tests for Containerfile parsing and generation

### Acceptance Criteria

- `bkt dnf install htop` updates both manifest AND Containerfile
- `bkt dnf copr enable atim/starship` updates both manifest AND Containerfile
- Manual Containerfile edits outside markers are preserved
- `bkt containerfile check` shows drift without modifying

---

## 2. Post-Reboot Automation

**Source:** Workflow gap  
**Priority:** 🔴 High  
**Status:** Not Started

### Problem

After rebooting into a new image, users must manually run `bkt apply` and `bkt dnf sync`. This is easy to forget.

### Current State

- `bootc-bootstrap.service` runs on first login (user-level)
- Handles flatpaks, extensions, gsettings, shims
- Does NOT handle system-level package sync

### Solution

Add system-level service that runs after image deployment:

```ini
# systemd/system/bootc-apply.service
[Unit]
Description=Apply bkt manifests after image deployment
After=local-fs.target network-online.target
ConditionPathExists=/usr/bin/bkt

[Service]
Type=oneshot
ExecStart=/usr/bin/bkt dnf sync --now
RemainAfterExit=yes

[Install]
WantedBy=multi-user.target
```

### Deliverables

- [ ] Create `systemd/system/bootc-apply.service`
- [ ] Add to Containerfile installation
- [ ] Consider: detect if running on new image deployment
- [ ] Consider: `--now` flag for `bkt dnf sync` to avoid double-reboot

### Acceptance Criteria

- After reboot with new image, packages sync automatically
- No manual `bkt dnf sync` required

---

## 3. Drift Visibility in Status

**Status:** ✅ Completed

Implemented in PR #28. `bkt status` now shows:

```
⚠️ Drift Detected
    3 flatpaks installed but not in manifest
    1 extension enabled but not in manifest

    Run bkt capture to import these changes.
```

---

## 4. Ephemeral Manifest

**Source:** RFC-0001  
**Priority:** 🟡 Medium  
**Status:** Not Started

### Problem

`--local` flag exists but changes aren't tracked. Users can't see what they've done locally or promote those changes to PRs.

### Solution

Implement ephemeral manifest from RFC-0001:

```
~/.local/share/bkt/ephemeral.json
```

Track all `--local` changes with boot_id validation.

### Deliverables

- [ ] Create `bkt/src/manifest/ephemeral.rs`
- [ ] Implement `EphemeralManifest` struct with boot_id tracking
- [ ] Record `--local` changes to ephemeral.json
- [ ] Implement `bkt local list` command
- [ ] Implement `bkt local commit` command (creates PR from accumulated changes)
- [ ] Implement `bkt local clear` command
- [ ] Clear ephemeral manifest on reboot (boot_id mismatch)

### Acceptance Criteria

- `bkt flatpak add --local org.gnome.Boxes` records to ephemeral.json
- `bkt local list` shows local-only changes
- `bkt local commit` creates PR with accumulated changes
- Reboot invalidates ephemeral manifest

---

## 5. Image-Time Configuration

**Source:** RFC-0004  
**Priority:** 🟡 Medium  
**Status:** Not Started

### Problem

Some configuration can only be applied at image build time:
- Kernel arguments (`kargs`)
- Systemd unit enable/disable
- Custom systemd units

### Current State

RFC-0004 exists but is marked "Future".

### Solution

Implement `bkt admin` commands for image-time config:

- `bkt admin kargs add/remove`
- `bkt admin systemd enable/disable` (for custom units, not runtime control)

### Deliverables

- [ ] Review and update RFC-0004
- [ ] Implement `bkt admin kargs` commands
- [ ] Implement systemd unit management for image-time
- [ ] Create manifest for managed units
- [ ] Hook into Containerfile generation

### Acceptance Criteria

- `bkt admin kargs add rd.driver.blacklist=nouveau` updates manifest and Containerfile
- `bkt admin systemd enable my-service.service` adds unit and enables in Containerfile

---

## 6. RFC Audit & Cleanup

**Status:** ✅ Completed

Completed in PR #25. All RFC statuses now reflect implementation reality:
- RFC-0001 through RFC-0003: Implemented
- RFC-0005 through RFC-0009: Implemented
- RFC-0010: Implemented (Transparent Delegation)
- RFC-0011: Implemented (Testing Strategy)

---

## Implementation Order

```
Phase 3a: Close the Loop (Weeks 1-2)
├── 1. Containerfile Auto-Generation 🔴 HIGH IMPACT
└── Review user experience so far

Phase 3b: Automation (Week 3)
├── 2. Post-Reboot Automation
└── Test full workflow end-to-end

Phase 3c: Polish (Weeks 4+)
├── 4. Ephemeral Manifest
└── 5. Image-Time Configuration (if time permits)
```

---

## User Workflow After Phase 3

### Daily Usage (The Dream)

1. **Install something**: Use GNOME Software, `dnf install`, Extension Manager — whatever
2. **See drift**: `bkt status` shows "3 items not in manifest"
3. **Capture**: `bkt capture` imports everything
4. **Auto-update Containerfile**: Manifests automatically sync to Containerfile
5. **Commit**: Changes create a PR to your distribution
6. **Rebuild**: CI builds new image
7. **Reboot**: `bkt admin bootc upgrade` or automatic
8. **Done**: Everything syncs automatically

### Key Difference from Phase 2

- No manual Containerfile editing
- No forgetting to capture (status reminds you)
- No forgetting to apply after reboot (automated)

---

## Questions to Resolve

1. **Containerfile location**: Should we support custom Containerfile paths?
2. **Section marker format**: Exact syntax for managed sections?
3. **COPR in Containerfile**: Separate section or inline with packages?
4. **Boot detection**: How to detect "first boot on new image" reliably?
5. **Ephemeral scope**: Should ephemeral manifest survive image updates?

---

## Deferred to Phase 4+

- Multi-machine sync
- Interactive TUI mode
- `bkt init` command
- Plugin system
- Remote management
- Semver update policies for upstream
