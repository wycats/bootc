# RFC 0016: GSettings Discovery and Baseline Management

- Feature Name: `gsettings_discovery`
- Start Date: 2026-01-19
- RFC PR: (leave this empty until PR is opened)
- Tracking Issue: (leave this empty)

## Summary

Implement a baseline-based discovery mechanism for GSettings that enables users to identify which settings have drifted from their initial state, without needing to know the exact schema or key names. This complements RFC 0007's drift detection by providing the foundational infrastructure for GSettings-specific drift analysis.

## Motivation

Currently, `bkt gsetting capture` requires knowing the exact schema and key to capture:

```bash
# User must already know the schema and key
bkt gsetting capture org.gnome.desktop.interface --key gtk-theme
```

This creates a discovery problem:

1. **Users don't know what changed**: When you tweak a setting in GNOME Settings, you don't know which dconf path was modified
2. **No way to find drifted settings**: Without a baseline, there's no reference point to compare against
3. **Manual hunting is tedious**: Users must grep through `dconf dump /` output and guess which settings matter
4. **Transient settings pollute results**: Window positions, recent files, and other ephemeral state obscure meaningful changes

### The Baseline Approach

By capturing a snapshot of dconf state at a known-good point (typically after a fresh image boot), we create a reference against which to compare. Any differences represent intentional customizations worth preserving—or accidental drift to investigate.

```
+------------------+     +------------------+     +------------------+
|  Fresh Boot      |---->|    Baseline      |---->|  User Changes    |
| (default state)  |     | (captured once)  |     | (detected diff)  |
+------------------+     +------------------+     +------------------+
```

## Guide-level Explanation

### Creating a Baseline

After a fresh image boot (or when you want to "reset" your baseline), capture the current state:

```bash
# Create initial baseline from current dconf state
bkt gsetting baseline create

# View baseline info
bkt gsetting baseline info
# Created: 2026-01-19 14:30:00
# Entries: 4,521
# Location: ~/.local/share/bkt/gsettings-baseline.txt
```

The baseline is a snapshot of `dconf dump /` output, representing all GSettings values at that moment.

### Discovering Changed Settings

Once you have a baseline, you can see what's changed:

```bash
# Show all settings that differ from baseline
bkt gsetting diff

# +-------------------------------------------------------------+
# | GSettings Changes from Baseline                             |
# +-------------------------------------------------------------+
# | Modified:                                                    |
# |   org.gnome.desktop.interface.gtk-theme                     |
# |     baseline: Adwaita                                       |
# |     current:  Colloid-Dark                                  |
# |                                                             |
# |   org.gnome.desktop.interface.color-scheme                  |
# |     baseline: default                                       |
# |     current:  prefer-dark                                   |
# |                                                             |
# | Added (not in baseline):                                    |
# |   org.gnome.shell.extensions.dash-to-dock.dock-position     |
# |     current: BOTTOM                                         |
# |                                                             |
# | Ignored: 47 transient settings (window positions, etc.)     |
# +-------------------------------------------------------------+
```

### Capturing All Changed Settings

Instead of capturing one schema at a time, capture everything that changed:

```bash
# Preview what would be captured
bkt gsetting capture --all-changed

# GSettings Capture: 3 to add, 0 already in manifest
#   [capture] gsetting:org.gnome.desktop.interface.gtk-theme = Colloid-Dark
#   [capture] gsetting:org.gnome.desktop.interface.color-scheme = prefer-dark
#   [capture] gsetting:org.gnome.shell.extensions.dash-to-dock.dock-position = BOTTOM
#
# Use --apply to execute this plan.

# Actually capture them
bkt gsetting capture --all-changed --apply
```

### Managing the Baseline

```bash
# Update baseline to current state (after legitimizing changes)
bkt gsetting baseline reset

# Show what would change in the baseline
bkt gsetting baseline reset --dry-run
```

### Filtering Transient Settings

Some settings are inherently transient and should be ignored:

```bash
# View current ignore patterns
bkt gsetting ignore list

# Add a pattern to ignore
bkt gsetting ignore add "org.gnome.desktop.app-folders.*"

# Remove a pattern
bkt gsetting ignore remove "org.gnome.desktop.app-folders.*"
```

## Reference-level Explanation

### Baseline Storage

The baseline is stored as a plain text file in dconf dump format:

```
~/.local/share/bkt/gsettings-baseline.txt
```

Format (standard dconf dump output):

```ini
[org/gnome/desktop/interface]
gtk-theme='Adwaita'
icon-theme='Adwaita'
cursor-theme='Adwaita'
font-name='Cantarell 11'

[org/gnome/desktop/wm/preferences]
button-layout='appmenu:minimize,maximize,close'
```

### Schema Filter Configuration

Ignore patterns are stored in the bkt configuration:

```
~/.config/bkt/gsettings-ignore.toml
```

```toml
# Patterns to ignore when computing diffs
# Uses glob-style matching against "schema.key" format

[ignore]
patterns = [
    # Window geometry (always transient)
    "org.gnome.*.window-*",
    "*.window-position",
    "*.window-size",
    "*.window-maximized",

    # Recent files and history
    "org.gnome.*.recent-*",
    "*.recent-files",
    "*.history",
    "*.file-history",

    # Application state (not configuration)
    "org.gnome.nautilus.preferences.search-filter-time-type",
    "org.gnome.nautilus.icon-view.captions",

    # Extension internal state
    "org.gnome.shell.extensions.*.last-*",
    "org.gnome.shell.extensions.*.cached-*",

    # Session state
    "org.gnome.desktop.screensaver.lock-delay",
    "org.gnome.settings-daemon.plugins.power.sleep-inactive-*",
]

# Schemas to completely ignore (all keys)
schemas = [
    "org.gtk.Settings.FileChooser",           # File dialog state
    "org.gtk.gtk4.Settings.FileChooser",      # GTK4 file dialog state
    "org.gnome.desktop.app-folders",          # App folder organization
    "org.gnome.shell.app-switcher",           # App switcher state
    "org.gnome.desktop.input-sources",        # Keyboard layouts (often session-specific)
]
```

### Default Ignore Patterns

The following patterns are always ignored (built-in defaults):

| Category            | Pattern                               | Reason                           |
| ------------------- | ------------------------------------- | -------------------------------- |
| Window State        | `*.window-*`                          | Geometry is session-specific     |
| Recent Files        | `*.recent-*`, `*.history`             | User activity, not configuration |
| File Chooser        | `org.gtk.Settings.FileChooser.*`      | Dialog state                     |
| App Folders         | `org.gnome.desktop.app-folders.*`     | Grid organization                |
| Disabled Extensions | `org.gnome.shell.disabled-extensions` | Managed by extension system      |

### Diff Algorithm

```rust
/// Compare current dconf state against baseline.
fn compute_diff(
    baseline: &DconfDump,
    current: &DconfDump,
    ignore: &IgnorePatterns,
) -> GsettingsDiff {
    let mut diff = GsettingsDiff::new();

    // Find modified and removed settings
    for (path, baseline_value) in baseline.entries() {
        if ignore.matches(path) {
            diff.ignored += 1;
            continue;
        }

        match current.get(path) {
            Some(current_value) if current_value != baseline_value => {
                diff.modified.push(ModifiedSetting {
                    path: path.clone(),
                    baseline: baseline_value.clone(),
                    current: current_value.clone(),
                });
            }
            None => {
                diff.removed.push(RemovedSetting {
                    path: path.clone(),
                    baseline: baseline_value.clone(),
                });
            }
            _ => {} // Unchanged
        }
    }

    // Find added settings
    for (path, current_value) in current.entries() {
        if ignore.matches(path) {
            diff.ignored += 1;
            continue;
        }

        if !baseline.contains(path) {
            diff.added.push(AddedSetting {
                path: path.clone(),
                current: current_value.clone(),
            });
        }
    }

    diff
}
```

### New CLI Commands

```rust
#[derive(Debug, Subcommand)]
pub enum GSettingAction {
    // ... existing commands ...

    /// Show settings that differ from baseline
    Diff {
        /// Include transient settings (window positions, etc.)
        #[arg(long)]
        include_transient: bool,

        /// Output format (table, json)
        #[arg(short, long, default_value = "table")]
        format: String,
    },

    /// Manage the GSettings baseline
    Baseline {
        #[command(subcommand)]
        action: BaselineAction,
    },

    /// Manage ignore patterns
    Ignore {
        #[command(subcommand)]
        action: IgnoreAction,
    },
}

#[derive(Debug, Subcommand)]
pub enum BaselineAction {
    /// Create a new baseline from current state
    Create {
        /// Overwrite existing baseline
        #[arg(long)]
        force: bool,
    },
    /// Show baseline information
    Info,
    /// Update baseline to current state
    Reset,
}

#[derive(Debug, Subcommand)]
pub enum IgnoreAction {
    /// List current ignore patterns
    List,
    /// Add an ignore pattern
    Add { pattern: String },
    /// Remove an ignore pattern
    Remove { pattern: String },
}
```

### Modified Capture Command

```rust
/// Capture current GSettings values to manifest
Capture {
    /// Schema name to capture (captures all keys from this schema)
    #[arg(conflicts_with = "all_changed")]
    schema: Option<String>,

    /// Specific key to capture (optional - defaults to all keys in schema)
    #[arg(short, long, requires = "schema")]
    key: Option<String>,

    /// Capture all settings that differ from baseline
    #[arg(long, conflicts_with = "schema")]
    all_changed: bool,

    /// Apply the plan immediately (default is preview only)
    #[arg(long)]
    apply: bool,
}
```

### Integration with `bkt status`

The drift detection system (RFC 0007) integrates with baseline diffs:

```bash
bkt status

# ... other status sections ...
#
# GSettings:
#   3 settings differ from baseline (use `bkt gsetting diff` for details)
#   2 settings in manifest don't match current values
```

```rust
/// GSettings drift detection for bkt status.
fn check_gsettings_drift() -> GsettingsDriftSummary {
    let baseline = load_baseline()?;
    let current = capture_current_dconf()?;
    let ignore = load_ignore_patterns()?;

    let diff = compute_diff(&baseline, &current, &ignore);

    GsettingsDriftSummary {
        baseline_changes: diff.modified.len() + diff.added.len(),
        manifest_mismatches: count_manifest_mismatches(),
    }
}
```

## Rationale and Alternatives

### Why dconf dump instead of gsettings list-recursively?

- `dconf dump /` is faster (single command vs many gsettings calls)
- Captures all backends consistently
- Output format is simple to parse and diff
- Includes settings that gsettings might not list (extension schemas)

### Why file-based baseline instead of database?

- Simple to inspect and debug
- Can be versioned in git if desired
- Easy to share/copy between machines
- dconf dump format is well-known

### Why ignore patterns instead of allowlist?

- GNOME has thousands of schemas; allowlist would be unmaintainable
- Most settings are configuration, only a few are transient state
- Ignore patterns are easier to reason about
- Users can add project-specific patterns

### Alternative: Schema-based heuristics

We could try to automatically detect transient settings by schema naming patterns. However:

- Too fragile; naming is inconsistent
- Some "transient-looking" settings are actually important
- User override capability is still needed

### Alternative: Per-app baseline

Instead of one global baseline, maintain per-application baselines. Rejected because:

- Complicates the mental model
- Most customizations are cross-cutting (themes, fonts)
- Extension settings don't map cleanly to apps

## Prior Art

- **dconf-editor**: Shows all settings but no diff capability
- **GNOME Tweaks**: Manages specific settings but doesn't track changes
- **Ansible gsetting module**: Sets values but doesn't discover changes
- **Chezmoi**: Similar baseline concept for dotfiles

## Resolved Questions

1. **Baseline versioning**: Should we track baseline metadata (image version, date)?
   
   **Resolution**: Yes. The baseline file should include metadata:
   ```
   # bkt gsettings baseline
   # Created: 2026-01-19T10:30:00Z
   # Image: ghcr.io/wycats/bazzite-dx:2026.01.15
   # Boot ID: abc123...
   
   [org/gnome/desktop/interface]
   gtk-theme='Adwaita'
   ...
   ```
   This helps debug "why does my baseline have unexpected values" scenarios.

2. **Automatic baseline on first boot**: Should `bkt` auto-create baseline when none exists?
   
   **Resolution**: Yes. When `bkt gsetting diff` or related commands are run and no baseline exists, automatically create one from current state with a notice:
   ```
   ℹ️ No baseline found. Creating baseline from current state.
      Future runs will compare against this snapshot.
   ```
   This provides zero-config experience while being transparent about what happened.

3. **Multi-baseline support**: Should users be able to maintain named baselines?
   
   **Resolution**: Defer to future. Single baseline covers the primary use case. Named baselines could be added later for advanced workflows.

4. **Extension schema discovery**: How do we handle dynamically-installed extension schemas?
   
   **Resolution**: Extensions install their schemas to `~/.local/share/gnome-shell/extensions/*/schemas/`. The `dconf dump` will include these paths. We should include extension schemas in diff but clearly label them as extension-specific in output.

## Future Possibilities

### Interactive Discovery Mode

```bash
bkt gsetting watch
# Monitoring for changes... (Ctrl+C to stop)
#
# [14:32:15] org.gnome.desktop.interface.gtk-theme: Adwaita → Colloid-Dark
# [14:32:18] org.gnome.desktop.interface.color-scheme: default → prefer-dark
#
# Capture these changes? [y/N]
```

### Smart Categorization

Automatically categorize detected changes:

```bash
bkt gsetting diff --categorized

# Appearance (3 changes)
#   gtk-theme, icon-theme, color-scheme
#
# Keyboard (1 change)
#   keybindings.switch-windows
#
# Extensions (2 changes)
#   dash-to-dock.dock-position, dash-to-dock.dash-max-icon-size
```

### Baseline Profiles

Named baselines for different configurations:

```bash
# Create a "work" baseline
bkt gsetting baseline create --name work

# Switch between profiles
bkt gsetting diff --baseline work
```

### Export for New Machine Setup

```bash
# Export all changed settings as a portable script
bkt gsetting export > my-settings.sh

# On new machine
./my-settings.sh
```
