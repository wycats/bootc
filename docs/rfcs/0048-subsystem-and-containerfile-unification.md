# RFC 0048: Subsystem Architecture Unification

- Feature Name: `subsystem_unification`
- Start Date: 2026-02-15
- RFC PR: (leave this empty)
- Tracking Issue: (leave this empty)
- Status: Draft

## Problem

The subsystem architecture has three interconnected problems:

1. **The Subsystem Registry is dead code.** `subsystem.rs` implements a
   trait-based registry with execution phases, unified capture/sync, and
   filtering. But `apply.rs` and `capture.rs` each define their own local
   `Subsystem` enums and hard-code their subsystem lists. The registry has
   zero callers outside its own tests.

2. **The Containerfile generator violates its own axiom.** RFC 0042
   established: "Every piece of information in the Containerfile must trace
   back to a manifest file." In practice, the generator contains hard-coded
   paths, special-case logic, and knowledge that should live in manifests.

3. **The tier distinction is implicit.** Subsystems fall into two
   fundamentally different lifecycle models — image-bound (Atomic) vs
   runtime-applied (Convergent) — but this isn't reflected in the trait
   design. This makes it impossible to implement features like `staged`
   diffing polymorphically.

## Subsystem Tiers

The most important architectural insight is that subsystems have two
fundamentally different lifecycle models:

### Tier 1: Atomic (Image-bound)

- **State lives in the image** — changes require image rebuild + reboot
- **No local drift possible** — the image is the source of truth
- **"Desired state" = "what's in the image"**
- **Examples:** `system` (RPMs), `upstream` (binaries), `wrappers`, config files

Atomic subsystems support:
- `staged()` — diff between staged deployment and booted deployment
- `capture()` — capture layered packages to manifest (for `system` only)
- No `sync()` — changes are deferred to image rebuild

### Tier 2: Convergent (Runtime-applied)

- **State lives outside the image** — user space, flatpak DB, gsettings, etc.
- **Changes can be applied immediately OR deferred**
- **Local drift is possible** — runtime state may differ from manifest
- **Three-way state model:** manifest intent, current runtime, staged baseline
- **Examples:** `flatpak`, `extension`, `gsetting`, `distrobox`, `shim`,
  `appimage`, `homebrew`

Convergent subsystems support:
- `sync()` — converge runtime state to manifest
- `capture()` — capture runtime state to manifest
- `drift()` — compare manifest to runtime state
- `baseline()` — snapshot of "last applied" state for three-way diffs

### Why This Matters

The tier distinction affects every subsystem operation:

| Operation | Atomic | Convergent |
|-----------|--------|------------|
| `add`/`remove` | Deferred (PR → rebuild) | Immediate or deferred |
| `sync` | N/A | Converge runtime to manifest |
| `capture` | Capture layered RPMs | Capture runtime state |
| `status` | Manifest vs image | Manifest vs runtime vs baseline |
| `staged` | Diff staged vs booted | Diff (manifest + staged baseline) vs (manifest + runtime) |

## Execution Phases

Within each tier, subsystems execute in a deterministic phase order:

1. **Infrastructure**: remotes, repositories, registries
2. **Packages**: installable units (flatpak apps, system packages)
3. **Configuration**: extensions, gsettings, shims

The registry orders subsystems by phase. Within a phase, ordering is stable
based on registration order. Capture ordering is the reverse of sync ordering
(configuration → packages → infrastructure).

This absorbs RFC-0029 (Subsystem Dependencies) — phase ordering is part of
the registry design, not a separate concern.

## Evidence of Current Problems

### Three Subsystem Enums

The codebase has three separate subsystem enumerations that don't agree:

| Location       | Enum                                    | Members                                                            |
| -------------- | --------------------------------------- | ------------------------------------------------------------------ |
| `subsystem.rs` | `Subsystem` trait + `SubsystemRegistry` | Extension, Flatpak, GSettings, Shim, Distrobox, AppImage, Homebrew |
| `apply.rs`     | `Subsystem` (local enum)                | Shim, Distrobox, Gsetting, Extension, Flatpak, AppImage            |
| `capture.rs`   | `CaptureSubsystem` (local enum)         | Extension, Distrobox, Flatpak, System, AppImage, Homebrew          |

Apply has Shim but not System/Homebrew. Capture has System/Homebrew but not
Shim. Neither uses the registry.

### Missing Atomic Subsystems

Several image-bound subsystems aren't in the registry at all:

- `upstream` — binaries fetched from GitHub releases
- `external-repos` — third-party RPM repositories
- `image-config` — base image settings
- `wrappers` — Rust wrapper binaries for systemd integration

These should be Tier 1 subsystems with `staged()` support.

### Containerfile Axiom Violations

The generator contains hard-coded knowledge that should trace to manifests:

| Hard-coded content                                   | Should come from                          |
| ---------------------------------------------------- | ----------------------------------------- |
| Base image (`ghcr.io/ublue-os/bazzite-gnome:stable`) | `repo.json` or `image-base.json`          |
| `/opt` relocation mappings                           | External repos manifest (`opt_path` field)|
| `fc-cache -f` special case                           | Font-cache module in `image-config.json`  |
| Host shim template                                   | Shim manifest or fragment                 |

## Proposed Trait Design

```rust
/// Lifecycle tier for a subsystem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubsystemTier {
    /// Image-bound: state lives in the bootc image.
    /// Changes require image rebuild + reboot.
    Atomic,
    /// Runtime-applied: state lives outside the image.
    /// Changes can be applied immediately or deferred.
    Convergent,
}

/// Execution phase within a tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ExecutionPhase {
    Infrastructure,
    Packages,
    Configuration,
}

pub trait Subsystem: Send + Sync {
    fn name(&self) -> &'static str;
    fn id(&self) -> &'static str;
    
    /// The lifecycle tier for this subsystem.
    fn tier(&self) -> SubsystemTier;
    
    /// Execution phase within the tier.
    fn phase(&self) -> ExecutionPhase {
        ExecutionPhase::Configuration
    }

    fn load_manifest(&self, ctx: &SubsystemContext) -> Result<Box<dyn Manifest>>;

    // === Convergent operations (Tier 2) ===
    
    fn capture(&self, ctx: &PlanContext) -> Result<Option<Box<dyn DynPlan>>>;
    fn sync(&self, ctx: &PlanContext, config: &SubsystemConfig) -> Result<Option<Box<dyn DynPlan>>>;
    fn drift(&self, ctx: &SubsystemContext) -> Result<Option<DriftReport>> { Ok(None) }
    
    /// Baseline snapshot for three-way diffs (Tier 2 only).
    fn baseline(&self, ctx: &SubsystemContext) -> Result<Option<Box<dyn Manifest>>> { Ok(None) }

    // === Atomic operations (Tier 1) ===
    
    /// Diff between staged deployment and booted deployment.
    fn staged(&self, ctx: &StagedContext) -> Result<Option<StagedReport>> { Ok(None) }

    // === Build-time operations ===
    
    /// Emit Containerfile build stages for this subsystem.
    fn containerfile_stages(&self, ctx: &BuildContext) -> Result<Option<Vec<ContainerfileStage>>> { Ok(None) }
    
    /// Emit Containerfile image-stage lines for this subsystem.
    fn containerfile_image_lines(&self, ctx: &BuildContext) -> Result<Option<Vec<String>>> { Ok(None) }

    // === Capability flags ===
    
    fn supports_capture(&self) -> bool { true }
    fn supports_sync(&self) -> bool { self.tier() == SubsystemTier::Convergent }
    fn supports_drift(&self) -> bool { self.tier() == SubsystemTier::Convergent }
    fn supports_staged(&self) -> bool { self.tier() == SubsystemTier::Atomic }
}
```

## Implementation Plan

### Phase 1: Add Tier to Existing Subsystems

1. Add `SubsystemTier` enum and `tier()` method to the trait.
2. Classify existing subsystems:
   - **Atomic:** `system`
   - **Convergent:** `flatpak`, `extension`, `gsetting`, `distrobox`, `shim`,
     `appimage`, `homebrew`, `fetchbin`
3. Add `supports_staged()` capability flag.
4. Wire `bkt system staged` through the trait (move logic from `system.rs`
   command to `SystemSubsystem::staged()`).

### Phase 2: Add Missing Atomic Subsystems

1. Create `UpstreamSubsystem` (Tier 1) with `staged()` support.
2. Create `WrapperSubsystem` (Tier 1) with `staged()` support.
3. Register them in `SubsystemRegistry`.
4. Implement `staged()` for each — compare binaries in staged vs booted
   deployment paths.

### Phase 3: Wire the Registry

1. Replace local `Subsystem` / `CaptureSubsystem` enums in `apply.rs` and
   `capture.rs` with registry iteration.
2. Filter by `supports_sync()` / `supports_capture()` and tier.
3. Delete the local enums.
4. Verify: `bkt apply` and `bkt capture` produce identical behavior.

### Phase 4: Containerfile Manifest Extraction

Extract hard-coded generator knowledge into manifest fields:

1. **Base image** → add `base_image` field to `repo.json`
2. **`/opt` relocation** → add `opt_path` field to external repos entries
3. **tmpfiles** → derive from `/opt` relocation data
4. **Host shim template** → add template fields to shim schema

### Phase 5: Subsystem ↔ Containerfile Connection

Subsystems that contribute to the Containerfile do so through the registry:

1. Implement `containerfile_stages()` and `containerfile_image_lines()` for
   relevant subsystems.
2. The generator becomes a loop over the registry instead of a sequence of
   `emit_*` functions.
3. Remove all `emit_*` functions that are now subsumed by trait methods.

## Relationship to Other RFCs

- **RFC-0029 (Subsystem Dependencies)**: Absorbed into this RFC. Phase
  ordering is part of the registry design.
- **RFC-0007 (Drift Detection)**: Drift is a Tier 2 (Convergent) concept.
  This RFC provides the tier model that 0007 should reference.
- **RFC-0028 (Plugin Subsystems)**: Orthogonal. Plugins would implement the
  `Subsystem` trait with an appropriate tier.
- **RFC 0042 (Managed Containerfile)**: This RFC enforces the axiom that
  0042 established but the implementation violated.

## Success Criteria

- `SubsystemTier` enum exists and every subsystem declares its tier.
- `SubsystemRegistry` is the sole source of subsystem enumeration.
- No local `Subsystem` or `CaptureSubsystem` enums exist.
- `bkt system staged` works through the trait, not special-case code.
- `bkt staged` (future) can iterate all Tier 1 subsystems polymorphically.
- `containerfile.rs` contains zero hard-coded package names, paths, or
  application-specific knowledge.
- Adding a new subsystem requires zero changes to `apply.rs`, `capture.rs`,
  or the Containerfile generator scaffolding.
