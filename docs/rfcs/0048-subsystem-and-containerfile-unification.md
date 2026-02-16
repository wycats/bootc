# RFC 0048: Subsystem and Containerfile Unification

## Status

Draft

## Problem

Two core architectural promises are currently broken:

1. **The Subsystem Registry is dead code.** `subsystem.rs` implements
   RFC-0026's vision — a trait-based registry with execution phases,
   unified capture/sync, and filtering. But `apply.rs` and `capture.rs`
   each define their own local `Subsystem` enums and hard-code their
   subsystem lists. The registry has zero callers outside its own tests.

2. **The Containerfile generator violates its own axiom.** RFC 0042
   established: "Every piece of information in the Containerfile must
   trace back to a manifest file." In practice, the generator contains
   hard-coded paths, special-case logic, and knowledge that should live
   in manifests.

These are two symptoms of the same problem: the architecture was
designed correctly but the implementation drifted. Each feature was
wired in with "just this one special case" and the principled
abstractions were never adopted.

## Evidence

### Three Subsystem Enums

The codebase has three separate subsystem enumerations that don't agree
on membership:

| Location | Enum | Members |
|---|---|---|
| `subsystem.rs` | `Subsystem` trait + `SubsystemRegistry` | Extension, Flatpak, GSettings, Shim, Distrobox, AppImage, Homebrew |
| `apply.rs` | `Subsystem` (local enum) | Shim, Distrobox, Gsetting, Extension, Flatpak, AppImage |
| `capture.rs` | `CaptureSubsystem` (local enum) | Extension, Distrobox, Flatpak, System, AppImage, Homebrew |

Apply has Shim but not System/Homebrew. Capture has System/Homebrew but
not Shim. Neither uses the registry.

### Containerfile Axiom Violations

The generator (`containerfile.rs`) contains hard-coded knowledge that
should trace to manifests:

| Hard-coded content | Should come from |
|---|---|
| Base image (`ghcr.io/ublue-os/bazzite-gnome:stable`) | A manifest field (e.g., `repo.json` or new `image-base.json`) |
| `/opt` relocation mappings (1password, microsoft-edge) | External repos manifest (new `opt_path` field) |
| `fc-cache -f` special case | The existing `font-cache` module in `image-config.json` |
| tmpfiles entries for `/var/opt/*` | Derived from `/opt` relocation data |
| Host shim template (`/bin/bash`, `flatpak-spawn --host`) | Shim manifest or fragment |
| RPM cleanup/snapshot paths | Build policy manifest or fragment |
| `bkt-build` tool paths in every stage | Tools manifest |

### Missing RFC

`subsystem.rs` references "RFC-0026" but no such RFC file exists in
`docs/rfcs/`. The subsystem trait was implemented without a written
design document, which may explain why adoption stalled.

## Design Goals

The end state should look like it was built from scratch — no residue
from the incremental journey. Specifically:

1. **One subsystem registry.** `apply` and `capture` iterate the
   registry, filtered by capability (`supports_capture`,
   `supports_sync`). The local enums are deleted.

2. **Phase ordering is authoritative.** The `ExecutionPhase` enum in
   the registry determines execution order for both apply and capture.
   No hard-coded ordering in commands.

3. **Every Containerfile line traces to data.** The generator reads
   manifests and fragments. It contains no domain-specific knowledge
   about specific packages, paths, or applications.

4. **New subsystems are additive.** Adding a subsystem means:
   implementing the `Subsystem` trait, adding a manifest file, and
   registering it. No changes to `apply.rs`, `capture.rs`, or
   `containerfile.rs` scaffolding.

## Plan

### Phase 1: Wire the Registry

**Goal:** `apply` and `capture` use `SubsystemRegistry` instead of
local enums.

1. Audit each subsystem implementation in the registry against what
   `apply.rs` and `capture.rs` actually do. Fill gaps.
2. Replace the local `Subsystem` / `CaptureSubsystem` enums with
   registry iteration filtered by `supports_sync()` /
   `supports_capture()`.
3. Preserve the `--only` / `--exclude` CLI flags, backed by
   `registry.filtered()`.
4. Delete the local enums.
5. Verify: `bkt apply` and `bkt capture` produce identical behavior.

### Phase 2: Containerfile Manifest Extraction

**Goal:** Extract hard-coded generator knowledge into manifest fields.

For each violation identified above, either:
- Add a field to an existing manifest (preferred), or
- Create a new manifest file, or
- Move the content to a fragment (for truly bespoke logic)

Priority order (by impact):

1. **Base image** → add `base_image` field to `repo.json`
2. **`/opt` relocation** → add `opt_path` field to external repos entries
3. **tmpfiles** → derive from `/opt` relocation data
4. **`fc-cache`** → honor the existing `font-cache` module instead of
   special-casing
5. **Host shim template** → add template fields to shim schema
6. **RPM cleanup/snapshot** → build policy fragment or manifest
7. **Tool paths** → tools manifest

### Phase 3: Subsystem ↔ Containerfile Connection

**Goal:** Subsystems that contribute to the Containerfile do so through
the registry, not through ad-hoc generator functions.

This is the deeper unification: a subsystem that has both runtime
behavior (apply/capture) and build-time behavior (Containerfile
generation) should express both through the same trait. For example:

```rust
pub trait Subsystem {
    // ... existing methods ...

    /// Emit Containerfile build stages for this subsystem.
    /// Returns None if this subsystem has no build-time component.
    fn containerfile_stages(&self, ctx: &BuildContext)
        -> Result<Option<Vec<ContainerfileStage>>>;

    /// Emit Containerfile image-stage lines for this subsystem.
    fn containerfile_image_lines(&self, ctx: &BuildContext)
        -> Result<Option<Vec<String>>>;
}
```

The generator becomes a loop over the registry instead of a sequence
of `emit_*` functions.

### Phase 4: Cleanup

1. Remove all `emit_*` functions that are now subsumed by subsystem
   trait methods.
2. Verify `bkt containerfile generate` produces identical output.
3. Verify `bkt containerfile check` passes.
4. Update documentation to reflect the unified model.

## Relationship to Other RFCs

- **RFC-0026 (Subsystem Trait)**: This RFC completes the adoption that
  0026 started. The trait design is sound; the problem is that nothing
  uses it.
- **RFC 0042 (Managed Containerfile)**: This RFC enforces the axiom
  that 0042 established but the implementation violated.
- **RFC 0047 (bkt wrap)**: The wrapper build stage (now a Containerfile
  stage derived from manifest data) is the first example of the Phase 3
  pattern — a subsystem expressing build-time behavior through
  structured data.

## Success Criteria

- `SubsystemRegistry` is the sole source of subsystem enumeration.
- No local `Subsystem` or `CaptureSubsystem` enums exist.
- `containerfile.rs` contains zero hard-coded package names, paths, or
  application-specific knowledge.
- Adding a new subsystem requires zero changes to `apply.rs`,
  `capture.rs`, or the Containerfile generator scaffolding.
- `bkt containerfile check` enforces that the committed Containerfile
  matches what the generator produces from manifests.
