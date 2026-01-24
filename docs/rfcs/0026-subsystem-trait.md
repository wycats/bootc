# RFC-0026: Subsystem Trait

- **Status**: Proposed
- **Created**: 2026-01-24

## Summary

Introduce a formal `Subsystem` trait that unifies how all subsystems (extensions, flatpak, gsettings, distrobox, etc.) handle manifest loading, capture, and sync operations. This eliminates duplicated logic between Rust (`bkt`) and Bash (`bootc-bootstrap`), closing a class of bugs where different code paths handle manifest merging differently.

## Motivation

### The Bug That Exposed The Problem

We recently fixed a bug where:

1. The Rust `bkt extension` commands correctly merged system + user manifests using `GnomeExtensionsManifest::merged(system, user)`
2. The bash `bootc-bootstrap` script was ONLY reading the system manifest, ignoring user overrides at `~/.config/bootc/gnome-extensions.json`
3. **Result**: Extensions the user disabled kept getting re-enabled on every boot

The fix required duplicating the merge logic in Bash using `jq`:

```bash
# If user overrides exist, merge them (user takes precedence)
if [[ -f "$user_file" ]]; then
  jq -s '
    def merge_extensions:
      (.[0].extensions // []) as $system |
      (.[1].extensions // []) as $user |
      # Convert to objects keyed by id for merging
      ($system | map(if type == "string" then {id: ., enabled: true} else . end) | INDEX(.id)) as $sys_map |
      ($user | map(if type == "string" then {id: ., enabled: true} else . end) | INDEX(.id)) as $usr_map |
      # Merge with user taking precedence
      ($sys_map * $usr_map) | to_entries | map(.value);
    {extensions: merge_extensions}
  ' "$file" "$user_file" > "$merged_file"
fi
```

This is fragile: the Rust and Bash implementations can (and did) diverge.

### The Root Cause

The root cause is **architectural debt**:

1. **Manifest loading is ad-hoc** - Each subsystem implements its own `load_system()`, `load_user()`, `merged()` methods with slightly different patterns
2. **No single source of truth** - `bootc-bootstrap` reimplements manifest logic that `bkt` already has
3. **Subsystem enumeration is duplicated** - `CaptureSubsystem` and `Subsystem` enums in `capture.rs` and `apply.rs` list overlapping but different sets
4. **Capture/sync composition is boilerplate** - Both `CaptureCommand::plan()` and `ApplyCommand::plan()` have near-identical if/else chains

### Current State

Currently, subsystems are implied through scattered patterns:

```rust
// capture.rs - defines which subsystems can be captured
pub enum CaptureSubsystem {
    Extension, Distrobox, Flatpak, System, AppImage, Homebrew,
}

// apply.rs - defines which subsystems can be synced
pub enum Subsystem {
    Shim, Distrobox, Gsetting, Extension, Flatpak, AppImage,
}

// Each command composes plans via if/else chains:
impl Plannable for CaptureCommand {
    fn plan(&self, ctx: &PlanContext) -> Result<Self::Plan> {
        let mut composite = CompositePlan::new("Capture");
        if self.should_include(CaptureSubsystem::Extension) {
            composite.add(ExtensionCaptureCommand.plan(ctx)?);
        }
        if self.should_include(CaptureSubsystem::Distrobox) {
            composite.add(DistroboxCaptureCommand.plan(ctx)?);
        }
        // ... repeated for each subsystem
    }
}
```

This pattern violates DRY and makes it easy for `bootc-bootstrap` to have different behavior.

## Design

### The Subsystem Trait

```rust
/// A subsystem manages a category of declarative configuration.
///
/// Each subsystem knows how to:
/// - Load its manifest (with proper system + user merging)
/// - Capture current system state to a manifest
/// - Sync manifest state to the running system
pub trait Subsystem: Send + Sync {
    /// Human-readable name for display (e.g., "GNOME Extensions").
    fn name(&self) -> &'static str;

    /// Short identifier for CLI filtering (e.g., "extension").
    fn id(&self) -> &'static str;

    /// Load the merged manifest (system defaults + user overrides).
    ///
    /// This is THE canonical way to get the effective manifest.
    /// The merge semantics are defined once here, not scattered across code.
    fn load_manifest(&self, ctx: &SubsystemContext) -> Result<Box<dyn Manifest>>;

    /// Create a capture plan (system → manifest).
    ///
    /// Returns None if this subsystem doesn't support capture.
    fn capture(&self, ctx: &PlanContext) -> Result<Option<Box<dyn Plan>>>;

    /// Create a sync plan (manifest → system).
    ///
    /// Returns None if this subsystem doesn't support sync.
    fn sync(&self, ctx: &PlanContext) -> Result<Option<Box<dyn Plan>>>;
}

/// Context for subsystem manifest loading.
pub struct SubsystemContext {
    /// Repository root (where manifests/ lives).
    pub repo_root: PathBuf,
    /// User config directory (~/.config/bootc/).
    pub user_config_dir: PathBuf,
    /// System manifest directory (/usr/share/bootc-bootstrap/).
    pub system_manifest_dir: PathBuf,
}

/// Marker trait for typed manifest access.
pub trait Manifest: std::fmt::Debug + Send + Sync {
    /// Serialize to JSON for capture operations.
    fn to_json(&self) -> Result<String>;
}
```

### Subsystem Registry

A registry provides the single source of truth for all subsystems:

```rust
/// Registry of all known subsystems.
pub struct SubsystemRegistry {
    subsystems: Vec<Box<dyn Subsystem>>,
}

impl SubsystemRegistry {
    /// Create registry with all built-in subsystems.
    pub fn builtin() -> Self {
        Self {
            subsystems: vec![
                Box::new(ExtensionSubsystem),
                Box::new(FlatpakSubsystem),
                Box::new(DistroboxSubsystem),
                Box::new(GsettingSubsystem),
                Box::new(ShimSubsystem),
                Box::new(AppImageSubsystem),
                Box::new(HomebrewSubsystem),
                Box::new(SystemSubsystem),
            ],
        }
    }

    /// Get all subsystems.
    pub fn all(&self) -> &[Box<dyn Subsystem>] {
        &self.subsystems
    }

    /// Get subsystems by ID filter.
    pub fn filtered(&self, include: Option<&[&str]>, exclude: &[&str]) -> Vec<&dyn Subsystem> {
        self.subsystems
            .iter()
            .filter(|s| {
                if exclude.contains(&s.id()) {
                    return false;
                }
                match include {
                    Some(ids) => ids.contains(&s.id()),
                    None => true,
                }
            })
            .map(|s| s.as_ref())
            .collect()
    }

    /// Get subsystems that support capture.
    pub fn capturable(&self) -> Vec<&dyn Subsystem> {
        self.subsystems
            .iter()
            .filter(|s| {
                // Check if capture returns Some
                // In practice, this would be a separate trait method
                true
            })
            .map(|s| s.as_ref())
            .collect()
    }

    /// Get subsystems that support sync.
    pub fn syncable(&self) -> Vec<&dyn Subsystem> {
        self.subsystems
            .iter()
            .filter(|s| true)
            .map(|s| s.as_ref())
            .collect()
    }
}
```

### Example: Extension Subsystem

```rust
pub struct ExtensionSubsystem;

impl Subsystem for ExtensionSubsystem {
    fn name(&self) -> &'static str {
        "GNOME Extensions"
    }

    fn id(&self) -> &'static str {
        "extension"
    }

    fn load_manifest(&self, ctx: &SubsystemContext) -> Result<Box<dyn Manifest>> {
        // THE single source of truth for extension manifest loading
        let system = GnomeExtensionsManifest::load(
            &ctx.system_manifest_dir.join("gnome-extensions.json")
        )?;
        let user = GnomeExtensionsManifest::load(
            &ctx.user_config_dir.join("gnome-extensions.json")
        )?;
        Ok(Box::new(GnomeExtensionsManifest::merged(&system, &user)))
    }

    fn capture(&self, ctx: &PlanContext) -> Result<Option<Box<dyn Plan>>> {
        let plan = ExtensionCaptureCommand.plan(ctx)?;
        if plan.is_empty() {
            Ok(None)
        } else {
            Ok(Some(Box::new(plan)))
        }
    }

    fn sync(&self, ctx: &PlanContext) -> Result<Option<Box<dyn Plan>>> {
        let plan = ExtensionSyncCommand.plan(ctx)?;
        if plan.is_empty() {
            Ok(None)
        } else {
            Ok(Some(Box::new(plan)))
        }
    }
}

impl Manifest for GnomeExtensionsManifest {
    fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }
}
```

### Simplified Command Implementation

With the registry, commands become trivial:

```rust
impl Plannable for ApplyCommand {
    type Plan = CompositePlan;

    fn plan(&self, ctx: &PlanContext) -> Result<Self::Plan> {
        let registry = SubsystemRegistry::builtin();
        let mut composite = CompositePlan::new("Apply");

        for subsystem in registry.filtered(self.include.as_deref(), &self.exclude) {
            if let Some(plan) = subsystem.sync(ctx)? {
                composite.add_boxed(plan);
            }
        }

        Ok(composite)
    }
}

impl Plannable for CaptureCommand {
    type Plan = CompositePlan;

    fn plan(&self, ctx: &PlanContext) -> Result<Self::Plan> {
        let registry = SubsystemRegistry::builtin();
        let mut composite = CompositePlan::new("Capture");

        for subsystem in registry.filtered(self.include.as_deref(), &self.exclude) {
            if let Some(plan) = subsystem.capture(ctx)? {
                composite.add_boxed(plan);
            }
        }

        Ok(composite)
    }
}
```

### bootc-bootstrap Becomes Trivial

The key benefit: `bootc-bootstrap` can delegate to `bkt` entirely:

```bash
#!/usr/bin/env bash
set -euo pipefail

# Instead of reimplementing manifest logic in bash,
# just call the Rust tool that has the canonical implementation.

log() {
  printf "[bootc-bootstrap] %s\n" "$*" >&2
}

if ! command -v bkt >/dev/null 2>&1; then
  log "bkt not found; cannot apply manifests"
  exit 1
fi

# That's it. One command. All subsystems. Correct manifest merging.
exec bkt apply --confirm
```

Or if we want per-subsystem control and error handling:

```bash
#!/usr/bin/env bash
set -euo pipefail

log() { printf "[bootc-bootstrap] %s\n" "$*" >&2; }

apply_subsystem() {
  local name="$1"
  log "Syncing ${name}..."
  bkt "${name}" sync --apply || log "${name} sync failed (continuing)"
}

apply_subsystem extension
apply_subsystem flatpak
apply_subsystem distrobox
apply_subsystem gsetting
apply_subsystem shim
apply_subsystem appimage
```

## Integration with Existing Traits

### Relationship to Plannable/Plan

The `Subsystem` trait complements rather than replaces `Plannable`:

- **`Plannable`** - A command that can produce a plan (generic trait)
- **`Plan`** - An executable plan with describe/execute/is_empty
- **`Subsystem`** - A category of configuration with manifest + capture + sync

Subsystems USE `Plannable` internally:

```rust
fn sync(&self, ctx: &PlanContext) -> Result<Option<Box<dyn Plan>>> {
    // The sync command is still Plannable
    let plan = ExtensionSyncCommand.plan(ctx)?;
    Ok(Some(Box::new(plan)))
}
```

### DynPlan for Heterogeneous Composition

The existing `DynPlan` trait (from RFC-0008) enables the `Box<dyn Plan>` returns:

```rust
// Subsystem returns Box<dyn Plan>
fn sync(&self, ctx: &PlanContext) -> Result<Option<Box<dyn Plan>>>;

// CompositePlan accepts it
impl CompositePlan {
    pub fn add_boxed(&mut self, plan: Box<dyn Plan>) {
        if !plan.is_empty() {
            self.plans.push(plan);
        }
    }
}
```

## Migration Path

### Phase 1: Define Trait and Registry

1. Add `subsystem.rs` with `Subsystem` trait and `SubsystemRegistry`
2. Implement `Subsystem` for each existing subsystem
3. Each implementation wraps existing command types

### Phase 2: Migrate Commands

1. Replace `CaptureSubsystem` and `Subsystem` enums with registry
2. Simplify `CaptureCommand::plan()` and `ApplyCommand::plan()` to use registry iteration
3. Remove per-subsystem if/else chains

### Phase 3: Simplify bootc-bootstrap

1. Replace per-subsystem Bash functions with `bkt <subsystem> sync`
2. Or replace entire script with `bkt apply`
3. Remove duplicated manifest parsing logic

### Backward Compatibility

- CLI interface unchanged (`bkt apply`, `bkt extension sync`, etc.)
- Manifest formats unchanged
- `bootc-bootstrap` behavior unchanged (but implementation simplified)

## Alternatives Considered

### Alternative 1: Keep Separate Enums

Keep `CaptureSubsystem` and `Subsystem` as separate enums.

**Rejected because**: Leads to inconsistency (Extension in both, System only in Capture, Shim only in Apply). The enums diverge over time.

### Alternative 2: Just Fix bootc-bootstrap

Just fix the Bash script to properly merge manifests, without architectural changes.

**Rejected because**: This treats the symptom, not the disease. The next subsystem will have the same bug. The merge logic will continue to diverge.

### Alternative 3: Eliminate bootc-bootstrap Entirely

Replace `bootc-bootstrap.service` with a Rust binary.

**Considered but deferred**: This is a good eventual goal, but the Subsystem trait is needed regardless. We can migrate incrementally: first make `bootc-bootstrap` call `bkt`, then eventually replace the systemd unit.

### Alternative 4: Code Generation

Generate both Rust and Bash from a common definition.

**Rejected because**: Overengineered. The simpler solution is to have Bash call Rust.

## Future Work

### Subsystem Discovery

Eventually, subsystems could be discovered dynamically:

```rust
// Plugin-style subsystem loading
impl SubsystemRegistry {
    pub fn with_plugins(plugin_dir: &Path) -> Self {
        // Load .so/.dylib plugins that implement Subsystem
    }
}
```

### Subsystem Dependencies

Some subsystems might depend on others (e.g., Flatpak needs remotes before apps):

```rust
trait Subsystem {
    fn depends_on(&self) -> &[&'static str] { &[] }
}
```

### Per-Subsystem Configuration

Subsystems could expose configuration:

```rust
trait Subsystem {
    fn configure(&mut self, config: &SubsystemConfig) -> Result<()>;
}
```

## Appendix: Subsystem Inventory

| Subsystem | ID | Capture | Sync | Manifest |
|-----------|-----|---------|------|----------|
| GNOME Extensions | `extension` | ✅ | ✅ | `gnome-extensions.json` |
| Flatpak Apps | `flatpak` | ✅ | ✅ | `flatpak-apps.json` |
| Flatpak Remotes | `flatpak-remote` | ❌ | ✅ | `flatpak-remotes.json` |
| Distrobox | `distrobox` | ✅ | ✅ | `distrobox.json` |
| GSettings | `gsetting` | ✅* | ✅ | `gsettings.json` |
| Host Shims | `shim` | ❌ | ✅ | `host-shims.json` |
| AppImages | `appimage` | ✅ | ✅ | `appimage-apps.json` |
| Homebrew | `homebrew` | ✅ | ❌ | `homebrew.json` |
| System Packages | `system` | ✅ | ❌ | `system-packages.json` |

*GSettings capture requires a schema filter, so it's typically invoked explicitly rather than via `bkt capture`.
