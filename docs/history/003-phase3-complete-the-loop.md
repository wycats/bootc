# Phase 3 — Complete the Loop (Archived)

**Status:** ✅ Completed  
**Duration:** Phase 3 work  
**Archived:** January 2026

---

## Vision

Phase 3 closed the remaining gaps between intention and automation:

1. **Containerfile Auto-Generation**: When you run `bkt dnf install htop`, the Containerfile updates automatically
2. **Post-Reboot Automation**: Manifest changes apply without manual intervention after image deployment
3. **Drift Visibility**: `bkt status` shows exactly what's out of sync
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

---

## Completed Items

| ID  | Item                          | Source    | Status                |
| --- | ----------------------------- | --------- | --------------------- |
| 1   | Containerfile Auto-Generation | RFC-0002  | ✅ Completed (PR #31) |
| 2   | Post-Reboot Automation        | Workflow  | ✅ Completed          |
| 3   | Drift Visibility in Status    | Workflow  | ✅ Completed (PR #28) |
| 4   | Ephemeral Manifest            | RFC-0001  | ✅ Completed          |
| 5   | Image-Time Configuration      | RFC-0004  | ✅ Completed          |
| 6   | RFC Audit & Cleanup           | Housekeep | ✅ Completed (PR #25) |
| 7   | Changelog in Status           | RFC-0005  | ✅ Completed          |
| 8   | Topgrade Integration          | Feature   | ✅ Completed          |

---

## Key Deliverables

### 1. Containerfile Auto-Generation

- `bkt/src/containerfile.rs` - Managed section editor (497 lines)
- Section markers: `# === SECTION_NAME (managed by bkt) ===`
- Supports: SYSTEM_PACKAGES, COPR_REPOS, HOST_SHIMS
- `bkt containerfile sync` and `bkt containerfile check` commands

### 2. Post-Reboot Automation

- `systemd/system/bootc-apply.service` - Runs on first boot
- Marker file approach for deployment tracking
- `--now` flag for apply-live without second reboot

### 3. Drift Visibility

- `bkt status` shows drift detection
- Indicates flatpaks, extensions, packages not in manifest
- Suggests `bkt capture` when drift detected

### 4. Ephemeral Manifest

- `~/.local/share/bkt/ephemeral.json` tracks `--local` changes
- Boot ID validation (clears on reboot)
- `bkt local list`, `bkt local commit`, `bkt local clear` commands

### 5. Image-Time Configuration

- `bkt admin kargs` for kernel arguments
- `bkt admin systemctl` for systemd unit management
- D-Bus integration with polkit for privileged operations

### 6. Changelog in Status

- Pending changelog entries shown in `bkt status`
- Draft vs ready-to-release differentiation
- `bkt changelog release` suggested as next action

### 7. Topgrade Integration

- `/etc/topgrade.toml` injected into image
- Runs `bootc upgrade` and `bootc-bootstrap` during updates
- Drift check integrated into update flow

---

## Architecture Established

- **Plan/Execute Pattern**: All commands produce plans before execution
- **Transparent Delegation**: Toolbox ↔ Host communication via flatpak-spawn
- **Bidirectional Sync**: `bkt apply` (manifest→system) and `bkt capture` (system→manifest)
- **PR Workflow**: Changes create PRs via `gh` CLI
- **Testing Infrastructure**: MockPrBackend, property-based tests

---

## What's Next

See [CURRENT.md](../../CURRENT.md) for Phase 4: Manifest Fidelity & Workflow Gaps.
