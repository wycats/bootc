# Phase 4 — Subsystem Unification

**Status:** 🚧 In Progress  
**Started:** February 2026  
**RFC:** [0048-subsystem-and-containerfile-unification](../rfcs/0048-subsystem-and-containerfile-unification.md)

---

## Vision

Phase 4 unifies the subsystem architecture around a tier model that reflects
how different subsystems actually behave:

- **Atomic (Tier 1):** State lives in the image. Changes require rebuild + reboot.
- **Convergent (Tier 2):** State lives at runtime. Changes can apply immediately.

This enables polymorphic operations like `bkt staged` that work across all
subsystems, and eliminates the three competing subsystem enumerations that
currently exist in the codebase.

### The Unified Model

```
                    ┌─────────────────────────────────────┐
                    │         SubsystemRegistry           │
                    │  (single source of truth)           │
                    └─────────────────────────────────────┘
                                    │
              ┌─────────────────────┴─────────────────────┐
              ▼                                           ▼
    ┌─────────────────┐                       ┌─────────────────┐
    │  Tier 1: Atomic │                       │ Tier 2: Convergent │
    │  (image-bound)  │                       │ (runtime-applied)  │
    └─────────────────┘                       └─────────────────┘
    │ system (RPMs)   │                       │ flatpak           │
    │ upstream        │                       │ extension         │
    │ wrappers        │                       │ gsetting          │
    │ config files    │                       │ distrobox         │
    └─────────────────┘                       │ shim, appimage    │
                                              │ homebrew          │
                                              └─────────────────┘
    Operations:                               Operations:
    - staged()                                - sync()
    - containerfile_stages()                  - capture()
                                              - drift()
                                              - baseline()
```

---

## Goals

### Goal 1: Add Tier to Subsystem Trait ✅ Partial

Add `SubsystemTier` enum and `tier()` method to distinguish Atomic vs Convergent.

| Task | Status |
|------|--------|
| Define `SubsystemTier` enum | ⬜ Not started |
| Add `tier()` method to `Subsystem` trait | ⬜ Not started |
| Classify existing subsystems by tier | ⬜ Not started |
| Add `supports_staged()` capability flag | ⬜ Not started |

### Goal 2: Wire `bkt system staged` Through Trait

Move the staged diff logic from command code to the `SystemSubsystem` trait impl.

| Task | Status |
|------|--------|
| Create `StagedContext` and `StagedReport` types | ⬜ Not started |
| Add `staged()` method to `Subsystem` trait | ⬜ Not started |
| Implement `staged()` for `SystemSubsystem` | ⬜ Not started |
| Refactor `bkt system staged` to use trait | ⬜ Not started |

### Goal 3: Add Missing Atomic Subsystems

Register image-bound subsystems that currently live outside the registry.

| Task | Status |
|------|--------|
| Create `UpstreamSubsystem` with `staged()` | ⬜ Not started |
| Create `WrapperSubsystem` with `staged()` | ⬜ Not started |
| Register in `SubsystemRegistry` | ⬜ Not started |
| Test `bkt staged` across all Tier 1 subsystems | ⬜ Not started |

### Goal 4: Wire the Registry

Replace hard-coded subsystem lists with registry iteration.

| Task | Status |
|------|--------|
| Audit `apply.rs` local enum vs registry | ⬜ Not started |
| Audit `capture.rs` local enum vs registry | ⬜ Not started |
| Replace local enums with registry iteration | ⬜ Not started |
| Delete local `Subsystem` / `CaptureSubsystem` enums | ⬜ Not started |
| Verify `bkt apply` behavior unchanged | ⬜ Not started |
| Verify `bkt capture` behavior unchanged | ⬜ Not started |

### Goal 5: Containerfile Manifest Extraction (Future)

Extract hard-coded generator knowledge into manifest fields.

| Task | Status |
|------|--------|
| Add `base_image` field to `repo.json` | ⬜ Not started |
| Add `opt_path` field to external repos | ⬜ Not started |
| Derive tmpfiles from `/opt` relocation | ⬜ Not started |
| Add template fields to shim schema | ⬜ Not started |

---

## Completed Items

| ID | Item | PR | Date |
|----|------|-----|------|
| 1 | `bkt system staged` command | #127 | 2026-02-16 |
| 2 | RFC 0048 expanded with tier model | #127 | 2026-02-16 |
| 3 | RFC 0029 absorbed into 0048 | #127 | 2026-02-16 |

---

## Key Insights

### Why Tiers Matter

The tier distinction isn't just organizational — it determines what operations
make sense for each subsystem:

| Operation | Atomic | Convergent |
|-----------|--------|------------|
| `add`/`remove` | Deferred (PR → rebuild) | Immediate or deferred |
| `sync` | N/A | Converge runtime to manifest |
| `capture` | Capture layered RPMs | Capture runtime state |
| `staged` | Diff staged vs booted | Three-way diff |
| `drift` | N/A | Compare manifest vs runtime |

### The Three-Enum Problem

Before this phase, the codebase had three competing subsystem enumerations:

| Location | Members |
|----------|---------|
| `subsystem.rs` registry | Extension, Flatpak, GSettings, Shim, Distrobox, AppImage, Homebrew |
| `apply.rs` local enum | Shim, Distrobox, Gsetting, Extension, Flatpak, AppImage |
| `capture.rs` local enum | Extension, Distrobox, Flatpak, System, AppImage, Homebrew |

Apply has Shim but not System/Homebrew. Capture has System/Homebrew but not
Shim. Neither uses the registry. This phase eliminates the local enums.

---

## Success Criteria

- [ ] `SubsystemTier` enum exists and every subsystem declares its tier
- [ ] `SubsystemRegistry` is the sole source of subsystem enumeration
- [ ] No local `Subsystem` or `CaptureSubsystem` enums exist
- [ ] `bkt system staged` works through the trait, not special-case code
- [ ] `bkt staged` can iterate all Tier 1 subsystems polymorphically
- [ ] Adding a new subsystem requires zero changes to `apply.rs`, `capture.rs`
