# RFC 0029: Subsystem Dependencies

Phase-based ordering for subsystem execution and planning.

## Motivation

Subsystems have implicit ordering requirements (for example, remotes before
apps). A small, explicit phase model makes the order deterministic and easier
to reason about, without introducing a full dependency graph.

## Design

### Command Surface

This RFC introduces ordering semantics, not new CLI commands. The phase model
is consumed by subsystem registries and planning code.

### Manifest Format

No new manifests are introduced. Phase selection is declared in Rust on each
subsystem implementation via `Subsystem::phase()`.

### Behavior

Phases are defined as:

- `Infrastructure`: remotes, registries, repositories.
- `Packages`: installable units.
- `Configuration`: settings and dependent configuration.

The registry orders subsystems by phase. Within a phase, ordering is stable
based on registration order. Capture ordering is the reverse of sync ordering
(configuration to packages to infrastructure).

## Implementation Notes

- The phase model is implemented in `ExecutionPhase` and `Subsystem::phase()`.
- `SubsystemRegistry::syncable()` returns subsystems ordered by phase.
- `SubsystemRegistry::capturable()` returns subsystems in reverse phase order.

## Known Gaps

- `bkt apply` and `bkt capture` still use their own hard-coded subsystem lists
  and do not yet consume the registry or phase ordering.
- No explicit per-subsystem dependency edges or `after` hints.````markdown

# RFC-0029: Subsystem Dependencies

- **Status**: Implemented
- **Created**: 2026-01-24
- **Depends on**: RFC-0028

## Summary

Introduce tiered execution phases for subsystems to enforce deterministic ordering and make implicit dependencies explicit. The registry will execute subsystems by phase: infrastructure → packages → configuration.

## Motivation

Subsystems currently execute in arbitrary order. This is fragile because several subsystems depend on the side effects of others:

- Flatpak remotes must exist before apps can be installed.
- Distrobox containers may need to exist before shims that reference them.
- Extensions may depend on system packages being installed.

The current `bootc-bootstrap` script already encodes a reliable ordering (remotes → apps → extensions → settings → shims). The manifest split between `flatpak-remotes.json` and `flatpak-apps.json` is another indicator of dependency layering.

We should bring this ordering into the subsystem model itself instead of encoding it in ad-hoc scripts.

## Goals

- Provide deterministic subsystem ordering.
- Make dependency tiers explicit and easy to reason about.
- Preserve backward compatibility with existing subsystems.
- Keep the model simple enough to implement quickly.

## Non-Goals

- A full dependency graph with arbitrary edges in the initial release.
- Expressing fine-grained resource dependencies within a single subsystem.
- Automatic reordering based on runtime discovery.

## Guide-level Explanation

Subsystems declare a high-level phase. The registry executes phases in order:

1. **infrastructure**: remotes, repositories, registries
2. **packages**: installable units (flatpak apps, system packages)
3. **configuration**: extensions, gsettings, shims

Subsystems default to `configuration` if they do not override the phase.

Example trait method:

```rust
fn phase(&self) -> ExecutionPhase {
    ExecutionPhase::Configuration
}
```

## Design Options

### Option A: Tiered Phases (Recommended)

A small, fixed set of phases with explicit ordering. This is easy to understand and implement, and mirrors the existing operational ordering in `bootc-bootstrap`.

**Pros**

- Simple mental model
- Minimal API surface
- Deterministic ordering
- Backward compatible via default phase

**Cons**

- Limited granularity
- No expressible cross-phase dependencies beyond the phase boundary

### Option B: Explicit Dependency Graph

Subsystems declare dependencies on other subsystems, producing a DAG.

**Pros**

- Fine-grained control
- Naturally models complex relationships

**Cons**

- More complex API
- Requires cycle detection from day one
- Harder to keep stable across plugin evolution

### Option C: Implicit Runtime Ordering

Order determined by runtime observation or subsystem registration order.

**Pros**

- No new API

**Cons**

- Non-deterministic and fragile
- Hard to debug

## Recommended Approach

Adopt **Option A** and introduce an `ExecutionPhase` enum plus a `phase()` method on the subsystem trait. This provides deterministic ordering while preserving simplicity. If future needs arise, Option B can be layered on top of the phase model.

## Reference-level Explanation

### ExecutionPhase

A new enum defines the execution tiers:

- `Infrastructure`
- `Packages`
- `Configuration`

### Subsystem Trait Extension

Subsystems may override:

```rust
fn phase(&self) -> ExecutionPhase {
    ExecutionPhase::Configuration
}
```

The registry sorts subsystems by phase and executes them in order. Within a phase, ordering is stable and deterministic (existing registration order).

## Implementation Plan

### Phase 1: Phase-aware Registry

- Add `ExecutionPhase` enum.
- Add `phase()` method to the subsystem trait with default `Configuration`.
- Update `SubsystemRegistry` to group by phase and execute in order.
- Migrate built-in subsystems to explicit phases.

### Phase 2: Dependency Hints (Optional)

- Allow subsystems to declare `after` relationships within a phase.
- Keep this as a soft ordering hint, not a hard dependency.

### Phase 3: Full Dependency Graph (Future)

- Support explicit `depends_on` edges.
- Produce a DAG ordering.

## Cycle Detection Strategy

For the phase-based system, cycles are not possible because the phase ordering is fixed. If Phase 2 or Phase 3 introduces explicit edges, the registry will:

- Build a dependency graph.
- Perform a topological sort.
- If a cycle is detected, produce a structured error listing the involved subsystems and abort execution.

## Security Considerations

The phase model does not introduce new execution capabilities; it only changes ordering. Any future explicit dependencies must be validated against declared subsystem identifiers to avoid spoofing or injection.

## Alternatives Considered

- **Full DAG immediately**: rejected for complexity and overhead.
- **Registration order only**: already problematic and not deterministic across plugin discovery.

## Open Questions

- Should phase ordering be configurable for advanced users?
- Do plugin subsystems need a way to declare their phase in `plugin.json`?
- Should we allow explicit phase-level `after` constraints for built-in subsystems?

```

```
