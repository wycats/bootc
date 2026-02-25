# RFC 0014: Extension State Management

- Feature Name: `extension_state_management`
- Start Date: 2026-01-19
- RFC PR: (leave this empty until PR is opened)
- Tracking Issue: (leave this empty)

> **⚠️ Absorbed by [RFC-0052](../0052-manifest-lifecycle.md).**
> Extension enable/disable state management is now documented as part of the
> unified manifest lifecycle in RFC-0052. The system+user manifest merge
> described in this RFC is eliminated in the new design.

## Summary

Extend `bkt` extension tracking to properly capture and sync the **enabled/disabled state** of GNOME extensions, not just their presence in the manifest. This ensures that when a user disables an extension via Extension Manager, that state is preserved across `bkt apply` operations.

## Motivation

### Current Behavior (Problematic)

Today, `bkt extension capture` only tracks whether an extension is **installed**. It doesn't fully track the enabled/disabled state in a way that survives sync operations:

```bash
# User installs and enables Dash to Dock
bkt extension add dash-to-dock@micxgx.gmail.com
# Manifest: ["dash-to-dock@micxgx.gmail.com"]

# Later, user disables it via Extension Manager (GUI)
# Extension is still installed, just disabled

# User runs sync to apply their config
bkt extension sync
# ❌ WRONG: Extension gets re-enabled because manifest says it should exist
```

The root cause: the manifest stores extension UUIDs as strings (implying `enabled: true`), and `sync` enables all manifest extensions regardless of user intent.

### Desired Behavior

```bash
# User disables extension via Extension Manager
# User runs capture to update manifest
bkt extension capture --apply
# Manifest now contains: { "id": "dash-to-dock@micxgx.gmail.com", "enabled": false }

# On next sync
bkt extension sync
# ✓ Extension remains disabled (matches manifest state)
```

### Why This Matters

1. **Respects user intent**: Disabling an extension is a deliberate choice
2. **Prevents surprise re-enabling**: Users expect disabled extensions to stay disabled
3. **Enables "installed but dormant" workflows**: Keep extensions available for occasional use without having them always active

## Guide-level Explanation

### Querying Extension State

GNOME Shell provides built-in tools to query extension state:

```bash
# List all enabled extensions
gnome-extensions list --enabled

# List all installed extensions (enabled + disabled)
gnome-extensions list

# Get detailed info about a specific extension
gnome-extensions info dash-to-dock@micxgx.gmail.com
# Output includes "State: ENABLED" or "State: DISABLED"
```

### Manifest Representation

The schema already supports two formats for extension entries:

```json
{
  "extensions": [
    "appindicatorsupport@rgcjonas.gmail.com",

    { "id": "dash-to-dock@micxgx.gmail.com", "enabled": true },

    { "id": "blur-my-shell@aunetx", "enabled": false }
  ]
}
```

**Interpretation:**

- Plain string UUID → `enabled: true` (legacy format, still supported)
- Object with `enabled: true` → extension should be enabled
- Object with `enabled: false` → extension should be installed but disabled

### Updated Command Behaviors

#### `bkt extension capture`

Capture now detects enabled/disabled state:

```bash
$ bkt extension capture
Extension Capture: 3 to add, 2 already in manifest
  ➤ Capture extension:appindicatorsupport@rgcjonas.gmail.com
  ➤ Capture extension:dash-to-dock@micxgx.gmail.com (disabled)
  ➤ Capture extension:blur-my-shell@aunetx

Use --apply to execute this plan.
```

Extensions that are installed but disabled are captured with `enabled: false`.

#### `bkt extension sync`

Sync now respects enabled/disabled state:

```bash
$ bkt extension sync
Extension Sync: 1 to enable, 1 to disable, 5 checked
  ➤ Enable extension:appindicatorsupport@rgcjonas.gmail.com
  ➤ Disable extension:dash-to-dock@micxgx.gmail.com
```

**Key behaviors:**

- Extensions with `enabled: true` (or plain strings) → enable if disabled
- Extensions with `enabled: false` → disable if enabled
- Extensions not in manifest → ignored (no action)

#### `bkt extension enable/disable`

These commands now update both the system state AND the manifest:

```bash
# Enables extension AND updates manifest to enabled: true
$ bkt extension enable dash-to-dock@micxgx.gmail.com
✓ Enabled 'dash-to-dock@micxgx.gmail.com' in manifest
✓ Enabled dash-to-dock@micxgx.gmail.com

# Disables extension AND updates manifest to enabled: false
$ bkt extension disable blur-my-shell@aunetx
✓ Disabled 'blur-my-shell@aunetx' in manifest
✓ Disabled blur-my-shell@aunetx
```

## Reference-level Explanation

### Implementation Details

#### Querying Enabled State

The existing `is_enabled()` function parses `gnome-extensions info` output:

```rust
fn is_enabled(uuid: &str) -> bool {
    Command::new("gnome-extensions")
        .args(["info", uuid])
        .output()
        .map(|o| {
            o.status.success() &&
            String::from_utf8_lossy(&o.stdout).contains("State: ENABLED")
        })
        .unwrap_or(false)
}
```

For bulk operations, `get_enabled_extensions()` is more efficient:

```rust
fn get_enabled_extensions() -> Vec<String> {
    Command::new("gnome-extensions")
        .args(["list", "--enabled"])
        .output()
        // ... parse line-by-line
}
```

#### Capture Planning

The `ExtensionCaptureCommand::plan()` method:

1. Gets all installed extensions via `gnome-extensions list`
2. Gets enabled extensions via `gnome-extensions list --enabled`
3. For each installed extension:
   - If not in manifest → add with correct enabled state
   - If in manifest but state differs → update with correct state
   - If in manifest and state matches → skip

```rust
impl Plannable for ExtensionCaptureCommand {
    fn plan(&self, _ctx: &PlanContext) -> Result<Self::Plan> {
        let installed = get_installed_extensions_list();
        let enabled: HashSet<_> = get_enabled_extensions().into_iter().collect();
        let merged = GnomeExtensionsManifest::merged(&system, &user);

        let mut to_capture = Vec::new();

        for uuid in installed {
            let is_enabled_physically = enabled.contains(&uuid);

            if let Some(existing) = merged.get(&uuid) {
                // State matches → nothing to do
                if existing.enabled() == is_enabled_physically {
                    already_in_manifest += 1;
                    continue;
                }
                // State differs → needs update
            }

            to_capture.push(ExtensionToCapture {
                uuid,
                enabled: is_enabled_physically,
            });
        }

        Ok(ExtensionCapturePlan { to_capture, already_in_manifest })
    }
}
```

#### Sync Planning

The `ExtensionSyncCommand::plan()` method:

1. Loads merged manifest (system + user)
2. For each manifest entry:
   - If `enabled: true` and extension is disabled → queue enable
   - If `enabled: false` and extension is enabled → queue disable
   - If extension not installed → skip with warning

```rust
for item in merged.extensions {
    let uuid = item.id().to_string();
    let should_be_enabled = item.enabled();

    if is_enabled(&uuid) {
        if !should_be_enabled {
            to_disable.push(uuid);
        }
    } else if should_be_enabled {
        if is_installed(&uuid) {
            to_enable.push(ExtensionToSync { uuid, state: Disabled });
        } else {
            to_enable.push(ExtensionToSync { uuid, state: NotInstalled });
        }
    }
}
```

### Manifest Storage Format

Captured extensions with `enabled: false` are stored as objects:

```json
{
  "extensions": [
    "always-enabled@example.com",
    { "id": "sometimes-disabled@example.com", "enabled": false }
  ]
}
```

When state changes back to enabled, we preserve the object format:

```json
{ "id": "sometimes-disabled@example.com", "enabled": true }
```

This is semantically equivalent to the string format but explicitly shows state tracking.

## Migration

### Existing Manifests

Existing manifests with string-only UUIDs continue to work unchanged:

```json
{ "extensions": ["ext-a@example.com", "ext-b@example.com"] }
```

Interpretation: all listed extensions should be enabled.

### Upgrade Path

1. **No breaking changes**: String format remains valid indefinitely
2. **Gradual migration**: Running `bkt extension capture --apply` converts entries to object format only when state differs
3. **User override**: `bkt extension disable <uuid>` converts any string entry to object format with `enabled: false`

### Automatic Format Upgrade

We do NOT automatically convert all strings to objects. The object format is used only when:

- Extension is captured as disabled
- User explicitly runs `bkt extension disable`
- State tracking is needed to override system manifest

This keeps manifests clean and human-readable.

## Edge Cases

### Extension Installed But Not in Manifest

**Scenario**: User installs an extension via Extension Manager that isn't in the manifest.

**Behavior**:

- `bkt extension sync` → ignores it (no action)
- `bkt extension capture` → adds it to manifest with current state
- `bkt extension list` → shows it as "not tracked" (future enhancement)

**Rationale**: Extensions outside the manifest are user experiments. We don't touch them during sync.

### Extension in Manifest But Not Installed

**Scenario**: Manifest contains an extension that isn't installed locally.

**Behavior**:

- `bkt extension sync` → skips with informational message
- `bkt extension list` → shows "✗ not installed"

**Rationale**: `bkt` doesn't auto-install extensions (that requires user interaction with Extension Manager or extensions.gnome.org).

### System Manifest vs User Manifest Conflicts

**Scenario**: System manifest has `ext@example.com` (enabled), user wants it disabled.

**Behavior**:

- User runs `bkt extension disable ext@example.com`
- User manifest gains: `{ "id": "ext@example.com", "enabled": false }`
- Merged manifest sees the user override → extension stays disabled

**Implementation**: User manifest entries override system manifest entries when merging.

### Extension Removed via Extension Manager

**Scenario**: User uninstalls extension via Extension Manager GUI.

**Behavior**:

- `bkt extension sync` → extension is in manifest but not installed; skipped with warning
- `bkt extension capture` → could add logic to detect "in manifest but not installed" as needing removal

**Future consideration**: Add `bkt extension capture --prune` to remove uninstalled extensions from manifest.

## Drawbacks

1. **Increased manifest complexity**: Object format is more verbose than strings
2. **Two formats to support**: Code must handle both string and object entries
3. **Sync becomes bidirectional concept**: Users must understand capture vs sync

## Rationale and Alternatives

### Why Not Auto-Capture on Every Sync?

We could automatically run capture before sync to detect state changes. Rejected because:

- Sync should be idempotent and predictable
- Mixing capture into sync makes the operation non-obvious
- Users should explicitly choose when to update their manifest

### Why Not Remove Disabled Extensions from Manifest?

We could interpret "disabled" as "should be removed from manifest". Rejected because:

- Users may want to keep track of extensions they toggle frequently
- Removal is destructive; disabling is reversible
- Separate semantics: `remove` = untrack, `disable` = keep but don't enable

### Why Object Format Instead of Separate Lists?

Alternative: maintain separate `enabled` and `disabled` arrays:

```json
{
  "enabled": ["ext-a@example.com"],
  "disabled": ["ext-b@example.com"]
}
```

Rejected because:

- Requires schema migration
- Harder to see full extension list at a glance
- Current `anyOf` approach is already implemented and works well

## Resolved Questions

1. **Should `bkt extension add` default to enabled or match current state?**

   **Resolution**: Default to enabled (string format). Following command-punning principles, `bkt extension add` is for adding new extensions to track, and new extensions are typically added because you want to use them. If you want to capture the current state of an already-installed extension, use `bkt capture`.

2. **Should we support `--enabled-only` flag for capture?**

   **Resolution**: No. The purpose of capture is to capture the current state accurately. Capturing "installed but disabled" is valuable information—it means "I have this extension available but choose not to use it." Omitting disabled extensions would lose that intent.

3. **What about per-extension settings (gsettings under the extension's schema)?**
   - Out of scope for this RFC
   - Could be a future enhancement to capture extension configuration

## Future Possibilities

1. **`bkt extension capture --prune`**: Remove manifest entries for uninstalled extensions
2. **`bkt extension status`**: Quick overview comparing manifest to reality
3. **Extension settings capture**: Store per-extension gsettings/dconf values
4. **Auto-install support**: Integrate with `gnome-extensions install` for extensions.gnome.org
