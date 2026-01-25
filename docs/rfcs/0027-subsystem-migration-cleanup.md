# RFC-0027: Subsystem Trait Migration Cleanup

- **Status**: Implemented (Phases 1-5 complete)
- **Created**: 2026-01-24
- **Depends On**: RFC-0026

## Summary

Complete the Subsystem trait migration by removing vestigial enums from `capture.rs` and `apply.rs`, unifying manifest loading patterns in `status.rs` and `local.rs`, and connecting `ChangeDomain` and `DriftCategory` to the registry. This cleanup eliminates duplicated subsystem definitions and ensures all subsystem-aware code uses the canonical registry as the single source of truth.

## Motivation

RFC-0026 introduced the `Subsystem` trait and `SubsystemRegistry` to unify how subsystems handle manifest loading, capture, and sync operations. The migration was partially completed:

1. ✅ `Subsystem` trait and `SubsystemRegistry` implemented in `bkt/src/subsystem.rs`
2. ✅ `CaptureCommand` and `ApplyCommand` now iterate over the registry
3. ✅ `bootc-bootstrap` delegates to `bkt` instead of reimplementing manifest logic

However, vestiges of the old architecture remain:

- **Duplicated enums**: `CaptureSubsystem` and `Subsystem` enums still exist for CLI parsing
- **Ad-hoc manifest loading**: `status.rs` and `local.rs` have duplicated load/merge patterns
- **Parallel domain definitions**: `ChangeDomain` and `DriftCategory` duplicate subsystem IDs

This creates maintenance burden and risk of divergence. For example, adding a new subsystem requires updating:

- `SubsystemRegistry::builtin()`
- `CaptureSubsystem` enum (if capturable)
- `Subsystem` enum (if syncable)
- `ChangeDomain` enum (if used in local changes)
- `DriftCategory` enum (if drift-checkable)

The goal is **one place to add a new subsystem**: the registry.

## Implementation Plan

### Phase 1: Remove Vestigial Enums ✅

**Status**: Complete

Replace `CaptureSubsystem` and `Subsystem` enums with dynamic validation against the registry.

#### Current State

**File:** [bkt/src/commands/capture.rs](../../../bkt/src/commands/capture.rs#L14-L41)

```rust
/// The subsystems that can be captured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum CaptureSubsystem {
    Extension,
    Distrobox,
    Flatpak,
    System,
    AppImage,
    Homebrew,
}

impl std::fmt::Display for CaptureSubsystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CaptureSubsystem::Extension => write!(f, "extension"),
            // ... repeated for each variant
        }
    }
}
```

**File:** [bkt/src/commands/apply.rs](../../../bkt/src/commands/apply.rs#L14-L41)

```rust
/// The subsystems that can be synced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Subsystem {
    Shim,
    Distrobox,
    Gsetting,
    Extension,
    Flatpak,
    AppImage,
}
```

Note the sets differ: `CaptureSubsystem` has `System`/`Homebrew` but not `Shim`/`Gsetting`; `Subsystem` is the reverse.

#### Target State

Replace enums with `String` and validate against registry at parse time:

```rust
// In bkt/src/commands/capture.rs
use crate::subsystem::SubsystemRegistry;

#[derive(Debug, Args)]
pub struct CaptureArgs {
    /// Only capture specific subsystems (comma-separated)
    #[arg(long, short = 's', value_delimiter = ',', value_parser = parse_capturable_subsystem)]
    pub only: Option<Vec<String>>,

    /// Exclude specific subsystems from capture
    #[arg(long, short = 'x', value_delimiter = ',', value_parser = parse_capturable_subsystem)]
    pub exclude: Option<Vec<String>>,

    /// Apply the plan immediately
    #[arg(long)]
    pub apply: bool,
}

fn parse_capturable_subsystem(s: &str) -> Result<String, String> {
    let registry = SubsystemRegistry::builtin();
    let capturable_ids: Vec<&str> = registry.capturable().iter().map(|s| s.id()).collect();

    if capturable_ids.contains(&s) {
        Ok(s.to_string())
    } else {
        Err(format!(
            "Unknown subsystem '{}'. Valid options: {}",
            s,
            capturable_ids.join(", ")
        ))
    }
}

pub struct CaptureCommand {
    pub include: Option<Vec<String>>,
    pub exclude: Vec<String>,
}

impl CaptureCommand {
    fn should_include_id(&self, id: &str) -> bool {
        if self.exclude.iter().any(|e| e == id) {
            return false;
        }
        if let Some(ref include) = self.include {
            return include.iter().any(|i| i == id);
        }
        true
    }
}
```

Similarly for `apply.rs` with `parse_syncable_subsystem`.

#### Steps

1. **Add registry helper methods** to `bkt/src/subsystem.rs`:
   - Add `capturable_ids() -> Vec<&'static str>` method
   - Add `syncable_ids() -> Vec<&'static str>` method
   - Add `is_valid_capturable(id: &str) -> bool` method
   - Add `is_valid_syncable(id: &str) -> bool` method

2. **Update capture.rs** (lines 14-77):
   - Remove `CaptureSubsystem` enum and its `Display` impl
   - Add `parse_capturable_subsystem` value parser function
   - Change `CaptureArgs.only` and `CaptureArgs.exclude` from `Vec<CaptureSubsystem>` to `Vec<String>`
   - Update `CaptureCommand` to use `Vec<String>` instead of `Vec<CaptureSubsystem>`
   - Remove `should_include` method (keep only `should_include_id`)

3. **Update apply.rs** (lines 14-77):
   - Remove `Subsystem` enum and its `Display` impl
   - Add `parse_syncable_subsystem` value parser function
   - Change `ApplyArgs.only` and `ApplyArgs.exclude` from `Vec<Subsystem>` to `Vec<String>`
   - Update `ApplyCommand` to use `Vec<String>`
   - Remove `should_include` method (keep only `should_include_id`)

4. **Update tests** in both files:
   - Convert `CaptureSubsystem::Extension` to `"extension".to_string()` etc.
   - Update assertions to use string comparisons

#### Validation

- [ ] `cargo build` passes
- [ ] `cargo test` passes
- [ ] `bkt capture --only extension` still works (valid subsystem)
- [ ] `bkt capture --only invalid` shows helpful error with valid options
- [ ] `bkt apply --only shim` still works
- [ ] `bkt apply --only system` errors (system is capturable, not syncable)

---

### Phase 2: Unify status.rs Manifest Loading ✅

**Status**: Complete

Replace duplicated load/merge patterns with registry-based loading.

#### Current State

**File:** [bkt/src/commands/status.rs](../../../bkt/src/commands/status.rs)

Four duplicated patterns exist:

1. **Flatpak** (lines 298-301):

```rust
let system = FlatpakAppsManifest::load_system().unwrap_or_default();
let user = FlatpakAppsManifest::load_user().unwrap_or_default();
let merged = FlatpakAppsManifest::merged(&system, &user);
```

2. **Extensions** (lines 321-323):

```rust
let system = GnomeExtensionsManifest::load_system().unwrap_or_default();
let user = GnomeExtensionsManifest::load_user().unwrap_or_default();
let merged = GnomeExtensionsManifest::merged(&system, &user);
```

3. **GSettings** (lines 346-348):

```rust
let system = GSettingsManifest::load_system().unwrap_or_default();
let user = GSettingsManifest::load_user().unwrap_or_default();
let merged = GSettingsManifest::merged(&system, &user);
```

4. **Shims** (lines 402-404):

```rust
let system = ShimsManifest::load_system().unwrap_or_default();
let user = ShimsManifest::load_user().unwrap_or_default();
let merged = ShimsManifest::merged(&system, &user);
```

#### Target State

Use registry's `load_manifest()` with type downcasting:

```rust
use crate::subsystem::{SubsystemContext, SubsystemRegistry};

pub fn run(args: StatusArgs) -> Result<()> {
    let registry = SubsystemRegistry::builtin();
    let ctx = SubsystemContext::new();

    // Flatpak status
    let flatpak_status = {
        let manifest = registry.find("flatpak")
            .expect("flatpak subsystem not found")
            .load_manifest(&ctx)
            .unwrap_or_else(|_| Box::new(FlatpakAppsManifest::default()));

        // Downcast to concrete type for status calculation
        let manifest = manifest.as_any()
            .downcast_ref::<FlatpakAppsManifest>()
            .expect("flatpak manifest type mismatch");

        // ... existing status calculation using manifest.apps
    };
    // ... similar for other subsystems
}
```

This requires adding `as_any()` to the `Manifest` trait for downcasting.

#### Steps

1. **Extend Manifest trait** in `bkt/src/subsystem.rs`:

   ```rust
   pub trait Manifest: std::fmt::Debug + Send + Sync {
       fn to_json(&self) -> Result<String>;
       fn as_any(&self) -> &dyn std::any::Any;
   }
   ```

2. **Implement as_any** for each manifest type in subsystem.rs (add to each `impl Manifest for XxxManifest`):

   ```rust
   fn as_any(&self) -> &dyn std::any::Any {
       self
   }
   ```

3. **Add type aliases or helper functions** for cleaner downcasting:

   ```rust
   // In subsystem.rs
   impl SubsystemRegistry {
       pub fn load_flatpak_manifest(&self, ctx: &SubsystemContext) -> Result<FlatpakAppsManifest> {
           self.find("flatpak")
               .ok_or_else(|| anyhow::anyhow!("flatpak subsystem not found"))?
               .load_manifest(ctx)
               .and_then(|m| {
                   m.as_any()
                       .downcast_ref::<FlatpakAppsManifest>()
                       .cloned()
                       .ok_or_else(|| anyhow::anyhow!("manifest type mismatch"))
               })
       }
       // Similar for extension, gsetting, shim
   }
   ```

4. **Update status.rs** to use registry:
   - Add imports for `SubsystemContext`, `SubsystemRegistry`
   - Replace each load/merge block with registry call
   - Handle errors gracefully with `unwrap_or_default()`

#### Alternative: Status Trait Method

A cleaner approach is to add a `status()` method to the `Subsystem` trait:

```rust
pub trait Subsystem: Send + Sync {
    // ... existing methods ...

    /// Get status for this subsystem (for `bkt status` command).
    fn status(&self, ctx: &SubsystemContext) -> Result<Box<dyn SubsystemStatus>>;
}

pub trait SubsystemStatus: std::fmt::Debug + Send + Sync {
    fn total(&self) -> usize;
    fn synced(&self) -> usize;
    fn pending(&self) -> usize;
    fn untracked(&self) -> usize;
    fn to_json(&self) -> Result<serde_json::Value>;
}
```

This moves status calculation into each subsystem, making the code more cohesive.

#### Validation

- [ ] `cargo build` passes
- [ ] `cargo test` passes
- [ ] `bkt status` shows same output as before
- [ ] `bkt status --format json` produces valid JSON

---

### Phase 3: Unify local.rs Manifest Loading ✅

**Status**: Complete

The `local.rs` file loads manifests when applying changes to create PRs.

#### Current State

**File:** [bkt/src/commands/local.rs](../../../bkt/src/commands/local.rs)

Each `apply_*_changes` function loads its own manifest:

1. **apply_flatpak_changes** (line 363):

   ```rust
   let mut manifest = FlatpakAppsManifest::load(&manifest_path)?;
   ```

2. **apply_extension_changes** (line 431):

   ```rust
   let mut manifest = GnomeExtensionsManifest::load(&manifest_path)?;
   ```

3. **apply_gsetting_changes** (line 454):

   ```rust
   let mut manifest = GSettingsManifest::load(&manifest_path)?;
   ```

4. **apply_shim_changes** (line 497):

   ```rust
   let mut manifest = ShimsManifest::load(&manifest_path)?;
   ```

5. **apply_dnf_changes** (line 522):

   ```rust
   let mut manifest = SystemPackagesManifest::load(&manifest_path)?;
   ```

6. **apply_appimage_changes** (line ~545):
   ```rust
   let mut manifest = AppImageAppsManifest::load(&manifest_path)?;
   ```

#### Target State

These functions load from the **repo** manifests directory (not system/user merged), so they're slightly different from the status.rs case. However, they could still benefit from registry integration for:

- Consistent path resolution via `SubsystemContext::repo_manifest_path()`
- Future extensibility (e.g., validation)

```rust
fn apply_flatpak_changes(
    changes: &[&EphemeralChange],
    ctx: &SubsystemContext,
) -> Result<ManifestChange> {
    let manifest_path = ctx.repo_manifest_path("flatpak-apps.json");
    let mut manifest = FlatpakAppsManifest::load(&manifest_path)?;
    // ... existing logic
}
```

#### Steps

1. **Thread SubsystemContext through** `run_commit_workflow`:
   - Create `SubsystemContext::with_repo_root(repo_path)` early
   - Pass `&ctx` to each `apply_*_changes` function

2. **Update apply\_\* function signatures**:

   ```rust
   fn apply_flatpak_changes(
       changes: &[&EphemeralChange],
       ctx: &SubsystemContext,  // Replace manifests_dir
   ) -> Result<ManifestChange>
   ```

3. **Use ctx.repo_manifest_path()** instead of `manifests_dir.join()`:
   ```rust
   let manifest_path = ctx.repo_manifest_path("flatpak-apps.json");
   ```

#### Validation

- [ ] `cargo build` passes
- [ ] `cargo test` passes
- [ ] `bkt local commit` still creates correct PRs

---

### Phase 4: Connect ChangeDomain to Registry ✅

**Status**: Complete

The `ChangeDomain` enum duplicates subsystem identifiers.

#### Current State

**File:** [bkt/src/manifest/ephemeral.rs](../../../bkt/src/manifest/ephemeral.rs#L27-L42)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ChangeDomain {
    Flatpak,
    Extension,
    Gsetting,
    Shim,
    Dnf,
    AppImage,
}
```

And the parse function in `local.rs` (lines 102-115):

```rust
fn parse_domain_filter(domain: &str) -> Result<ChangeDomain> {
    match domain.to_lowercase().as_str() {
        "flatpak" | "fp" => Ok(ChangeDomain::Flatpak),
        "extension" | "ext" => Ok(ChangeDomain::Extension),
        // ...
    }
}
```

#### Target State

Option A: **Keep enum but add validation method**

```rust
impl ChangeDomain {
    /// Validate this domain corresponds to a registered subsystem.
    pub fn subsystem_id(&self) -> &'static str {
        match self {
            ChangeDomain::Flatpak => "flatpak",
            ChangeDomain::Extension => "extension",
            ChangeDomain::Gsetting => "gsetting",
            ChangeDomain::Shim => "shim",
            ChangeDomain::Dnf => "system",  // Note: maps to SystemSubsystem
            ChangeDomain::AppImage => "appimage",
        }
    }

    pub fn is_registered(&self) -> bool {
        SubsystemRegistry::builtin().find(self.subsystem_id()).is_some()
    }
}
```

Option B: **Replace enum with registry lookup** (more invasive)

Store domain as `String` in `EphemeralChange` and validate against registry on parse:

```rust
pub struct EphemeralChange {
    pub domain: String,  // Was: ChangeDomain
    // ...
}
```

**Recommended**: Option A for backward compatibility with existing ephemeral.json files.

#### Steps (Option A)

1. **Add subsystem_id() method** to `ChangeDomain` in ephemeral.rs
2. **Add is_registered() method** that checks against registry
3. **Update parse_domain_filter** to use subsystem_id for mapping
4. **Add compile-time test** that all ChangeDomain variants map to valid subsystem IDs:
   ```rust
   #[test]
   fn test_all_domains_are_registered() {
       let registry = SubsystemRegistry::builtin();
       for domain in [
           ChangeDomain::Flatpak,
           ChangeDomain::Extension,
           // ...
       ] {
           assert!(
               registry.find(domain.subsystem_id()).is_some(),
               "ChangeDomain::{:?} maps to unregistered subsystem '{}'",
               domain,
               domain.subsystem_id()
           );
       }
   }
   ```

#### Validation

- [ ] `cargo build` passes
- [ ] `cargo test` passes
- [ ] `bkt local list` shows correct domain names
- [ ] `bkt local commit` works correctly

---

### Phase 5: Connect DriftCategory to Registry ✅

**Status**: Complete

The `DriftCategory` enum defines which subsystems can be drift-checked.

#### Current State

**File:** [bkt/src/commands/drift.rs](../../../bkt/src/commands/drift.rs#L48-L58)

```rust
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum DriftCategory {
    /// Check RPM packages
    Packages,
    /// Check Flatpak applications
    Flatpaks,
    /// Check GNOME extensions
    Extensions,
    /// Check all categories
    All,
}
```

Currently drift checking delegates to `scripts/check-drift` Python script, and the category filter isn't even implemented:

```rust
if let Some(cat) = &category
    && !matches!(cat, DriftCategory::All)
{
    Output::warning(format!(
        "Category filter '{:?}' is not yet supported. Running full drift check.",
        cat
    ));
}
```

#### Target State

There are two approaches:

**Option A: Keep delegation to Python** (minimal change)

- Add `supports_drift()` method to `Subsystem` trait
- Keep `DriftCategory` enum but add mapping to subsystem IDs
- Pass category to Python script when implementing filtering

**Option B: Native Rust drift checking** (larger scope)

- Add `drift(&self, ctx: &SubsystemContext) -> Result<DriftReport>` to `Subsystem` trait
- Each subsystem implements its own drift detection
- Remove Python script dependency

**Recommended**: Option A for now, with Option B as future work documented in Phase 6.

#### Steps (Option A)

1. **Add supports_drift() to Subsystem trait** in subsystem.rs:

   ```rust
   fn supports_drift(&self) -> bool {
       false  // Default: subsystems opt-in to drift detection
   }
   ```

2. **Implement supports_drift()** for relevant subsystems:
   - `FlatpakSubsystem::supports_drift() -> true`
   - `ExtensionSubsystem::supports_drift() -> true`
   - `SystemSubsystem::supports_drift() -> true`

3. **Add registry method**:

   ```rust
   pub fn driftable(&self) -> Vec<&dyn Subsystem> {
       self.subsystems
           .iter()
           .filter(|s| s.supports_drift())
           .map(|s| s.as_ref())
           .collect()
   }
   ```

4. **Add subsystem_id() to DriftCategory**:

   ```rust
   impl DriftCategory {
       pub fn subsystem_ids(&self) -> Vec<&'static str> {
           match self {
               DriftCategory::Packages => vec!["system"],
               DriftCategory::Flatpaks => vec!["flatpak"],
               DriftCategory::Extensions => vec!["extension"],
               DriftCategory::All => vec!["system", "flatpak", "extension"],
           }
       }
   }
   ```

5. **Add validation test** similar to ChangeDomain.

#### Validation

- [ ] `cargo build` passes
- [ ] `cargo test` passes
- [ ] `bkt drift check` still works
- [ ] `bkt drift explain` shows correct categories

---

## Testing Strategy

### Unit Tests

Each phase includes targeted unit tests:

1. **Phase 1**: Test that `parse_capturable_subsystem` accepts valid IDs and rejects invalid ones
2. **Phase 2**: Test that registry-based manifest loading returns expected types
3. **Phase 3**: Test that SubsystemContext path resolution works correctly
4. **Phase 4**: Test `ChangeDomain::subsystem_id()` mapping completeness
5. **Phase 5**: Test `DriftCategory::subsystem_ids()` mapping completeness

### Integration Tests

Add a meta-test that validates all subsystem enumerations are in sync:

```rust
#[test]
fn test_subsystem_registry_completeness() {
    let registry = SubsystemRegistry::builtin();

    // All capturable subsystems have valid IDs
    for subsystem in registry.capturable() {
        assert!(!subsystem.id().is_empty());
        assert!(registry.find(subsystem.id()).is_some());
    }

    // All syncable subsystems have valid IDs
    for subsystem in registry.syncable() {
        assert!(!subsystem.id().is_empty());
        assert!(registry.find(subsystem.id()).is_some());
    }
}
```

### Manual Testing Checklist

- [ ] `bkt capture --help` shows valid subsystem options
- [ ] `bkt apply --help` shows valid subsystem options
- [ ] `bkt capture --only extension,flatpak` works
- [ ] `bkt capture --only invalid` shows helpful error
- [ ] `bkt status` shows all subsystem statuses
- [ ] `bkt local list` shows domain names correctly
- [ ] `bkt drift check` runs without errors

---

## Rollout

Execute phases sequentially, with each phase as a separate commit:

1. **Phase 1** (Remove Vestigial Enums): ~2 hours
   - Highest impact, removes most duplicated code
   - Breaking change to internal API, but CLI unchanged
2. **Phase 2** (Unify status.rs): ~1.5 hours
   - Requires Manifest trait extension
   - Good candidate for pair review
3. **Phase 3** (Unify local.rs): ~1 hour
   - Straightforward threading of SubsystemContext
   - Low risk
4. **Phase 4** (ChangeDomain): ~0.5 hours
   - Non-breaking addition of methods
   - Add validation test
5. **Phase 5** (DriftCategory): ~0.5 hours
   - Non-breaking addition of methods
   - Prepares for future native drift checking

**Total estimated effort**: ~5.5 hours

### Future Work (Out of Scope)

- **Native Rust drift checking**: Replace Python `check-drift` script with Rust implementation using the Subsystem trait
- **Dynamic subsystem registration**: Allow plugins/extensions to register new subsystems
- **Subsystem dependencies**: Model that some subsystems depend on others (e.g., Flatpak remotes before apps)
