# RFC 0007: Tier 2 — Runtime State & Drift Detection

- **Status**: Partially Implemented
- Feature Name: `drift_detection`
- Start Date: 2025-12-31
- RFC PR: (leave this empty until PR is opened)
- Tracking Issue: (leave this empty)

> **ℹ️ Implementation Status**
>
> | Feature                    | Status         | Notes                                          |
> | -------------------------- | -------------- | ---------------------------------------------- |
> | `bkt status` drift signals | ✅ Implemented | `pending_sync`, `pending_capture`, `has_drift` |
> | `bkt profile diff`         | ✅ Implemented | Flatpaks, extensions, gsettings                |
> | `bkt drift` command        | ⚠️ Stub        | Points to `bkt capture --dry-run`              |
> | Unified drift report       | ❌ Not started |                                                |
> | Systemd service state      | ❌ Not started | New domain proposed in this RFC                |
> | `.bktignore`               | ❌ Not started |                                                |

> **⚠️ Absorbed by [RFC-0052](0052-manifest-lifecycle.md).**
> The tier model, apply/capture lifecycle, and drift detection concepts from this
> RFC are consolidated into RFC-0052. The interactive drift resolve, `.bktignore`,
> and drift monitoring features are not yet carried forward.

## Summary

This RFC defines **Tier 2 state management** — runtime configuration that lives
outside the bootc image and can drift from declared manifests. Tier 2 changes
can be applied immediately without a reboot, but the system state may diverge
from manifests over time.

Tier 2 is the complement to [RFC-0004](0004-bkt-admin.md) (Tier 1), which covers
image-bound state that requires a rebuild and reboot.

## The Tier Model

The system has two fundamentally different state lifecycles:

| Tier                                       | State Location                        | Change Mechanism      | Drift Possible?                 |
| ------------------------------------------ | ------------------------------------- | --------------------- | ------------------------------- |
| **Tier 1** ([RFC-0004](0004-bkt-admin.md)) | Baked into image                      | PR → build → reboot   | No — image is deterministic     |
| **Tier 2** (this RFC)                      | Runtime (flatpak DB, gsettings, /etc) | Immediate or deferred | Yes — runtime state can diverge |

### What Belongs in Tier 2

- **Flatpaks** — Applications installed via Flatpak
- **GNOME Extensions** — Shell extensions (enabled/disabled state)
- **GSettings** — GNOME configuration values
- **Shims** — Host command wrappers in `~/.local/bin`
- **Host binaries** — Binaries installed via `fetchbin` to `~/.local/bin`
- **Distrobox** — Container configurations and exports
- **Toolbox packages** — Packages installed inside distrobox containers
- **AppImages** — Portable applications
- **Homebrew** — Packages in the Homebrew prefix (inside distrobox)
- **Systemd service state** (runtime) — Services enabled/disabled/masked in `/etc`

Note: Some Tier 2 domains live inside containers (homebrew, toolbox-packages),
but they use the same change mechanism — local modification captured to manifest
for reproducibility. The container boundary is orthogonal to the tier model.

### Why Tier 2 Can Drift

Tier 2 state lives in mutable locations:

- Flatpak database (`~/.local/share/flatpak`, `/var/lib/flatpak`)
- GSettings/dconf (`~/.config/dconf`)
- Systemd runtime state (`/etc/systemd/system/`)

Users (or other tools) can modify this state directly, causing it to diverge
from the declared manifests. Drift detection identifies these divergences.

## Motivation

Despite best intentions, systems drift:

1. **Ad-hoc installations**: `flatpak install` without `bkt`
2. **Manual gsettings**: Changes made via GUI or direct commands
3. **Forgotten experiments**: Packages installed for testing, never removed
4. **External tools**: Other scripts modifying system state
5. **Masked services**: Someone runs `systemctl mask` and forgets

Drift is inevitable. The question is: how quickly do you detect it?

### The Manifest-Driven Approach

**Important**: The manifest is the source of truth.

```
+-------------+     +---------------+     +-------------+
|  manifest   |---->|   bkt sync    |---->|   System    |
|  (source)   |     |  (converge)   |     |  (runtime)  |
+-------------+     +---------------+     +-------------+
       ↑                                        |
       +------------- bkt capture <-------------+
```

Users interact with manifests via `bkt` commands. The system converges to
manifest state via `bkt sync`, and runtime changes are captured back via
`bkt capture`.

### Types of Drift

| Type               | Example                             | Detection                      |
| ------------------ | ----------------------------------- | ------------------------------ |
| **Additive**       | Extra Flatpak installed             | Compare installed vs manifest  |
| **Subtractive**    | Package removed locally             | Compare manifest vs installed  |
| **Modificational** | gsetting changed                    | Compare current vs manifest    |
| **State**          | Service masked when it shouldn't be | Compare unit state vs manifest |

## Guide-level Explanation

### Checking for Drift

```bash
# Full drift report
bkt drift check

# Example output:
# Drift Report
# ============
#
# Flatpaks:
#   + org.gnome.Boxes (installed, not in manifest)
#   - org.gnome.Calculator (in manifest, not installed)
#
# GSettings:
#   ~ org.gnome.desktop.interface.gtk-theme
#     manifest: Adwaita-dark
#     current:  Colloid-Dark
#
# Systemd Services:
#   ~ tuned.service
#     manifest: enabled
#     current:  masked
#
# Shims:
#   - cargo (expected shim missing)

# Check specific domain
bkt drift check flatpak
bkt drift check gsettings
bkt drift check systemd
```

### Resolving Drift

```bash
# Interactive resolution
bkt drift resolve
# For each drift item:
#   [c] Capture to manifest (legitimize the change)
#   [a] Apply manifest to system (revert the change)
#   [s] Skip (ignore for now)
#   [i] Ignore permanently (add to .bktignore)

# Auto-resolve: prefer manifest
bkt drift resolve --prefer-manifest

# Auto-resolve: prefer system
bkt drift resolve --prefer-system
```

### Domain: Systemd Service State (NEW)

This RFC adds **systemd service runtime state** as a Tier 2 domain. This tracks
the enable/disable/mask state of services in `/etc/systemd/system/`, which is
mutable and persists across reboots but is NOT part of the image.

#### Why This Matters

The tuned/tuned-ppd incident illustrates the problem:

- Bazzite ships with `tuned` and `tuned-ppd` enabled via presets
- Someone (or something) ran `systemctl mask tuned`
- The mask persisted in `/etc/systemd/system/tuned.service -> /dev/null`
- Power management silently broke
- No tooling detected or reported this drift

#### Manifest Format

```json
// manifests/systemd-services.json
{
  "$schema": "../schemas/systemd-services.schema.json",
  "services": {
    "tuned.service": "enabled",
    "tuned-ppd.service": "enabled",
    "power-profiles-daemon.service": "masked",
    "cups.service": "disabled"
  }
}
```

Valid states: `enabled`, `disabled`, `masked`

#### Command Surface

```bash
# Declare intended state
bkt systemd enable tuned.service tuned-ppd.service
bkt systemd disable cups.service
bkt systemd mask power-profiles-daemon.service

# Check drift
bkt drift check systemd

# Apply manifest state to system
bkt sync systemd

# Capture current state to manifest
bkt capture systemd
```

#### Implementation

The existing `bkt/src/dbus/systemd.rs` has `enable()` and `disable()` methods.
This RFC adds:

- `mask()` / `unmask()` methods via D-Bus
- A new `systemd-services.json` manifest
- Drift detection comparing manifest to `systemctl is-enabled` output
- Sync logic to reconcile state

### Continuous Monitoring (Future)

```bash
# Enable drift monitoring (systemd timer)
bkt drift monitor enable

# Disable
bkt drift monitor disable

# Check last report
bkt drift monitor status
```

Monitoring creates periodic drift reports and can notify via desktop notification.

## Reference-level Explanation

### Drift Detection Pipeline

```
+-------------+     +-------------+     +-------------+
|   Collect   |---->|   Compare   |---->|   Report    |
| System State|     | vs Manifest |     |   Drift     |
+-------------+     +-------------+     +-------------+
```

#### Collecting System State

| Domain      | Collection Method                                         |
| ----------- | --------------------------------------------------------- |
| Flatpaks    | `flatpak list --app --columns=application`                |
| Extensions  | `gnome-extensions list --enabled`                         |
| GSettings   | `gsettings get <schema> <key>`                            |
| Shims       | Check file existence in shims directory                   |
| Distrobox   | `distrobox list`                                          |
| AppImages   | Scan AppImage directory                                   |
| Homebrew    | `brew list`                                               |
| **Systemd** | `systemctl is-enabled <unit>` or D-Bus `GetUnitFileState` |

#### Systemd State Detection

For each service in the manifest:

```rust
fn get_service_state(unit: &str) -> Result<ServiceState> {
    // Use D-Bus org.freedesktop.systemd1.Manager.GetUnitFileState
    // Returns: "enabled", "disabled", "masked", "static", etc.
    let state = systemd_manager.get_unit_file_state(unit)?;

    // Also check for runtime masks in /etc/systemd/system/
    let etc_path = format!("/etc/systemd/system/{}", unit);
    if std::fs::read_link(&etc_path).map(|p| p == Path::new("/dev/null")).unwrap_or(false) {
        return Ok(ServiceState::Masked);
    }

    match state.as_str() {
        "enabled" | "enabled-runtime" => Ok(ServiceState::Enabled),
        "disabled" => Ok(ServiceState::Disabled),
        "masked" | "masked-runtime" => Ok(ServiceState::Masked),
        "static" => Ok(ServiceState::Static), // No [Install] section
        _ => Ok(ServiceState::Unknown),
    }
}
```

### Manifest Formats

#### systemd-services.json (NEW)

```json
{
  "$schema": "../schemas/systemd-services.schema.json",
  "services": {
    "tuned.service": "enabled",
    "tuned-ppd.service": "enabled",
    "power-profiles-daemon.service": "masked"
  }
}
```

#### Drift Report Format

```json
{
  "generated_at": "2026-02-18T10:30:00Z",
  "domains": {
    "flatpak": {
      "additions": [{ "id": "org.gnome.Boxes", "source": "flathub" }],
      "removals": [],
      "modifications": []
    },
    "gsettings": {
      "modifications": [
        {
          "key": "org.gnome.desktop.interface.gtk-theme",
          "expected": "Adwaita-dark",
          "actual": "Colloid-Dark"
        }
      ]
    },
    "systemd": {
      "modifications": [
        {
          "unit": "tuned.service",
          "expected": "enabled",
          "actual": "masked"
        }
      ]
    }
  },
  "summary": {
    "total_drift": 3,
    "by_type": { "additions": 1, "modifications": 2 }
  }
}
```

### Exit Codes

| Code | Meaning                                   |
| ---- | ----------------------------------------- |
| 0    | No drift detected                         |
| 1    | Drift detected in managed domains         |
| 2    | Error collecting state or performing diff |

### Ignore Patterns

```ini
# .bktignore
# Ignore specific items
flatpak:org.gnome.Boxes

# Ignore patterns
gsettings:org.gnome.desktop.privacy.*

# Ignore systemd services
systemd:cups.service
```

## Relationship to Other RFCs

| RFC                                                         | Relationship                                   |
| ----------------------------------------------------------- | ---------------------------------------------- |
| [RFC-0004](0004-bkt-admin.md)                               | Tier 1 complement — image-bound state          |
| [RFC-0021](0021-local-change-management.md)                 | Ephemeral manifest for uncommitted changes     |
| [RFC-0023](0023-system-status-dashboard.md)                 | `bkt status` consumes drift signals            |
| [RFC-0048](0048-subsystem-and-containerfile-unification.md) | Defines `SubsystemTier::Convergent` for Tier 2 |

## Drawbacks

### Performance

Full drift checks can be slow. Mitigation: incremental checks, caching.

### False Positives

Some drift is intentional. Mitigation: `.bktignore` and interactive resolution.

### Systemd Complexity

Systemd has many unit states beyond enabled/disabled/masked. Mitigation: focus
on the common cases; treat "static" units as informational.

## Rationale and Alternatives

### Why Not Just Trust the Manifest?

Because humans forget to use `bkt` commands. And external tools (like whatever
masked tuned) don't know about our manifests.

### Alternative: Immutable Everything

Reboot to apply all changes. Too disruptive for development workflow.

### Alternative: No Systemd Tracking

Leave systemd state unmanaged. But the tuned incident shows this leads to
silent, persistent failures that are hard to diagnose.

## Prior Art

- **Puppet/Chef/Ansible**: Desired state configuration with drift detection
- **Terraform Plan**: Shows diff between desired and actual state
- **etckeeper**: Track /etc in git
- **systemd presets**: Distribution-level service configuration

## Future Possibilities

- **Drift Webhooks**: Notify external systems
- **Drift History**: Track drift over time
- **Predictive Drift**: Warn before drift occurs
- **Boot-time Reconciliation**: Automatically fix drift on login
