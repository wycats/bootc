# RFC 0015: Flatpak Override Management

- Feature Name: `flatpak_override_management`
- Start Date: 2026-01-19
- RFC PR: (leave this empty until PR is opened)
- Tracking Issue: (leave this empty)

## Summary

Capture and apply Flatpak permission overrides (the kind configured through Flatseal) as part of the Flatpak manifest flow. Overrides are parsed from override files during `bkt flatpak capture` and applied during `bkt flatpak sync`. There are no `bkt flatpak override show/edit` subcommands yet.

## Motivation

Flatpak applications run in sandboxed environments with restricted permissions. Users frequently need to grant additional permissions:

- Filesystem access (e.g., `~/Documents` for a text editor)
- Device access (e.g., webcam for video conferencing)
- D-Bus access (e.g., screen sharing permissions)
- Environment variables (e.g., custom `GTK_THEME`)

### The Problem

Users typically modify these permissions via **Flatseal** or `flatpak override`. These changes are stored in `~/.local/share/flatpak/overrides/<app-id>` (user scope) or `/var/lib/flatpak/overrides/<app-id>` (system scope).

`bkt` now captures overrides during `bkt flatpak capture`, so permission customizations can be preserved alongside app installs. Without capture, overrides are still lost on rebuild and must be manually recreated.

### The Solution

`bkt flatpak capture` reads override files and stores them in the manifest; `bkt flatpak sync` applies those overrides after installation:

```bash
# Capture installs and overrides to the manifest
bkt flatpak capture --apply

# Sync installs apps and applies stored overrides
bkt flatpak sync
```

The schema already includes an `overrides` field, and the current implementation populates and applies it automatically.

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

The current implementation includes a parser module that can:

1. **Read INI format** from override files
2. **Convert to CLI flags** for manifest storage
3. **Convert from CLI flags** to INI format for writing (reserved for future use)

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

`FlatpakAction::Capture` reads override files by default:

```rust
FlatpakAction::Capture {
  dry_run: bool,
  apply: bool,
}
```

Capture workflow:

1. List installed flatpaks
2. For each app, check for override file at scope-appropriate location
3. Parse overrides and convert to CLI flags
4. Store in `FlatpakApp.overrides`

### Sync Integration

`FlatpakAction::Sync` applies overrides after installation:

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

## Gaps

- No `bkt flatpak override show/add/remove` commands exist yet.
- Override conflict detection and "preserve local" behavior are not implemented.

## Open Questions

(None)
