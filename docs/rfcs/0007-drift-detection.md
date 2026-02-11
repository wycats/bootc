# RFC 0007: Configuration Drift Detection

- Status: Draft
- Feature Name: `drift_detection`
- Start Date: 2025-12-31
- RFC PR: (leave this empty until PR is opened)
- Tracking Issue: (leave this empty)

## Summary

Provide first-class drift detection that compares manifest declarations with
actual system state and reports differences in a single unified report.
This RFC remains future work, but it should build on the comparison and
diffing infrastructure that already exists in the codebase.

## Current State

Drift detection is partially built and visible in several places:

- `bkt status` computes `pending_sync`, `pending_capture`, and `has_drift` as
  aggregate drift signals across multiple subsystems.
- `bkt profile diff` prints missing and extra items for flatpaks, extensions,
  and gsettings.
- `bkt drift` exists as a command surface but currently only explains the
  concept and points users at `bkt capture --dry-run`.
- `bkt/src/manifest/diff.rs` provides a `Diffable` trait plus collection diff
  helpers used by manifest types.

The gap is not raw comparison primitives. The gap is a unified drift report,
consistent command surface, and coverage for all domains.

## What Remains To Build

### Unified Drift Report

Create a single, structured report that composes drift across subsystems,
with a summary and per-domain sections. The report should be produced by
`bkt drift check` and should also power `bkt status` when available.

### Command Surface

Implement the real `bkt drift check` command with:

- Per-domain filters (flatpak, extension, gsetting, shim, distrobox, appimage,
  homebrew, system packages).
- Human and JSON output.
- Exit codes based on drift severity.
- Optional persistence to `.local/state/bkt/last-drift-check.json` for
  `bkt drift status`.

### Domain Coverage Gaps

Some domains already have comparison logic in other commands, but drift
coverage is incomplete. The drift report should include at least:

- Flatpaks and extensions (already comparable in `bkt profile diff`).
- GSettings (diff against manifest values).
- Shims (generated files vs manifest, including missing and extra).
- Distrobox exports and packages.
- AppImage and Homebrew manifests.
- System packages and layered RPMs (as a distinct tier).

### Ignore Rules

Define ignore rules for drift reporting (a `.bktignore` file or manifest
metadata), so known intentional differences can be suppressed.

### Monitoring (Optional, Later)

Add an optional periodic drift check with user-visible reporting. This is
explicitly future work and not required for the initial command.

## Guide-level Explanation (Proposed)

### Checking For Drift

```bash
bkt drift check

# Proposed output
# Drift Report
# Flatpaks:
#   + org.gnome.Boxes (installed, not in manifest)
#   - org.gnome.Calculator (in manifest, not installed)
#
# GSettings:
#   ~ org.gnome.desktop.interface.gtk-theme
#     manifest: Adwaita-dark
#     current:  Colloid-Dark
#
# Shims:
#   - cargo (expected shim missing)
```

### Domain Filters

```bash
bkt drift check flatpak
bkt drift check gsettings
```

## Reference-level Explanation

### Data Flow

```
Collect system state -> Diff vs manifest -> Compose report -> Output
```

### Diffing Strategy

Use the existing `Diffable` trait and `diff_collections`/`diff_string_sets`
helpers to compute added, removed, and changed items for each domain.
Each subsystem should expose a small adapter that returns a `DiffResult` or a
normalized domain report which the drift report aggregates.

### Exit Codes (Proposed)

| Code | Meaning                                   |
| ---- | ----------------------------------------- |
| 0    | No drift detected                         |
| 1    | Drift detected in managed domains         |
| 2    | Error collecting state or performing diff |

## Drawbacks

- Requires careful domain coverage to avoid false positives.
- Some domains are expensive to query without caching.

## Rationale and Alternatives

This RFC consolidates existing comparison work into a single, discoverable
drift report rather than leaving drift detection scattered across commands.

## Unresolved Questions

1. Should `bkt drift check` persist full reports or only a summary?
2. What is the minimum set of domains for a useful first release?# RFC 0007: Configuration Drift Detection

- **Status**: Deferred
- Feature Name: `drift_detection`
- Start Date: 2025-12-31
- RFC PR: (leave this empty until PR is opened)
- Tracking Issue: (leave this empty)

> **⏸️ Implementation Deferred**
>
> The `bkt drift` command exists but is a stub that directs users to
> `bkt capture --dry-run` as an interim solution.
>
> The original implementation relied on a Python script, which was removed
> per the project's "No Custom Python Scripts" axiom (see [VISION.md](../VISION.md)).
>
> A native Rust implementation following this RFC's design is planned but
> not yet prioritized. The `bkt capture` workflow provides equivalent
> functionality for most use cases.

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

| Type               | Example                     | Detection                     |
| ------------------ | --------------------------- | ----------------------------- |
| **Additive**       | Extra Flatpak installed     | Compare installed vs manifest |
| **Subtractive**    | Package removed locally     | Compare manifest vs installed |
| **Modificational** | gsetting changed            | Compare current vs manifest   |
| **Version**        | Package updated outside bkt | Compare versions              |

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

### Separating User Packages from Base Image Tracking

The system maintains a clear separation between what YOU install and what Bazzite provides:

- **system-packages.json**: Packages explicitly installed by the user on the host
- **toolbox-packages.json**: Packages explicitly installed by the user in toolbox
- **base-image-assumptions.json**: What the upstream Bazzite image provides (reference only)

```bash
# See what packages we expect Bazzite to provide
bkt base list

# Verify Bazzite still provides them
bkt base verify

# See packages you explicitly added to host
bkt packages list
```

**Important**: `base-image-assumptions.json` is a **reference document** that tracks what upstream provides. It is NOT used to install packages—it's used to detect when upstream changes break our assumptions.

This ensures drift detection only flags changes to **user-managed** packages, not changes in upstream.

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

| Domain     | Collection Method                          |
| ---------- | ------------------------------------------ |
| Flatpaks   | `flatpak list --app --columns=application` |
| Packages   | `rpm -qa` + `rpm-ostree status`            |
| GSettings  | `dconf dump /`                             |
| Extensions | `gnome-extensions list --enabled`          |

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

Three manifest types serve distinct purposes:

```
manifests/
├── base-image-assumptions.json  # What Bazzite provides (upstream reference)
├── system-packages.json         # Host packages YOU added (managed by bkt)
└── toolbox-packages.json        # Toolbox packages YOU added (managed by bkt)
```

The **base-image-assumptions.json** file is a **reference document** that tracks what the upstream Bazzite image provides. It is NOT used to install packages—it's used for:

- CI verification that Bazzite still provides expected packages
- Drift detection to catch upstream breaking changes
- Documentation of our dependencies on the base image

The Containerfile references only `system-packages.json` for the `RUN dnf install` command—these are packages YOU add beyond what Bazzite provides.

```dockerfile
# === BASE IMAGE ===
FROM ghcr.io/ublue-os/bazzite-gnome:stable
# ↑ This provides everything in base-image-assumptions.json

# === USER PACKAGES (managed by bkt) ===
# Only packages YOU added beyond what Bazzite provides
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
      "additions": [{ "id": "org.gnome.Boxes", "source": "flathub" }],
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

### Interactive Resolution

The `bkt drift resolve` command provides interactive resolution for detected drift, allowing users to make informed decisions about each drifted item.

#### Command Interface

```bash
# Interactive resolution (walks through each item)
bkt drift resolve

# Domain-specific resolution
bkt drift resolve --only flatpak
bkt drift resolve --only extension
bkt drift resolve --only gsettings
bkt drift resolve --only packages

# Batch operations
bkt drift resolve --capture-all      # Capture all drift to manifest
bkt drift resolve --apply-all        # Apply manifest to system
bkt drift resolve --prefer-manifest  # Use manifest as source of truth
bkt drift resolve --prefer-system    # Use system as source of truth

# Preview mode
bkt drift resolve --dry-run          # Show what would happen
```

#### User Prompts

For each drifted item, the user sees the current state and available actions:

**Additive Drift (item in system, not in manifest):**

```
Flatpak: org.gnome.Boxes
  System: installed (not in manifest)

[c] Capture to manifest
[r] Remove from system
[s] Skip (leave as-is)
[i] Ignore permanently (add to .bktignore)
>
```

**Subtractive Drift (item in manifest, not in system):**

```
Flatpak: org.gnome.Calculator
  Manifest: declared (not installed)

[a] Apply (install from manifest)
[d] Delete from manifest
[s] Skip (leave as-is)
[i] Ignore permanently (add to .bktignore)
>
```

**Modificational Drift (item differs between manifest and system):**

```
GSettings: org.gnome.desktop.interface.gtk-theme
  Manifest: Adwaita-dark
  System:   Colloid-Dark

[c] Capture system value to manifest
[a] Apply manifest value to system
[s] Skip (leave as-is)
[i] Ignore permanently (add to .bktignore)
>
```

#### Batch Operations

For quick resolution without interactive prompts:

| Flag                | Behavior                                                               |
| ------------------- | ---------------------------------------------------------------------- |
| `--capture-all`     | Add all untracked items to manifest, update manifest for modifications |
| `--apply-all`       | Install missing items, remove untracked items, revert modifications    |
| `--prefer-manifest` | Alias for `--apply-all`—manifest is source of truth                    |
| `--prefer-system`   | Alias for `--capture-all`—system state is source of truth              |

```bash
# After reviewing drift report, capture everything
bkt drift check
bkt drift resolve --capture-all

# Or revert everything to manifest state
bkt drift resolve --apply-all
```

#### Dry Run Mode

The `--dry-run` flag shows what would happen without making changes:

```bash
bkt drift resolve --dry-run --prefer-manifest
# Would remove: org.gnome.Boxes (flatpak)
# Would install: org.gnome.Calculator (flatpak)
# Would reset: org.gnome.desktop.interface.gtk-theme → Adwaita-dark
#
# 3 changes would be made. Run without --dry-run to apply.
```

#### Integration with .bktignore

Items marked with `[i] Ignore permanently` are added to `.bktignore`:

```ini
# .bktignore
# Automatically added via `bkt drift resolve`
flatpak:org.gnome.Boxes           # Ignored 2025-01-02
gsettings:org.gnome.desktop.privacy.remember-recent-files  # Ignored 2025-01-02
```

Ignored items are excluded from future drift detection:

```bash
# Show what's being ignored
bkt drift ignored

# Remove an item from ignore list
bkt drift unignore flatpak:org.gnome.Boxes
```

#### Domain-Specific Resolution

Resolve drift for specific domains only:

```bash
# Only resolve flatpak drift
bkt drift resolve --only flatpak

# Only resolve extension drift
bkt drift resolve --only extension

# Only resolve gsettings drift
bkt drift resolve --only gsettings

# Only resolve package drift
bkt drift resolve --only packages

# Combine with batch operations
bkt drift resolve --only flatpak --capture-all
```

#### Resolution Report

After resolution, a summary is displayed:

```
╭─────────────────────────────────────╮
│ Drift Resolution Complete           │
├─────────────────────────────────────┤
│ Captured to manifest:  3            │
│ Applied from manifest: 1            │
│ Removed from system:   1            │
│ Added to .bktignore:   2            │
│ Skipped:               0            │
╰─────────────────────────────────────╯

Manifest updated. Run `bkt build` to regenerate Containerfile.
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

**Resolution**: Three separate manifests: `base-image-assumptions.json` (what Bazzite provides—upstream reference), `system-packages.json` (user-added host packages), and `toolbox-packages.json` (user-added toolbox packages). Drift detection only applies to user-managed packages. Base image assumptions are verified separately via `bkt base verify`.

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
