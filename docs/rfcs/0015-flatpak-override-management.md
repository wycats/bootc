# RFC 0015: Flatpak Override Management

- Feature Name: `flatpak_override_management`
- Start Date: 2026-01-19
- RFC PR: (leave this empty until PR is opened)
- Tracking Issue: (leave this empty)

## Summary

Add support for capturing, storing, and restoring Flatpak permission overrides (the kind configured through Flatseal). This enables `bkt` to preserve user-customized sandbox permissions across image rebuilds.

## Motivation

Flatpak applications run in sandboxed environments with restricted permissions. Users frequently need to grant additional permissions:

- Filesystem access (e.g., `~/Documents` for a text editor)
- Device access (e.g., webcam for video conferencing)
- D-Bus access (e.g., screen sharing permissions)
- Environment variables (e.g., custom `GTK_THEME`)

### The Problem

Users typically modify these permissions via **Flatseal** or `flatpak override`. These changes are stored in `~/.local/share/flatpak/overrides/<app-id>` (user scope) or `/var/lib/flatpak/overrides/<app-id>` (system scope).

Currently, `bkt` captures which Flatpak apps are installed but **not their permission overrides**:

```bash
bkt flatpak capture
# Captures: org.mozilla.firefox installed from flathub
# Does NOT capture: firefox has access to ~/Downloads
```

When the image is rebuilt, apps are reinstalled but their permission customizations are lost. Users must:

1. Remember which apps had custom permissions
2. Manually reconfigure each app in Flatseal
3. Repeat this after every image rebuild

### The Solution

Extend `bkt flatpak` to capture and restore override files:

```bash
# Capture all overrides to manifest
bkt flatpak capture --include-overrides

# View current overrides for an app
bkt flatpak override show org.mozilla.firefox

# Sync applies overrides alongside app installation
bkt flatpak sync
# Installs org.mozilla.firefox
# Applies stored overrides for org.mozilla.firefox
```

The schema already includes an `overrides` field—this RFC defines how to populate and apply it.

## Guide-level Explanation

### Override File Format

Flatpak overrides use an INI-style format with sections for different permission categories:

```ini
[Context]
filesystems=~/Documents:rw;~/Downloads:ro;!~/Private
devices=all
shared=network;ipc
sockets=wayland;x11;pulseaudio

[Session Bus Policy]
org.freedesktop.secrets=talk
org.gnome.Shell.Screenshot=own

[System Bus Policy]
org.freedesktop.UPower=talk

[Environment]
GTK_THEME=Adwaita-dark
MOZ_ENABLE_WAYLAND=1
```

#### Key Sections

| Section                | Purpose                  | Example Values                                |
| ---------------------- | ------------------------ | --------------------------------------------- |
| `[Context]`            | Core sandbox permissions | `filesystems`, `devices`, `shared`, `sockets` |
| `[Session Bus Policy]` | D-Bus session bus access | `org.freedesktop.secrets=talk`                |
| `[System Bus Policy]`  | D-Bus system bus access  | `org.freedesktop.UPower=talk`                 |
| `[Environment]`        | Environment variables    | `GTK_THEME=Adwaita-dark`                      |

#### Permission Syntax

Filesystems use special suffixes:

- `:rw` — read-write access (default)
- `:ro` — read-only access
- `:create` — create if missing, then read-write
- `!path` — explicitly deny access (negation)

### Manifest Representation

Overrides are stored in the `overrides` field as an array of strings:

```json
{
  "id": "org.mozilla.firefox",
  "remote": "flathub",
  "scope": "user",
  "overrides": [
    "--filesystem=~/Downloads:ro",
    "--filesystem=~/Documents:rw",
    "--device=all",
    "--env=MOZ_ENABLE_WAYLAND=1",
    "--talk-name=org.freedesktop.secrets"
  ]
}
```

This representation uses the `flatpak override` CLI flag syntax, which:

- Is well-documented and familiar
- Maps directly to `flatpak override` commands for application
- Avoids inventing a new format

### Capturing Overrides

#### From Files

Read override files directly from disk:

- User: `~/.local/share/flatpak/overrides/<app-id>`
- System: `/var/lib/flatpak/overrides/<app-id>`

#### From CLI

The `flatpak override --show` command outputs current overrides:

```bash
$ flatpak override --user --show org.mozilla.firefox
[Context]
filesystems=~/Downloads:ro;~/Documents

[Environment]
MOZ_ENABLE_WAYLAND=1
```

**Capture behavior:**

```bash
# Capture installs AND overrides
bkt flatpak capture --apply

# Result: manifest includes override data
```

When capturing, `bkt` will:

1. Parse the INI-style override file
2. Convert each permission to CLI flag format
3. Store in the `overrides` array

### Applying Overrides

During `bkt flatpak sync`, overrides are applied after installation:

```bash
bkt flatpak sync
# For each app:
#   1. Install via `flatpak install`
#   2. Apply overrides via `flatpak override --user <flags>`
```

Example:

```bash
flatpak override --user org.mozilla.firefox \
  --filesystem=~/Downloads:ro \
  --filesystem=~/Documents:rw \
  --device=all \
  --env=MOZ_ENABLE_WAYLAND=1 \
  --talk-name=org.freedesktop.secrets
```

### New Commands

#### Show Overrides

```bash
bkt flatpak override show <app-id>
```

Displays current overrides for an app in a readable format:

```
Overrides for org.mozilla.firefox:

Filesystems:
  ~/Downloads (read-only)
  ~/Documents (read-write)

Devices:
  all

Environment:
  MOZ_ENABLE_WAYLAND=1

Session Bus:
  org.freedesktop.secrets (talk)
```

With `--json`:

```json
{
  "app_id": "org.mozilla.firefox",
  "context": {
    "filesystems": ["~/Downloads:ro", "~/Documents:rw"],
    "devices": ["all"]
  },
  "environment": {
    "MOZ_ENABLE_WAYLAND": "1"
  },
  "session_bus_policy": {
    "org.freedesktop.secrets": "talk"
  }
}
```

#### Edit Overrides (Future)

```bash
# Add an override (punned command)
bkt flatpak override add org.mozilla.firefox --filesystem=~/Music:ro

# Remove an override
bkt flatpak override remove org.mozilla.firefox --filesystem=~/Music
```

These commands would:

1. Apply immediately via `flatpak override`
2. Update the manifest
3. Open a PR (unless `--local`)

This is deferred to a future RFC to keep scope manageable.

### System vs User Overrides

Flatpak supports both scopes:

| Scope      | Location                            | Applied to        |
| ---------- | ----------------------------------- | ----------------- |
| `--user`   | `~/.local/share/flatpak/overrides/` | Current user only |
| `--system` | `/var/lib/flatpak/overrides/`       | All users         |

**Design decision**: The `overrides` field in the manifest applies to the scope of the app installation. If an app is installed with `"scope": "user"`, overrides are applied with `--user`. If `"scope": "system"`, overrides are applied with `--system`.

**Implication**: System-scope overrides require elevated privileges during `bkt flatpak sync`.

### Priority and Merging

Flatpak override priority (highest to lowest):

1. User overrides (`~/.local/share/flatpak/overrides/`)
2. System overrides (`/var/lib/flatpak/overrides/`)
3. App metadata (defined by the app developer)

`bkt` captures and stores the **effective** overrides at the app's installation scope. It does not attempt to manage both user and system overrides for the same app.

## Reference-level Explanation

### Override Parser

Add a parser module that can:

1. **Read INI format** from override files
2. **Convert to CLI flags** for manifest storage
3. **Convert from CLI flags** to INI format for writing

```rust
pub struct FlatpakOverrides {
    pub filesystems: Vec<String>,
    pub devices: Vec<String>,
    pub shared: Vec<String>,
    pub sockets: Vec<String>,
    pub environment: HashMap<String, String>,
    pub session_bus_policy: HashMap<String, String>,
    pub system_bus_policy: HashMap<String, String>,
}

impl FlatpakOverrides {
    /// Parse from INI file content
    pub fn from_ini(content: &str) -> Result<Self>;

    /// Parse from flatpak override --show output
    pub fn from_show_output(output: &str) -> Result<Self>;

    /// Convert to CLI flag format for manifest storage
    pub fn to_cli_flags(&self) -> Vec<String>;

    /// Convert to INI format for writing override files
    pub fn to_ini(&self) -> String;
}
```

### Capture Integration

Extend `FlatpakAction::Capture` to include overrides:

```rust
FlatpakAction::Capture {
    dry_run: bool,
    apply: bool,
    #[arg(long, default_value = "true")]
    include_overrides: bool,
}
```

Capture workflow:

1. List installed flatpaks
2. For each app, check for override file at scope-appropriate location
3. Parse overrides and convert to CLI flags
4. Store in `FlatpakApp.overrides`

### Sync Integration

Extend `FlatpakAction::Sync` to apply overrides:

```rust
fn sync_flatpak(app: &FlatpakApp) -> Result<()> {
    // Install the app
    install_flatpak(app)?;

    // Apply overrides if present
    if let Some(overrides) = &app.overrides {
        apply_overrides(&app.id, app.scope, overrides)?;
    }

    Ok(())
}

fn apply_overrides(app_id: &str, scope: FlatpakScope, overrides: &[String]) -> Result<()> {
    let scope_flag = match scope {
        FlatpakScope::System => "--system",
        FlatpakScope::User => "--user",
    };

    let mut args = vec!["override", scope_flag, app_id];
    args.extend(overrides.iter().map(|s| s.as_str()));

    Command::new("flatpak")
        .args(&args)
        .status()
        .context("Failed to apply flatpak overrides")?;

    Ok(())
}
```

### File Locations

```rust
fn override_file_path(app_id: &str, scope: FlatpakScope) -> PathBuf {
    match scope {
        FlatpakScope::User => {
            dirs::data_local_dir()
                .unwrap_or_else(|| PathBuf::from("~/.local/share"))
                .join("flatpak/overrides")
                .join(app_id)
        }
        FlatpakScope::System => {
            PathBuf::from("/var/lib/flatpak/overrides").join(app_id)
        }
    }
}
```

### CLI Flag Mapping

| INI Section            | INI Key       | CLI Flag                                                 |
| ---------------------- | ------------- | -------------------------------------------------------- |
| `[Context]`            | `filesystems` | `--filesystem=<value>`                                   |
| `[Context]`            | `devices`     | `--device=<value>`                                       |
| `[Context]`            | `shared`      | `--share=<value>`                                        |
| `[Context]`            | `sockets`     | `--socket=<value>`                                       |
| `[Environment]`        | `KEY=value`   | `--env=KEY=value`                                        |
| `[Session Bus Policy]` | `name=policy` | `--talk-name=<name>` / `--own-name=<name>`               |
| `[System Bus Policy]`  | `name=policy` | `--system-talk-name=<name>` / `--system-own-name=<name>` |

Note: Session/System bus policy values map to:

- `talk` → `--talk-name` / `--system-talk-name`
- `own` → `--own-name` / `--system-own-name`
- `see` → `--see-name` / `--system-see-name`
- `none` → `--no-talk-name` / `--system-no-talk-name`

### Negation Handling

Override files support negation with `!` prefix:

```ini
[Context]
filesystems=!home
```

This translates to `--nofilesystem=home` in CLI format.

## Drawbacks

### Complexity

Parsing INI files and mapping bidirectionally between formats adds complexity. Mitigation: Use an existing INI parser crate.

### Privilege Escalation

System-scope overrides require root/sudo. This breaks the current model where `bkt flatpak sync` runs unprivileged.

**Options:**

1. Require `--system-overrides` flag to explicitly opt in
2. Warn and skip system overrides when running unprivileged
3. Use polkit for privilege escalation

Recommendation: Option 2 (warn and skip) for initial implementation.

### Sync vs Apply Semantics

If an app already has overrides on the local system, `bkt flatpak sync` will overwrite them with the manifest's overrides. This could be surprising.

**Mitigation:** Add a `--preserve-local-overrides` flag that skips override application for apps that already have local overrides.

## Rationale and Alternatives

### Alternative: Store INI Format Directly

Store the raw INI content instead of CLI flags:

```json
{
  "id": "org.mozilla.firefox",
  "overrides_ini": "[Context]\nfilesystems=~/Downloads:ro"
}
```

**Rejected because:**

- Multi-line strings are awkward in JSON
- CLI flags are more readable and diff-friendly
- CLI flags map directly to application commands

### Alternative: Separate Override Manifest

Create `manifests/flatpak-overrides.json` with override-specific structure:

```json
{
  "org.mozilla.firefox": {
    "filesystems": ["~/Downloads:ro"],
    "devices": ["all"]
  }
}
```

**Rejected because:**

- Overrides are inherently per-app; keeping them with the app is more cohesive
- The schema already has an `overrides` field

### Alternative: Only CLI Capture

Only support `flatpak override --show` output, not direct file parsing.

**Rejected because:**

- File parsing is more reliable for edge cases
- Some overrides may not appear correctly in `--show` output

## Prior Art

- **Flatseal**: GUI for managing Flatpak permissions; uses the same override files
- **Flatpak documentation**: Defines the override format and precedence rules
- **Nix Flatpak**: Some Nix modules capture Flatpak overrides declaratively

## Unresolved Questions

### Q1: Should `bkt flatpak add` Accept Inline Overrides?

```bash
bkt flatpak add org.mozilla.firefox --filesystem=~/Downloads:ro
```

**Resolution**: No. This conflates installation with permission configuration. Following command-punning principles, `bkt flatpak add` mirrors `flatpak install`, which doesn't accept permission flags. For permissions:

- Use Flatseal to configure, then `bkt capture`
- Or use `bkt flatpak override add <app> --filesystem=...` (future command)

### Q2: How to Handle Override Conflicts During Sync?

If the manifest says `--filesystem=~/Downloads:ro` but the local system has `--filesystem=~/Downloads:rw`, what should `sync` do?

**Resolution**: Warn and prompt before overwriting local state. Sync should detect when it would destroy local override customizations and ask:

```
⚠️ org.mozilla.firefox has local override changes not in manifest:
   Local:    --filesystem=~/Downloads:rw
   Manifest: --filesystem=~/Downloads:ro

[c] Capture local to manifest first
[o] Overwrite with manifest (lose local changes)  
[s] Skip this app
> 
```

This prevents accidental data loss. Use `--force` to skip prompts and always apply manifest.

### Q3: Should We Support Per-Override Granularity in PRs?

Currently, PRs are per-app. Should adding a single override create a PR that only includes that change?

**Resolution**: Yes, `bkt flatpak override add` (future) should work like `bkt flatpak add`.

## Future Possibilities

- **Override Diffing**: `bkt flatpak override diff` to show differences between local and manifest
- **Override Presets**: Common override bundles (e.g., "full filesystem access")
- **Flatseal Integration**: Import/export integration with Flatseal
- **Override Validation**: Warn about insecure or excessive permissions

## Implementation Checklist

### Phase 1: Core Parser

- [ ] INI parser for override files
- [ ] CLI flag serializer/deserializer
- [ ] Unit tests for format conversions

### Phase 2: Capture

- [ ] Add `--include-overrides` to `bkt flatpak capture`
- [ ] Read override files for each captured app
- [ ] Store in `FlatpakApp.overrides`

### Phase 3: Sync

- [ ] Apply overrides after app installation
- [ ] Handle missing overrides gracefully
- [ ] Add `--preserve-local-overrides` flag

### Phase 4: Show Command

- [ ] Implement `bkt flatpak override show <app-id>`
- [ ] Human-readable and `--json` output formats

### Phase 5: Polish

- [ ] System scope support with privilege handling
- [ ] Warning for override conflicts
- [ ] Documentation and examples
