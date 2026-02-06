# RFC 0040: Workstation Design Manifesto

- **Status**: Foundational
- Feature Name: `workstation_manifesto`
- Start Date: 2025-12-24 (originated in asahi-env)
- RFC PR: (leave this empty until PR is opened)
- Tracking Issue: (leave this empty)

> **Provenance:** This document preserves the design philosophy originally
> written as `docs/design/manifesto.md` in the
> [asahi-env](~/Code/asahi-env) project (Dec 2025). That project targeted
> Apple Silicon / Asahi Linux; this version generalizes the doctrines for any
> hardware running the bootc image. Hardware-specific examples are retained
> as illustrations, not prescriptions.

## Summary

Seven doctrines that govern how this workstation is configured. These answer
**why** specific configuration choices are made — a layer above the
capture-first workflow (VISION.md) and the `bkt` architecture
(ARCHITECTURE.md), which answer **how** configuration is managed.

```
Manifesto (this doc)     →  WHY we configure things a certain way
VISION.md                →  HOW configuration is captured and applied
ARCHITECTURE.md          →  HOW the tooling works internally
RFCs (per-feature)       →  WHAT specific features do
```

---

## 1. Adapt the Machine, Not the Human

We reject the notion that migrating to Linux requires abandoning a decade of
muscle memory. The operating system is a tool, and it should bend to the user's
existing neural pathways.

- **The Prime Directive: Physical keys must retain their semantic meaning.**
  - **Command (⌘)** is for _Application & System Control_ (New Tab, Close,
    Switch App).
  - **Control (⌃)** is for _Terminal & Context_ (Interrupt Signal, VIM chords).
  - **Option (⌥)** is for _Alternative Actions_ (Special chars, window
    snapping, OS overrides).
- **The Implementation:** We do not rely on fragile GUI reconfiguration tools.
  We solve this at the **kernel input level** (keyd). We use **layer
  inheritance** (`:A` or `:M`) to ensure that modifier keys maintain their
  "held" state for complex interactions like the Application Switcher, rather
  than firing simple macros.

> See [RFC 0039: keyd Mac Muscle Memory](0039-keyd-mac-muscle-memory.md) for
> the full keyd design.

## 2. Pragmatism over Purity

When the default stack is fragile, replace it with something that works —
even if it's unconventional.

- **The Strategy:** Prefer the stack that is empirically stable over the one
  that is theoretically correct.
- Disable "smart" features (roaming, power save, aggressive scanning) if they
  cause crashes. **Connectivity > Features.**
- **Original case study (Asahi):** Apple's Broadcom firmware crashed under
  `wpa_supplicant`'s aggressive scanning. Solution: adopt `iwd` and disable
  roaming. The principle generalizes: when a subsystem is flaky, simplify it
  ruthlessly rather than adding workarounds on top.

## 3. Virtualize the Glass

Modern displays and input devices behave differently than the standard PC
hardware Linux was built for.

- **The Ultrawide Problem:** "Maximize" is a legacy concept on a wide screen.
  - **Solution:** Treat the screen as a canvas of defined slots, not a bucket
    to be filled. Use tiling extensions (Tiling Shell) to define virtual
    layouts.
- **The Input Feel:** A trackpad is not a mouse.
  - Enforce **tap-to-drag with lock** for fluid, low-pressure interaction.
  - Use a **flat acceleration profile** for predictable movement.
  - Enable **disable-while-typing** for palm rejection.

## 4. Encapsulate the Legacy

When software is incompatible with the host environment, do not pollute the
host. Encapsulate it.

- **The Strategy:** Treat incompatible workloads as "foreign matter."
  Encapsulate them in a lightweight, transparent boundary.
- **Original case study (Asahi):** 16k page size on Apple Silicon broke x86
  apps. Solution: run them in muvm (micro-VM) rather than patching the host.
- **Current application:** Development toolchains live in distrobox containers.
  The host stays clean. Shims provide transparent access.
  See VISION.md § "The Distrobox Strategy."

## 5. Lateral Thinking

When the direct path is blocked by missing drivers or broken features, find
the lateral path. Do not wait for upstream fixes.

- **Case studies:**
  - Thunderbolt PCIe tunneling broken → force "dumb USB" mode via standard
    USB-C cable.
  - Dual monitor support missing → DisplayLink (bypass GPU, render via
    CPU/USB).
  - Native ARM64 browser with sync unavailable → use Edge (path of least
    resistance).
- **The principle:** If the obvious solution requires waiting for someone else
  to fix something, look for the "dumb" standard alternative that works today.

## 6. Truthful by Default

This project lives across machines, distros, and desktop environments.
Portability does not mean "works everywhere"; it means **fails honestly** and
**does not hallucinate success**.

- **The Rule:** If a capability is unavailable (missing tool, missing
  permission, non-systemd system), record it as **skipped** with the reason.
- **The Anti-Goal:** Never produce "successfully empty" output that looks
  healthy but is really "no access."
- **The Consequence:** Every check/probe has an explicit **capability
  boundary**: what it needs (tooling/privileges/platform) and what it does
  when that boundary is not met.

## 7. Evidence + Deletion

Workstation tweaks rot when they are justified by vibes. This project treats
changes as experiments.

- **Every change has evidence.** Capture a before/after snapshot, compare, and
  keep the artifact.
- **Every change has rollback.** If we cannot delete it cleanly, we do not
  trust it.
- **Every probe has deletion criteria.** A probe exists to retire folklore and
  manual rituals; when it no longer deletes confusion, it should be removed.

> This doctrine is the philosophical foundation of the capture-first workflow
> described in VISION.md. The capture mechanism _is_ the evidence loop.

---

## Decision Matrix

When a new problem arises, apply this logic:

1. **Is it a muscle memory conflict?** → Solve it in keyd (kernel level), not
   in the app.
2. **Is a subsystem flaky?** → Simplify the stack. Disable "smart" features.
3. **Is software incompatible with the host?** → Don't patch it. Encapsulate
   it (distrobox, VM, Flatpak sandbox).
4. **Is a hardware feature missing?** → Find the "dumb" standard alternative.
5. **Is this a portability boundary?** → Mark it as skipped with the reason;
   add a capability gate.
6. **Is this a tweak without evidence?** → Add a snapshot + diff + rollback
   loop before trusting it.
