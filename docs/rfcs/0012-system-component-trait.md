# RFC 0012: System Component Trait

## Summary

Refactor the `bkt` architecture to introduce a `SystemComponent` trait that standardizes how subsystems (Flatpak, Extensions, DNF, Shims, GSettings) interact with `status`, `capture`, and `apply` commands. This trait sits **atop** the existing `Plannable` infrastructure, providing component-specific structure while preserving the proven planning/execution separation.

## Motivation

Currently, logic for detecting system state is scattered across command implementations:
- `bkt status`: Counts installed/missing/drifted items using inline detection code.
- `bkt capture`: Lists installed items using similar-but-different detection logic.
- `bkt apply`: Calculates diffs using yet another variant of the same logic.

Recent bugs (Flatpak untracked detection, Extension disabled state capture) have shown that these implementations can drift apart. While we've refactored to share public `get_*` functions, a structural fix would prevent such drift by construction.

### Relationship to Existing Architecture

The codebase already implements a separation of concerns via the `Plannable` trait:

```rust
trait Plannable {
    type Plan: Plan;
    fn plan(&self, ctx: &PlanContext) -> Result<Self::Plan>;
}

trait Plan {
    fn describe(&self) -> PlanSummary;
    fn execute(self, ctx: &mut ExecuteContext) -> Result<ExecutionReport>;
    fn is_empty(&self) -> bool;
}
```

The `Plannable` trait separates **read-only planning** from **side-effecting execution**, enabling dry-run, plan composition via `CompositePlan`, and structured reporting. The `SystemComponent` trait proposed here is **complementary**, providing component-specific logic that feeds into the planning infrastructure.

## Design Decisions

Based on discussion, the following decisions have been made:

| Question | Decision |
|----------|----------|
| Object Safety | **Enum dispatch** — Use a `Component` enum to iterate over components |
| Where does `apply()` live? | **In Plannable** — `SystemComponent` is read-only + pure logic; execution stays in `Plan::execute()` |
| DNF Multi-Resource | **Enum Item** — `enum DnfItem { Package(String), Group(String), Copr(CoprRepo) }` |
| Capture Filter | **In trait** — `type CaptureFilter` associated type with `capture()` method |
| Timing | **Implement now** — Part of current "correct by construction" work |

## Design

### Core Traits

```rust
use anyhow::Result;
use std::hash::Hash;

/// Represents the identity and state of a single managed resource.
pub trait Resource: PartialEq + Clone + std::fmt::Debug {
    type Id: Eq + Hash + std::fmt::Display;
    
    fn id(&self) -> Self::Id;
    
    /// Merge two resources with the same ID.
    /// Default: other (user manifest) wins over self (system manifest).
    fn merge(&self, other: &Self) -> Self {
        other.clone()
    }
}

/// A unifying trait for all managed subsystems.
pub trait SystemComponent {
    /// The type of item managed (e.g., FlatpakApp, ExtensionItem).
    type Item: Resource;
    
    /// The manifest format (e.g., FlatpakAppsManifest).
    type Manifest;
    
    /// Filter type for capture operations. Use `()` if no filter needed.
    type CaptureFilter;

    /// Human-readable name for display purposes.
    fn name(&self) -> &'static str;

    // ─────────────────────────────────────────────────────
    // Phase 1: Discovery (Read-Only)
    // ─────────────────────────────────────────────────────

    /// Get the current state of the system.
    fn scan_system(&self) -> Result<Vec<Self::Item>>;

    /// Load and merge manifests (system + user).
    fn load_manifest(&self) -> Result<Self::Manifest>;

    /// Extract items from the manifest for diffing.
    fn manifest_items(&self, manifest: &Self::Manifest) -> Vec<Self::Item>;

    // ─────────────────────────────────────────────────────
    // Phase 2: Reconciliation (Pure Logic)
    // ─────────────────────────────────────────────────────

    /// Calculate differences between system state and manifest.
    /// Default implementation uses standard set-based diffing.
    fn diff(&self, system: &[Self::Item], manifest: &Self::Manifest) -> DriftReport<Self::Item> {
        let manifest_items = self.manifest_items(manifest);
        let system_ids: std::collections::HashSet<_> = system.iter().map(|i| i.id()).collect();
        let manifest_ids: std::collections::HashSet<_> = manifest_items.iter().map(|i| i.id()).collect();

        DriftReport {
            to_install: manifest_items.iter()
                .filter(|i| !system_ids.contains(&i.id()))
                .cloned().collect(),
            untracked: system.iter()
                .filter(|i| !manifest_ids.contains(&i.id()))
                .cloned().collect(),
            to_update: Vec::new(), // Override for components with rich state
            synced_count: manifest_items.iter()
                .filter(|i| system_ids.contains(&i.id()))
                .count(),
        }
    }

    // ─────────────────────────────────────────────────────
    // Phase 3: Capture (System → Manifest)
    // ─────────────────────────────────────────────────────

    /// Whether this component supports capture (system → manifest).
    /// Components like Shims return false (derived state only).
    fn supports_capture(&self) -> bool {
        true
    }

    /// Capture system state to manifest, with optional filtering.
    /// Returns None if component doesn't support capture.
    fn capture(&self, system: &[Self::Item], filter: Self::CaptureFilter) 
        -> Option<Result<Self::Manifest>>;
}

/// Result of diffing system state against manifest.
#[derive(Debug, Clone, Default)]
pub struct DriftReport<T> {
    /// Items in manifest but not on system (pending install).
    pub to_install: Vec<T>,
    /// Items on system but not in manifest (pending capture).
    pub untracked: Vec<T>,
    /// Items that exist in both but with different state (e.g., enabled vs disabled).
    pub to_update: Vec<(T, T)>, // (current, desired)
    /// Count of items already in sync.
    pub synced_count: usize,
}
```

### Enum Dispatch for Heterogeneous Collections

Since `SystemComponent` has associated types, it's not object-safe. We use enum dispatch:

```rust
/// All managed components in the system.
pub enum Component {
    Flatpak(FlatpakComponent),
    Extension(ExtensionComponent),
    GSettings(GSettingsComponent),
    Dnf(DnfComponent),
    Shim(ShimComponent),
}

impl Component {
    /// Get status for this component.
    pub fn status(&self) -> Result<ComponentStatus> {
        match self {
            Component::Flatpak(c) => c.component_status(),
            Component::Extension(c) => c.component_status(),
            // ...
        }
    }
    
    /// All components in standard order.
    pub fn all() -> Vec<Component> {
        vec![
            Component::Flatpak(FlatpakComponent),
            Component::Extension(ExtensionComponent),
            Component::GSettings(GSettingsComponent),
            Component::Dnf(DnfComponent),
            Component::Shim(ShimComponent),
        ]
    }
}
```

### Component Classification

Analysis of the existing codebase reveals three categories of components:

#### Category A: Bidirectional Components
**Flatpak, Extensions, GSettings**

Standard present/absent semantics. Support both apply (manifest→system) and capture (system→manifest).

#### Category B: Derived-State Components  
**Shims**

System state is entirely generated from manifest. Always regenerated from scratch, no meaningful capture. `supports_capture() → false`.

#### Category C: Multi-Resource Components
**DNF**

Manages multiple distinct resource types (packages, groups, COPR repos) with different lifecycles.

**Decision**: Use enum Item type:

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum DnfItem {
    Package(String),
    Group(String),
    Copr(CoprRepo),
}

impl Resource for DnfItem {
    type Id = String;
    
    fn id(&self) -> String {
        match self {
            DnfItem::Package(name) => format!("pkg:{}", name),
            DnfItem::Group(name) => format!("grp:{}", name),
            DnfItem::Copr(repo) => format!("copr:{}", repo.name),
        }
    }
}
```

### Rich State Handling (Extensions)

Extensions demonstrate that not all components have simple present/absent semantics:

```rust
enum ExtensionState {
    NotInstalled,
    InstalledDisabled,
    InstalledEnabled,
}
```

Components with rich state should override `diff()` to populate `to_update`:

```rust
impl SystemComponent for ExtensionComponent {
    fn diff(&self, system: &[Self::Item], manifest: &Self::Manifest) -> DriftReport<Self::Item> {
        let mut report = DriftReport::default();
        
        for manifest_ext in self.manifest_items(manifest) {
            match system.iter().find(|s| s.id() == manifest_ext.id()) {
                None => report.to_install.push(manifest_ext),
                Some(sys_ext) if sys_ext.enabled != manifest_ext.enabled => {
                    report.to_update.push((sys_ext.clone(), manifest_ext));
                }
                Some(_) => report.synced_count += 1,
            }
        }
        // ... handle untracked
        report
    }
}
```

### Capture with Filters (GSettings)

GSettings cannot enumerate "all settings" — the space is infinite. Capture requires explicit scoping.

**Decision**: The `capture()` method takes a `CaptureFilter` associated type:

```rust
impl SystemComponent for GSettingsComponent {
    type Item = GSetting;
    type Manifest = GSettingsManifest;
    type CaptureFilter = Vec<String>; // Schema names to capture
    
    fn capture(&self, system: &[Self::Item], filter: Self::CaptureFilter) 
        -> Option<Result<Self::Manifest>> 
    {
        // Only capture settings from specified schemas
        let filtered: Vec<_> = system.iter()
            .filter(|s| filter.contains(&s.schema))
            .cloned()
            .collect();
        Some(Ok(GSettingsManifest { settings: filtered }))
    }
}

// For components with no filter:
impl SystemComponent for FlatpakComponent {
    type CaptureFilter = (); // Unit type = no filter
    
    fn capture(&self, system: &[Self::Item], _filter: ()) 
        -> Option<Result<Self::Manifest>> 
    {
        Some(Ok(FlatpakAppsManifest { apps: system.to_vec() }))
    }
}

// For derived-state components:
impl SystemComponent for ShimComponent {
    type CaptureFilter = ();
    
    fn capture(&self, _system: &[Self::Item], _filter: ()) 
        -> Option<Result<Self::Manifest>> 
    {
        None // Shims don't support capture
    }
}
```

### Integration with Status Command

The `status` command becomes a generic loop:

```rust
fn gather_manifest_status(components: &[Box<dyn StatusProvider>]) -> ManifestStatus {
    let mut status = ManifestStatus::default();
    
    for comp in components {
        let system = comp.scan_system()?;
        let manifest = comp.load_manifest()?;
        let drift = comp.diff(&system, &manifest);
        
        status.add_section(comp.name(), ComponentStatus {
            total: drift.synced_count + drift.to_install.len(),
            synced: drift.synced_count,
            pending: drift.to_install.len(),
            untracked: drift.untracked.len(),
        });
    }
    
    status
}
```

### Integration with Plannable

`SystemComponent` provides the data; `Plannable` provides the execution model:

```rust
struct SyncCommand<C: SystemComponent> {
    component: C,
}

impl<C: SystemComponent> Plannable for SyncCommand<C> {
    type Plan = SyncPlan<C::Item>;
    
    fn plan(&self, _ctx: &PlanContext) -> Result<Self::Plan> {
        let system = self.component.scan_system()?;
        let manifest = self.component.load_manifest()?;
        let drift = self.component.diff(&system, &manifest);
        
        Ok(SyncPlan {
            component_name: self.component.name(),
            to_install: drift.to_install,
            to_update: drift.to_update,
        })
    }
}
```

## Performance Considerations

For efficient diffing with large item counts:
- Use `HashSet` for O(1) membership testing during diff (default implementation does this).
- Cache system state within a single planning cycle.
- Consider lazy evaluation for expensive system scans.

## Manifest Merging Strategy

All components support system + user manifest merging with consistent semantics:
- Items are identified by `Resource::Id`
- User manifest entries override system entries with the same ID
- The `Resource::merge()` method controls how conflicts are resolved

## Edge Cases

### Concurrent Modifications
If system state changes between `scan_system()` and plan execution, the plan may become stale. Mitigations:
- Plans should re-validate preconditions in `execute()` for critical operations
- Long-running plans should be cancellable

### Partial Failures
When installing 3/5 items succeeds, the `ExecutionReport` should capture:
- Which operations succeeded
- Which failed and why
- Whether to continue or abort

### Derived vs. Discovered State
Components like Shims represent derived state — the system state is generated from manifest with no independent existence:
- Return `supports_capture() → false`
- Apply is always full regeneration
- Status shows only pending/synced, never untracked

## Migration Path

1. **Phase 0**: Current state — shared `get_*` functions in domain modules ✓
2. **Phase 1**: Define `SystemComponent` trait in `src/component.rs`
3. **Phase 2**: Implement for Flatpak (simplest, most trait-compatible)
4. **Phase 3**: Update `bkt status` to use trait for Flatpak section
5. **Phase 4**: Iteratively port Extensions, GSettings, DNF, Shims
6. **Phase 5**: Refactor `capture` and `apply` to use generic loops

Existing `Plannable` implementations remain unchanged — `SystemComponent` wraps them.

## Open Questions

1. **Execution Context**: DNF behaves differently in Host (rpm-ostree) vs. Dev (dnf). Should context be passed to `SystemComponent` methods, or handled at the `Plannable` layer?

2. **Atomicity**: Should operations be atomic per-item or per-plan? What if flatpak install fails halfway?

3. **Validation Timing**: Should item validation happen in `diff()` (reject invalid items early) or `execute()` (defer validation)?

4. **PR Mode Integration**: How does this trait interact with `--pr` / `--pr-only` modes that update Containerfile instead of system?

