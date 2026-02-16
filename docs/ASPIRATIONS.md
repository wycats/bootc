# Aspirations: Where the Pitch Meets Reality

_Companion to [CONCEPTS.md](CONCEPTS.md) — an honest map of what works
today, what's planned, and what's still aspirational._

---

## How to Read This

CONCEPTS.md is a pitch. It describes the experience bkt is building
toward. This document tracks where the implementation is relative to
that pitch, so we know what to work on and don't accidentally present
aspirations as features.

Each section maps a CONCEPTS.md claim to its current state.

---

## Capture: "bkt can capture what you've done"

**Pitch:** You use your desktop normally, then run `bkt capture` to
snapshot what changed into manifests.

**Reality today:**

| Subsystem | Capture works? | Notes |
|---|---|---|
| Flatpaks | ✅ Yes | Captures installed app IDs |
| GNOME Extensions | ✅ Yes | Captures enabled extension UUIDs |
| GSettings | ⚠️ Partial | Schema/key-scoped, manual. Does NOT auto-discover "values that differ from defaults" |
| Distrobox packages | ✅ Yes | Captures toolbox DNF state |
| AppImages | ✅ Yes | Via GearLever integration |
| Homebrew | ✅ Yes | Captures brew list |
| System packages | ✅ Yes | Captures layered RPMs |
| Dev tools (cargo bin) | ❌ No | Planned — doctor check exists but no capture |

**Aspirations:**

- **GSettings default-diff discovery** (RFC 0016): Automatically find
  settings that differ from schema defaults, instead of requiring
  manual schema/key specification. This is the biggest gap vs the pitch.
- **Drift detection** (RFC 0007): Proactively detect when system state
  has diverged from manifests, rather than requiring manual capture.
- **Dev tool capture**: Detect binaries in `~/.cargo/bin` (and similar)
  that aren't tracked, and offer to capture them.

---

## Apply: "Make a machine match the manifests"

**Pitch:** Run `bkt apply` on a fresh machine and get your full desktop.

**Reality today:**

`bkt apply` handles: shims, distrobox, gsettings, extensions
(enable/disable), flatpaks, AppImages.

But **first-time setup** requires the bootstrap script, not `bkt apply`:

| What | `bkt apply`? | Bootstrap? |
|---|---|---|
| Flatpak apps | ✅ | ✅ |
| Flatpak remotes | ❌ | ✅ |
| Extension enable/disable | ✅ | ✅ |
| Extension installation | ❌ | ✅ |
| GSettings | ✅ | ✅ |
| Distrobox setup | ✅ | ❌ |
| Shim sync | ✅ | ❌ |

**Aspiration:** `bkt apply` should be sufficient for a complete setup,
with bootstrap being a thin wrapper that calls `bkt apply` with a
"first-run" flag. The distinction between bootstrap and apply should
be an implementation detail, not a user-facing concept.

---

## PR Creation: "Created PR #47 with your changes"

**Pitch:** `bkt capture` creates a PR with your changes.

**Reality today:** Capture updates local manifest files. PR creation
is part of the `bkt <subsystem> add/remove` command-punning flow
(RFC 0001), not the capture command.

**Aspiration:** Capture should optionally create a PR (or at least a
commit) with the changes it detected. The command-punning flow handles
the "add one thing" case; capture handles the "snapshot everything"
case. Both should be able to produce PRs.

---

## The Dev Sandbox: "Commands just work"

**Pitch:** A single invisible container where dev tools live. You run
`cargo install ripgrep` and `rg` just works on the host.

**Reality today:** Two separate mechanisms:

- **`bkt distrobox`**: Manages distrobox containers, exports, and
  shims. The "invisible" layer that makes container commands available
  on the host.
- **`bkt dev`**: Manages toolbox DNF packages. A different container
  runtime with different manifests.

The "commands just work" experience depends on distrobox-export shims
and host-shims — two separate mechanisms that achieve similar goals
through different paths.

**Aspiration:** A unified "dev environment" concept that:

- Presents as one thing to the user ("your dev sandbox")
- May use distrobox or toolbox under the hood (implementation detail)
- Has one manifest, one capture flow, one apply flow
- Makes the shim/export mechanism invisible

---

## GSettings: "Values that differ from defaults"

**Pitch:** bkt captures settings that differ from GNOME defaults.

**Reality today:** GSettings capture is schema-scoped and key-scoped.
You tell bkt which schemas and keys to track. It does not automatically
discover which settings you've changed.

**Aspiration (RFC 0016):** `bkt capture` compares your current
GSettings values against schema defaults and captures the diff. This
is technically feasible (GSettings schemas include default values) but
requires careful handling of:

- Vendor overrides (Bazzite sets some defaults differently)
- Settings that change frequently (window positions, recent files)
- Privacy-sensitive settings

---

## What CONCEPTS.md Doesn't Cover (Yet)

These are real, working features that reinforce the pitch but aren't
mentioned in the conceptual guide:

| Feature | What it does | Why it matters |
|---|---|---|
| **AppImages** | Captures/applies AppImage apps via GearLever | Another "install normally, capture later" domain |
| **Homebrew** | Captures/applies Linuxbrew packages | Dev tools that live on the host |
| **Host binaries** | Manages standalone binaries (starship, lazygit) via fetchbin | Upstream tools pinned to specific versions |
| **Upstream pinning** | Tracks external artifact versions with SHA verification | Reproducible builds with integrity checking |
| **Wrapper binaries** | Compiled Rust binaries that launch apps under systemd memory controls | Resource isolation without user-visible complexity |
| **Doctor/diagnostics** | System readiness checks | Helps users understand what's working and what's not |
| **Host shims** | Command delegation from host to Flatpak sandbox | Makes Flatpak apps callable from the terminal |

A follow-up presentation (CONCEPTS-DETAIL.md) should cover these in
the same conceptual style as CONCEPTS.md.

---

## Tracking

| Aspiration | RFC | Priority |
|---|---|---|
| GSettings default-diff | RFC 0016 | High — directly contradicts pitch |
| Drift detection | RFC 0007 | Medium — enables "watches what you do" |
| Unified dev sandbox | (needs RFC) | Medium — structural simplification |
| Apply = bootstrap | (needs RFC) | Medium — user-facing simplification |
| Capture creates PRs | RFC 0001 (partial) | Low — workflow convenience |
| Dev tool capture | (needs RFC) | Low — new subsystem |
