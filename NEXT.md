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
| 1   | [Containerfile Auto-Generation](#1-containerfile-auto)  | RFC-0002  | 🔴 High   | In Progress |
| 2   | [Post-Reboot Automation](#2-post-reboot)                | Workflow  | 🔴 High   | Not Started |
| 3   | [Drift Visibility in Status](#3-drift-visibility)       | Workflow  | ✅ Done   | Completed   |
| 4   | [Ephemeral Manifest](#4-ephemeral-manifest)             | RFC-0001  | 🟡 Medium | Not Started |
| 5   | [Image-Time Configuration](#5-image-time-config)        | RFC-0004  | 🟡 Medium | Not Started |
| 6   | [RFC Audit & Cleanup](#6-rfc-audit)                     | Housekeep | ✅ Done   | Completed   |

---

## 1. Containerfile Auto-Generation

**Source:** RFC-0002 (incomplete deliverable)  
**Priority:** 🔴 High  
**Status:** In Progress

### Problem

`bkt dnf install htop` updates `manifests/system-packages.json` but does NOT update the Containerfile. Users must manually sync these, breaking the command punning promise.

### Solution

Add section markers to Containerfile and auto-regenerate managed sections:

```dockerfile
# === SYSTEM_PACKAGES (managed by bkt) ===
RUN dnf install -y \
    htop \
    neovim \
    && dnf clean all
# === END SYSTEM_PACKAGES ===
```

### Implementation Progress

✅ **Module Structure** (`bkt/src/containerfile.rs` - 497 lines)

```rust
pub enum Section {
    SystemPackages,  // dnf install packages
    CoprRepos,       // dnf copr enable commands
    HostShims,       // COPY and symlink commands
}

pub struct ContainerfileEditor {
    pub fn load(path: &Path) -> Result<Self>;
    pub fn update_section(&mut self, section: Section, content: Vec<String>);
    pub fn has_section(&self, section: Section) -> bool;
    pub fn get_section_content(&self, section: Section) -> Option<&[String]>;
    pub fn write(&self) -> Result<()>;
    pub fn render(&self) -> String;
}

pub fn generate_system_packages(packages: &[String]) -> Vec<String>;
pub fn generate_copr_repos(repos: &[String]) -> Vec<String>;
```

✅ **Section Markers in Containerfile**

The Containerfile now has `# === SYSTEM_PACKAGES (managed by bkt) ===` markers around the dnf install block.

✅ **Integration with dnf commands**

The `sync_containerfile()` function in `dnf.rs` is called during PR creation for `bkt dnf install` and `bkt dnf remove`, syncing the Containerfile with manifest changes.

✅ **Tests** (11 tests)

- `test_section_markers` - Verifies marker format
- `test_parse_start_marker` - Validates marker parsing
- `test_parse_containerfile_with_sections` - Full parsing test
- `test_update_section` - Section replacement
- `test_generate_system_packages` - Package list generation
- `test_generate_system_packages_empty` - Empty package edge case
- `test_generate_system_packages_format` - Output format verification
- `test_generate_copr_repos` - COPR generation
- `test_generate_copr_repos_empty` - Empty repos edge case
- `test_render_preserves_unmanaged` - Unmanaged content preserved
- `test_parse_unclosed_section_error` - Error handling

### Remaining Deliverables

- [ ] Hook into `bkt dnf copr enable/disable` commands
- [ ] Add `bkt containerfile sync` command
- [ ] Add `bkt containerfile check` command
- [ ] Implement HOST_SHIMS section generation

### Acceptance Criteria

- ✅ Containerfile has managed section markers
- ✅ `bkt dnf install htop` updates both manifest AND Containerfile (via PR)
- ⏳ `bkt dnf copr enable atim/starship` updates both manifest AND Containerfile
- ✅ Manual Containerfile edits outside markers are preserved
- ⏳ `bkt containerfile check` shows drift without modifying

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

### Design Questions

**Q1: Boot Detection Strategy**

How do we detect "first boot on new image" vs. regular reboot?

Options:
1. **Deployment checksum comparison**: Check rpm-ostree deployment checksum vs last-recorded
2. **Marker file**: Create `/var/lib/bkt/last-applied-deployment` after apply
3. **Always run**: Apply is idempotent, so run on every boot (wastes time but simple)

**Recommendation**: Option 2 (marker file) - simple, reliable, and avoids unnecessary work.

**Q2: Double-Reboot Prevention**

If `bkt dnf sync` installs packages, it may require a reboot, but we're already post-reboot.

Options:
1. **`--now` flag**: Use `rpm-ostree install --apply-live` to avoid second reboot
2. **Skip if layered**: Only sync Containerfile packages, skip rpm-ostree changes
3. **Notify only**: Just notify user that packages need sync, don't auto-install

**Recommendation**: Option 1 with `--now` flag for seamless experience.

### Deliverables

- [ ] Create `systemd/system/bootc-apply.service`
- [ ] Add to Containerfile installation
- [ ] Implement deployment tracking marker file
- [ ] Add `--now` flag to `bkt dnf sync` for apply-live
- [ ] Test full reboot workflow

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

### Design Questions

**Q1: Data Structure**

What does `ephemeral.json` contain?

```json
{
  "boot_id": "abc123...",
  "created_at": "2026-01-06T12:00:00Z",
  "changes": [
    {
      "timestamp": "2026-01-06T12:05:00Z",
      "command": "bkt flatpak add --local org.gnome.Boxes",
      "subsystem": "flatpak",
      "action": "add",
      "item": "org.gnome.Boxes"
    }
  ]
}
```

**Q2: Aggregation Strategy**

Should ephemeral track each command separately, or aggregate into a single state?

- **Per-command**: Preserves history, can show exact commands to reproduce
- **Aggregated**: Simpler, mirrors manifest structure, easier to commit

**Recommendation**: Per-command with aggregation on commit.

**Q3: Conflict Handling**

What if ephemeral changes conflict with committed changes (e.g., remove package that manifest says to install)?

**Recommendation**: Ephemeral overrides during local session, `bkt local commit` requires resolution.

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

1. ~~**Containerfile location**: Should we support custom Containerfile paths?~~ — Using `Containerfile` in repo root
2. ~~**Section marker format**: Exact syntax for managed sections?~~ — Resolved: `# === SECTION_NAME (managed by bkt) ===`
3. **COPR in Containerfile**: Separate section or inline with packages? — Separate `COPR_REPOS` section
4. ~~**Boot detection**: How to detect "first boot on new image" reliably?~~ — Resolved: Marker file approach
5. **Ephemeral scope**: Should ephemeral manifest survive image updates?

---

## Immediate Next Steps (Prioritized)

Based on current implementation status and effort estimates:

### This Week (5-6 hours)

1. **Add COPR section to Containerfile** (~10 min)
   - Add `# === COPR_REPOS (managed by bkt) ===` markers before SYSTEM_PACKAGES

2. **Hook COPR commands into Containerfile sync** (~1 hour)
   - Add `sync_containerfile_copr()` in `dnf.rs`
   - Call from `handle_copr_enable` and `handle_copr_disable`

3. **Create `bkt containerfile` subcommand** (~2 hours)
   - `bkt containerfile sync` - regenerate all sections
   - `bkt containerfile check` - dry-run showing drift

4. **Merge PR #30** (when CI passes)

### Next Week (3-4 hours + design session)

1. **Design session**: Boot detection strategy (30 min)
2. **Create `systemd/system/bootc-apply.service`** (~1 hour)
3. **Add `--now` flag to `bkt dnf sync`** (~1 hour)
4. **Test full reboot workflow** (~1 hour)

### Later (8-10 hours + design session)

1. **Design session**: Ephemeral manifest data structure (1 hour)
2. **Implement ephemeral manifest tracking** (~4 hours)
3. **Implement `bkt local` commands** (~4 hours)

---

## Deferred to Phase 4+

- Multi-machine sync
- Interactive TUI mode
- `bkt init` command
- Plugin system
- Remote management
- Semver update policies for upstream
- Item 5: Image-Time Configuration (RFC-0004)
