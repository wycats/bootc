# RFC 0007: Configuration Drift Detection

- Feature Name: `drift_detection`
- Start Date: 2025-12-31
- RFC PR: (leave this empty until PR is opened)
- Tracking Issue: (leave this empty)

## Summary

Implement mechanisms to detect when the running system's state diverges from the declared manifest state, enabling proactive identification and resolution of configuration drift.

## Motivation

Despite best intentions, systems drift:

1. **Ad-hoc installations**: `flatpak install` without `bkt`
2. **Manual gsettings**: Changes made via GUI or direct commands
3. **Forgotten experiments**: Packages installed for testing, never removed
4. **External tools**: Other scripts modifying system state

Drift is inevitable. The question is: how quickly do you detect it?

### The Manifest-Driven Approach

**Important**: The manifest is the source of truth. The Containerfile is an **output**, never edited directly.

```
+-------------+     +---------------+     +-------------+
|  manifest   |---->| Containerfile |---->|   Image     |
|  (source)   |     |  (generated)  |     |  (output)   |
+-------------+     +---------------+     +-------------+
```

Users interact with manifests via `bkt` commands. The Containerfile is regenerated automatically.

### Types of Drift

| Type | Example | Detection |
|------|---------|-----------|
| **Additive** | Extra Flatpak installed | Compare installed vs manifest |
| **Subtractive** | Package removed locally | Compare manifest vs installed |
| **Modificational** | gsetting changed | Compare current vs manifest |
| **Version** | Package updated outside bkt | Compare versions |

## Guide-level Explanation

### Checking for Drift

```bash
# Full drift report
bkt drift check
# +-------------------------------------------------------------+
# | Drift Report                                                |
# +-------------------------------------------------------------+
# | Flatpaks:                                                   |
# |   + org.gnome.Boxes (installed, not in manifest)            |
# |   - org.gnome.Calculator (in manifest, not installed)       |
# |                                                             |
# | GSettings:                                                  |
# |   ~ org.gnome.desktop.interface.gtk-theme                   |
# |     manifest: Adwaita-dark                                  |
# |     current:  Colloid-Dark                                  |
# |                                                             |
# | Packages:                                                   |
# |   + htop (layered, not in manifest)                         |
# +-------------------------------------------------------------+

# Check specific domain
bkt drift check flatpak
bkt drift check gsettings
bkt drift check packages
```

### Resolving Drift

```bash
# Interactive resolution
bkt drift resolve
# For each drift item:
#   [a] Add to manifest (legitimize the change)
#   [r] Remove/revert (restore manifest state)
#   [s] Skip (ignore for now)
#   [i] Ignore permanently (add to .bktignore)

# Auto-resolve: prefer manifest
bkt drift resolve --prefer-manifest

# Auto-resolve: prefer system
bkt drift resolve --prefer-system
```

### Continuous Monitoring

```bash
# Enable drift monitoring (systemd timer)
bkt drift monitor enable

# Disable
bkt drift monitor disable

# Check last report
bkt drift monitor status
```

Monitoring creates periodic drift reports and can notify via desktop notification.

### Separating Manifest from System Packages

The drift detection system maintains a clear separation:

- **manifest.json**: Packages explicitly requested by the user
- **system-packages.json**: Packages installed by base image (Bazzite)

```bash
# See what packages came from the base image
bkt packages base

# See what packages you explicitly added
bkt packages user

# Compare
bkt packages diff
```

This ensures drift detection only flags changes to **user-managed** packages, not base image contents.

### Hybrid Tracking for Upstream Issues

Sometimes you need a package temporarily because upstream is missing a feature. The **hybrid tracking** approach:

```bash
# Install package, file upstream issue
bkt dnf install missing-feature --track-upstream https://github.com/org/repo/issues/123

# View tracked packages
bkt dnf tracked
# missing-feature
#   Upstream: https://github.com/org/repo/issues/123
#   Expected: 2025-Q2
#   Action: Remove when fixed

# When upstream fixes it
bkt dnf untrack missing-feature
```

#### Manifest Entry

```json
{
  "packages": ["missing-feature"],
  "tracking": {
    "missing-feature": {
      "upstream_issue": "https://github.com/org/repo/issues/123",
      "added": "2025-01-02",
      "expected_resolution": "2025-Q2",
      "action_on_resolution": "remove",
      "notes": "Workaround until upstream adds this feature"
    }
  }
}
```

This enables:
- **Proactive cleanup**: Periodic check of tracked issues
- **Documentation**: Why was this installed?
- **Automated reminders**: Notify when issue is closed

## Reference-level Explanation

### Drift Detection Pipeline

```
+-------------+     +-------------+     +-------------+
|   Collect   |---->|   Compare   |---->|   Report    |
| System State|     | vs Manifest |     |   Drift     |
+-------------+     +-------------+     +-------------+
```

#### Collecting System State

| Domain | Collection Method |
|--------|------------------|
| Flatpaks | `flatpak list --app --columns=application` |
| Packages | `rpm -qa` + `rpm-ostree status` |
| GSettings | `dconf dump /` |
| Extensions | `gnome-extensions list --enabled` |

#### Comparison Logic

```rust
fn detect_drift(manifest: &Manifest, system: &SystemState) -> DriftReport {
    let mut report = DriftReport::new();
    
    // Check for additions (in system, not in manifest)
    for item in &system.items {
        if !manifest.contains(item) {
            report.additions.push(item.clone());
        }
    }
    
    // Check for removals (in manifest, not in system)
    for item in &manifest.items {
        if !system.contains(item) {
            report.removals.push(item.clone());
        }
    }
    
    // Check for modifications (in both, but different)
    for item in &manifest.items {
        if let Some(sys_item) = system.get(item.id()) {
            if sys_item != item {
                report.modifications.push(Modification {
                    expected: item.clone(),
                    actual: sys_item.clone(),
                });
            }
        }
    }
    
    report
}
```

### Manifest Separation

Two distinct manifest files prevent confusion:

```
manifests/
├── system-packages.json    # Base image packages (read-only reference)
└── user-packages.json      # User-added packages (managed by bkt)
```

The Containerfile references only `user-packages.json` for the `RUN dnf install` command.

```dockerfile
# === BASE IMAGE ===
FROM ghcr.io/ublue-os/bazzite-gnome:stable

# === USER PACKAGES (managed by bkt) ===
# Only packages YOU added, not everything in bazzite
RUN dnf install -y \
    htop \
    neovim
# === END USER PACKAGES ===
```

### Ignore Patterns

```ini
# .bktignore
# Ignore specific items
flatpak:org.gnome.Boxes

# Ignore patterns
gsettings:org.gnome.desktop.privacy.*

# Ignore entire domains
# [disabled] packages:*
```

### Drift Report Format

```json
{
  "generated_at": "2025-01-02T10:30:00Z",
  "domains": {
    "flatpak": {
      "additions": [
        {"id": "org.gnome.Boxes", "source": "flathub"}
      ],
      "removals": [],
      "modifications": []
    },
    "gsettings": {
      "additions": [],
      "removals": [],
      "modifications": [
        {
          "key": "org.gnome.desktop.interface.gtk-theme",
          "expected": "Adwaita-dark",
          "actual": "Colloid-Dark"
        }
      ]
    }
  },
  "summary": {
    "total_drift": 2,
    "by_type": {
      "additions": 1,
      "modifications": 1
    }
  }
}
```

### Systemd Timer

```ini
# ~/.config/systemd/user/bkt-drift.timer
[Unit]
Description=Periodic drift check

[Timer]
OnBootSec=5min
OnUnitActiveSec=1h

[Install]
WantedBy=timers.target
```

```ini
# ~/.config/systemd/user/bkt-drift.service
[Unit]
Description=Check for configuration drift

[Service]
Type=oneshot
ExecStart=/usr/bin/bkt drift check --quiet --notify
```

## Drawbacks

### Performance

Full drift checks can be slow. Mitigation: incremental checks, caching.

### False Positives

Some drift is intentional. Mitigation: `.bktignore` and interactive resolution.

### Privacy Concerns

Drift reports contain system state. Mitigation: reports stay local unless explicitly shared.

## Rationale and Alternatives

### Why Not Just Trust the Manifest?

Because humans forget to use `bkt` commands.

### Alternative: Git-based Tracking

Compare manifests across commits. Useful for historical analysis but doesn't catch runtime drift.

### Alternative: Immutable Everything

Reboot to apply all changes. Too disruptive for development workflow.

## Prior Art

- **Puppet/Chef/Ansible**: Desired state configuration with drift detection
- **Terraform Plan**: Shows diff between desired and actual state
- **etckeeper**: Track /etc in git

## Unresolved Questions

### Q1: Manifest Separation

**Resolution**: Separate `system-packages.json` (base image reference) from `user-packages.json` (user managed). Drift detection only applies to user-managed packages.

### Q2: GSettings Scope

**Resolution**: Track only explicitly managed settings. Use `gsettings reset` for schema-provided defaults.

### Q3: Performance

**Resolution**: Incremental checks with caching. Full check only on demand.

### Q4: Extension State

**Resolution**: Track enabled/disabled state. Extension version managed separately.

### Q5: Layered Packages

**Resolution**: Include `rpm-ostree` layered packages in drift detection.

### Q6: Temporary Packages

**Resolution**: Use hybrid tracking with `--track-upstream` for temporary workarounds.

## Future Possibilities

- **Drift Webhooks**: Notify external systems
- **Drift History**: Track drift over time
- **Predictive Drift**: Warn before drift occurs (e.g., "You're about to run `flatpak install` - use `bkt flatpak add` instead")
- **Drift Remediation Playbooks**: Pre-defined resolution strategies
