# RFC 0012: GearLever Integration for AppImage Management

- Feature Name: `gearlever_integration`
- Start Date: 2026-01-14
- RFC PR: (leave this empty until PR is opened)
- Tracking Issue: (leave this empty)

## Summary

Add a new subsystem to `bkt` for managing AppImages through GearLever, enabling declarative configuration of apps distributed as AppImages (e.g., OrcaSlicer nightly, Ferdium, Keylight Controller).

## Motivation

Some applications are not available on Flathub or have superior AppImage distributions:

- **OrcaSlicer nightly**: GitHub-released AppImage with latest features
- **Ferdium**: Multi-messenger app with portable AppImage builds
- **Myrient Downloader**: Niche tool only distributed as AppImage

Previously, we attempted to manage OrcaSlicer via a custom Flatpak remote (`orcaslicer-origin`), but this approach:

1. Requires maintaining a custom flatpak remote
2. Remote doesn't exist in practice, causing `bkt apply` failures
3. Updates require manual remote management

GearLever (`it.mijorus.gearlever`) solves this by:

- Managing AppImage downloads and updates automatically
- Tracking GitHub releases with configurable prerelease support
- Creating proper desktop entries with icons
- Providing a simple JSON manifest we can integrate with

### GearLever's Manifest Format

GearLever stores its state at `~/.var/app/it.mijorus.gearlever/config/apps.json`:

```json
{
  "T3JjYVNsaWNlcg==": {
    "b64name": "T3JjYVNsaWNlcg==",
    "update_url": "https://github.com/OrcaSlicer/OrcaSlicer/releases/download/*/OrcaSlicer_Linux_AppImage_Ubuntu2404_nightly.AppImage",
    "update_url_manager": "GithubUpdater",
    "update_manager_config": {
      "allow_prereleases": true,
      "repo_url": "https://github.com/OrcaSlicer/OrcaSlicer",
      "repo_filename": "OrcaSlicer_Linux_AppImage_Ubuntu2404_nightly.AppImage"
    },
    "name": "OrcaSlicer"
  }
}
```

Key observations:

- Keys are base64-encoded app names (e.g., "OrcaSlicer" → "T3JjYVNsaWNlcg==")
- `update_url_manager`: Currently only "GithubUpdater" observed
- `update_manager_config.allow_prereleases`: Controls nightly/prerelease access
- `repo_filename`: Glob pattern for matching release assets

## Guide-level Explanation

### Subsystem Naming: `appimage`

**Decision**: Use `appimage` as the subsystem name, not `gearlever`.

**Rationale**:

- **User intent**: Users think "I want an AppImage", not "I want to use GearLever"
- **Future-proofing**: If a better AppImage manager emerges, the command semantics remain valid
- **Consistency**: We say `bkt flatpak add`, not `bkt flathub add`

GearLever is the _implementation_, AppImage is the _concept_.

### Command Examples

```bash
# Add an AppImage from GitHub releases
bkt appimage add github:OrcaSlicer/OrcaSlicer \
  --asset "OrcaSlicer_Linux_AppImage_Ubuntu2404_nightly.AppImage" \
  --prereleases

# Add with explicit name
bkt appimage add github:ferdium/ferdium-app \
  --asset "Ferdium-linux-Portable-*-x86_64.AppImage" \
  --name Ferdium \
  --prereleases

# List configured AppImages
bkt appimage list

# Remove an AppImage
bkt appimage remove OrcaSlicer

# Sync: download any missing AppImages (triggers GearLever update)
bkt appimage sync
```

### Manifest: `manifests/appimage-apps.json`

Our manifest uses a simplified, human-friendly format. The filename is `appimage-apps.json` (not `gearlever-apps.json`) to enable future backend swapping without manifest migration:

```json
{
  "$schema": "https://wycats.github.io/bootc/appimage-apps.schema.json",
  "apps": [
    {
      "name": "OrcaSlicer",
      "repo": "OrcaSlicer/OrcaSlicer",
      "asset": "OrcaSlicer_Linux_AppImage_Ubuntu2404_nightly.AppImage",
      "prereleases": true
    },
    {
      "name": "Ferdium",
      "repo": "ferdium/ferdium-app",
      "asset": "Ferdium-linux-Portable-*-x86_64.AppImage",
      "prereleases": true
    },
    {
      "name": "Keylight Controller",
      "repo": "justinforlenza/keylight-control",
      "asset": "Keylight_Controller-x86_64.AppImage",
      "prereleases": false
    }
  ]
}
```

**Key differences from GearLever's format**:

- Human-readable keys (not base64)
- Simplified fields (`repo` instead of multiple URL fields)
- `bkt` reconstructs GearLever's full format on sync

### Capture Workflow

```bash
bkt appimage capture
```

Reads from GearLever's `apps.json` and updates `manifests/appimage-apps.json`:

1. Parse `~/.var/app/it.mijorus.gearlever/config/apps.json`
2. Decode base64 names
3. Extract GitHub repo from `repo_url`
4. Write simplified entries to manifest
5. Open PR to propagate to distribution

### Apply/Sync Workflow

```bash
bkt apply --only appimage
```

Writes to GearLever's `apps.json`:

1. Load `manifests/appimage-apps.json`
2. Load existing GearLever `apps.json`
3. Generate base64 keys and full GearLever format
4. **Prune by default**: Remove entries not in manifest (use `--keep` to preserve user-added apps)
5. Write config to `~/.var/app/it.mijorus.gearlever/config/apps.json`

### Background Updates

GearLever already handles automatic updates via XDG autostart:

```ini
# ~/.config/autostart/it.mijorus.gearlever.desktop
[Desktop Entry]
Type=Application
Exec=flatpak run --command=gearlever it.mijorus.gearlever --fetch-updates
```

This runs on login and checks for updates. Combined with `settings.json`:

```json
{ "fetch-updates-in-background": true }
```

**Decision**: `bkt apply` updates the config only. GearLever downloads:

- Automatically on next login (via autostart)
- Immediately if user runs: `flatpak run it.mijorus.gearlever --fetch-updates`

This is the right approach because:

1. GearLever already has robust update infrastructure
2. No need to duplicate download logic
3. Declarative config is sufficient for machine reproducibility

## Reference-level Explanation

### Manifest Types

```rust
/// An AppImage entry (our simplified format, backend-agnostic)
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AppImageApp {
    /// Human-readable app name
    pub name: String,
    /// GitHub repository (owner/repo format)
    pub repo: String,
    /// Asset filename pattern (glob supported)
    pub asset: String,
    /// Whether to include prereleases
    #[serde(default)]
    pub prereleases: bool,
    /// Whether this app is disabled (won't sync)
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub disabled: bool,
}

/// The appimage-apps.json manifest
#[derive(Debug, Clone, Serialize, Deserialize, Default, JsonSchema)]
pub struct AppImageAppsManifest {
    #[serde(rename = "$schema", skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    pub apps: Vec<AppImageApp>,
}

impl AppImageApp {
    /// Generate base64 key for GearLever's format
    pub fn b64_key(&self) -> String {
        base64::encode(&self.name)
    }

    /// Convert to GearLever's native format
    pub fn to_gearlever_entry(&self) -> GearLeverNativeEntry {
        GearLeverNativeEntry {
            b64name: self.b64_key(),
            name: self.name.clone(),
            update_url: format!(
                "https://github.com/{}/releases/download/*/{}",
                self.repo, self.asset
            ),
            update_url_manager: "GithubUpdater".to_string(),
            update_manager_config: GearLeverUpdateConfig {
                allow_prereleases: self.prereleases,
                repo_url: format!("https://github.com/{}", self.repo),
                repo_filename: self.asset.clone(),
            },
        }
    }
}
```

### GearLever Detection

```rust
impl AppImageAppsManifest {
    /// Path to GearLever's config directory
    pub fn gearlever_config_dir() -> Option<PathBuf> {
        let home = std::env::var("HOME").ok()?;
        let path = PathBuf::from(home)
            .join(".var/app/it.mijorus.gearlever/config");
        if path.exists() {
            Some(path)
        } else {
            None
        }
    }

    /// Check if GearLever is available
    pub fn is_gearlever_available() -> bool {
        Self::gearlever_config_dir().is_some()
    }
}
```

### Graceful Degradation

When GearLever is not installed:

```rust
pub fn ensure_gearlever_available() -> Result<()> {
    if !AppImageAppsManifest::is_gearlever_available() {
        anyhow::bail!(
            "GearLever not found. Install it first:\n\n  \
             flatpak install flathub it.mijorus.gearlever\n\n\
             Then run GearLever once to initialize its config directory."
        );
    }
    Ok(())
}
```

### Plannable Integration

```rust
impl Plannable for AppImageAppsManifest {
    type Item = AppImageApp;

    fn plan(&self, current: &GearLeverNativeManifest) -> Plan<Self::Item> {
        let mut plan = Plan::new();

        // Decode current GearLever state
        let current_by_name: HashMap<String, _> = current.apps
            .iter()
            .map(|(_, entry)| (entry.name.clone(), entry))
            .collect();

        for app in &self.apps {
            if let Some(existing) = current_by_name.get(&app.name) {
                // Check if config differs
                if app.needs_update(existing) {
                    plan.update(app.clone());
                }
            } else {
                plan.add(app.clone());
            }
        }

        // Removals: apps in current but not in manifest
        // (Only if we want strict declarative mode)

        plan
    }
}
```

### Filter Flags

```bash
# Apply only AppImage subsystem
bkt apply --only appimage

# Apply everything except AppImages
bkt apply --exclude appimage

# Status for AppImages
bkt status --only appimage
```

Implementation follows the existing filter pattern in `ExecutionContext`.

## Design Considerations

### 1. Base64 Key Scheme

**Question**: Should our manifest replicate GearLever's base64 keys?

**Decision**: No. Use human-readable `name` field.

**Rationale**:

- Base64 keys are an implementation detail of GearLever
- Our manifest is for humans to read and edit
- We generate base64 keys at sync time
- If GearLever changes its key scheme, we only need to update conversion logic

### 2. Conflict Resolution and Pruning

**Decision**: Prune by default, with `--keep` flag to preserve user-added apps.

**Rationale**:

- Declarative model: manifest is source of truth
- User-added apps are the exception, not the rule
- `--keep` flag available for cases where local experimentation is needed
- Consistent with other subsystems (flatpaks, extensions, etc.)

```rust
fn sync_to_gearlever(manifest: &AppImageAppsManifest, keep_unmanaged: bool) -> Result<()> {
    let mut native = GearLeverNativeManifest::load_current()?;

    // Collect manifest app names
    let manifest_names: HashSet<_> = manifest.apps.iter()
        .map(|a| a.name.clone()).collect();

    // Update/add manifest entries
    for app in &manifest.apps {
        native.upsert(app.to_gearlever_entry());
    }

    // Remove entries not in manifest (unless --keep)
    if !keep_unmanaged {
        native.retain(|entry| manifest_names.contains(&entry.name));
    }

    native.save()?;
    Ok(())
}
```

### 3. Manifest Location

**Decision**: Use `appimage-apps.json` (not `gearlever-apps.json`).

**Rationale**:

- Matches the `bkt appimage` command name
- Enables backend swapping without manifest migration
- If a better AppImage manager emerges, manifest stays the same
- Consistent with user intent: "I want AppImages", not "I use GearLever"

### 4. GearLever as Required Dependency

GearLever is a **required** dependency for the AppImage subsystem. It should be:

1. **In base-image-assumptions.json** or marked as non-removable in flatpak-apps.json
2. Installed at system scope (available to all users)
3. Protected from removal by `bkt` (error if user tries to remove it)

```json
// base-image-assumptions.json or flatpak-apps.json with "required": true
{
  "id": "it.mijorus.gearlever",
  "remote": "flathub",
  "scope": "system",
  "required": true // Cannot be removed via bkt
}
```

**Order of operations in `bkt apply`**:

1. Flatpak remotes (ensure Flathub exists)
2. Flatpak apps (install/verify GearLever)
3. AppImages (GearLever is now available)

**Graceful degradation**: If GearLever is missing and `appimage-apps.json` is empty, skip with info message. If manifest has entries but GearLever missing, error with install instructions.

## Open Questions

### Q1: Should we support non-GitHub sources?

GearLever theoretically supports direct URLs. Do we need:

```json
{
  "name": "SomeApp",
  "url": "https://example.com/app.AppImage"
}
```

**Decision**: No for v1. Focus on GitHub releases first. Add URL support later if needed.

### Q2: How to handle AppImage versioning/pinning?

GearLever auto-updates to latest. Should we support pinning?

**Decision**: No pinning in v1. GearLever's model is "latest matching release". If pinning is needed, consider it for v2.

### Q3: Asset pattern validation?

Should we validate that the `asset` pattern actually matches a GitHub release at add-time?

**Decision**: Yes, attempt to validate during `bkt appimage add`. Warn but don't fail - the release might not exist yet for nightly builds.

## CLI Commands

The `bkt appimage` subcommand provides full manifest management:

```bash
# Add an AppImage from GitHub releases
bkt appimage add github:OrcaSlicer/OrcaSlicer \
  --asset "OrcaSlicer_Linux_AppImage_Ubuntu2404_nightly.AppImage" \
  --prereleases

# Add with explicit name
bkt appimage add github:ferdium/ferdium-app \
  --asset "Ferdium-linux-Portable-*-x86_64.AppImage" \
  --name Ferdium \
  --prereleases

# Remove an AppImage from manifest
bkt appimage remove OrcaSlicer

# Disable an AppImage (keeps in manifest but won't sync)
bkt appimage disable OrcaSlicer

# Re-enable a disabled AppImage
bkt appimage enable OrcaSlicer

# List configured AppImages
bkt appimage list

# Capture from current GearLever state
bkt appimage capture

# Sync manifest to GearLever (prune by default)
bkt appimage sync

# Sync without removing user-added apps
bkt appimage sync --keep
```

### Manifest with Disabled Apps

```json
{
  "$schema": "https://wycats.github.io/bootc/appimage-apps.schema.json",
  "apps": [
    {
      "name": "OrcaSlicer",
      "repo": "OrcaSlicer/OrcaSlicer",
      "asset": "OrcaSlicer_Linux_AppImage_Ubuntu2404_nightly.AppImage",
      "prereleases": true
    },
    {
      "name": "Ferdium",
      "repo": "ferdium/ferdium-app",
      "asset": "Ferdium-linux-Portable-*-x86_64.AppImage",
      "prereleases": true,
      "disabled": true // Won't be synced to GearLever
    }
  ]
}
```

Disabled apps are useful for:

- Temporarily removing an app without losing the config
- Machine-specific exclusions (could be combined with profiles later)
- Documenting "I tried this but don't need it now"

## Implementation Plan

1. **Phase 1: Manifest and Types**

   - Add `AppImageApp`, `AppImageAppsManifest` types in `manifest/appimage.rs`
   - Add JSON schema for `appimage-apps.json`
   - Add `GearLeverNativeManifest` for parsing GearLever's format
   - Mark GearLever as required in base-image or flatpak manifest

2. **Phase 2: CLI Commands**

   - `bkt appimage add github:owner/repo --asset pattern [--prereleases] [--name Name]`
   - `bkt appimage remove <name>`
   - `bkt appimage disable <name>` / `bkt appimage enable <name>`
   - `bkt appimage list`

3. **Phase 3: Capture Command**

   - `bkt appimage capture` reads from GearLever
   - Converts to simplified format
   - Merges with existing manifest (preserving disabled state)

4. **Phase 4: Sync/Apply**
   - `bkt appimage sync` / `bkt apply --only appimage`
   - Prune by default, `--keep` to preserve user-added
   - Skip disabled apps
   - Graceful handling when GearLever missing

## Future Extensions

- **AppImageLauncher support**: Alternative backend
- **Version pinning**: Lock to specific release tags
- **Sandbox configuration**: AppImage sandbox permissions
- **Desktop file customization**: Categories, keywords, etc.

## References

- GearLever: https://github.com/mijorus/gearlever
- AppImage specification: https://appimage.org/
- RFC 0001: Command Punning Philosophy (command naming patterns)
- RFC 0006: Upstream Dependency Management (similar pinning concepts)
