# RFC 0039: keyd Mac Muscle Memory

- **Status**: Foundational
- Feature Name: `keyd_mac_muscle_memory`
- Start Date: 2025-12-27 (ported from asahi-env manifesto, 2025-12-24)
- RFC PR: (leave this empty until PR is opened)
- Tracking Issue: (leave this empty)

> **Provenance:** This RFC captures the keyboard design philosophy originally
> documented in the [asahi-env](~/Code/asahi-env) project's
> `docs/design/manifesto.md` and `bootstrap.md`. That project targeted Apple
> Silicon (Asahi Linux); this repo generalizes it to any hardware running the
> bootc image.

## Summary

Solve Mac-to-Linux muscle memory conflicts at the **kernel input level** using
keyd, so that physical modifier keys retain their semantic meaning regardless of
the application, desktop environment, or hardware platform.

## Core Philosophy: "Adapt the Machine, Not the Human"

We reject the notion that migrating to Linux requires abandoning a decade of
muscle memory. The operating system is a tool, and it should bend to the user's
existing neural pathways.

### The Prime Directive

**Physical keys must retain their semantic meaning.**

| Physical Key    | Semantic Role                      | Examples                                       |
| --------------- | ---------------------------------- | ---------------------------------------------- |
| **Command (⌘)** | Application & System Control       | New Tab, Close, Copy, Paste, Switch App        |
| **Control (⌃)** | Terminal & Context                 | Interrupt Signal, VIM chords                   |
| **Option (⌥)**  | Alternative Actions / OS Overrides | Special chars, window snapping, GNOME overview |

### Why Kernel-Level (keyd)

- We do **not** rely on fragile GUI reconfiguration tools (GNOME settings,
  per-app configs). Those break across updates, sessions, and DEs.
- We solve this at the **kernel input level** so every application — terminal,
  browser, Electron app — sees the same keystrokes.
- We use **Layer Inheritance** (`:A` or `:M`) to ensure modifier keys maintain
  their "held" state for complex interactions like the Application Switcher,
  rather than firing simple macros.

### Decision Rule

> **Is it a Muscle Memory conflict?** → Solve it in keyd (kernel level), not in
> the app.

## Design: Two Layers, Two Roles

The keyd config assigns each bottom-row modifier a distinct role:

### Layer 1: Option (⌥) → `layer(meta)` — The OS Key

```ini
# Physical Option key becomes Super/Meta for OS-level shortcuts.
# Tap: emits leftmeta → triggers GNOME overview (overlay-key = 'Super')
# Hold: Super modifier for window management, workspaces, etc.
leftalt = layer(meta)
```

**Why `leftalt`?** On Mac hardware (and PC keyboards in "Windows mode"), the
physical Option/Alt key sends the `leftalt` scancode. This mapping makes the
physical thumb key — the one a Mac user reaches for instinctively — trigger OS
actions.

### Layer 2: Command (⌘) → `layer(meta_mac)` — The App Key

```ini
# Physical Command key becomes a Cmd→Ctrl translation layer.
# The :A suffix means the layer defaults to Alt as its base modifier.
leftmeta = layer(meta_mac)
rightmeta = layer(meta_mac)

[meta_mac:A]
# Cmd+Tab → Alt+Tab (GNOME app switcher stays open while held)
# Cmd+C → Ctrl+Insert (CUA clipboard — works in terminals too)
# Cmd+V → Shift+Insert
# Cmd+S → Ctrl+S, Cmd+Z → Ctrl+Z, etc.
```

**Why `:A` (Alt base)?** The GNOME application switcher requires Alt to be
_held_ to keep the switcher UI open. A simple `Cmd+Tab → Alt+Tab` macro would
release Alt immediately, dismissing the switcher. Layer inheritance with `:A`
means the physical Command key _is_ Alt for the duration of the hold, so
`Cmd+Tab, Tab, Tab` cycles through windows naturally.

**Why CUA clipboard?** `Ctrl+C` in a terminal sends SIGINT. The IBM CUA
shortcuts (`Ctrl+Insert` / `Shift+Insert` / `Shift+Delete`) perform
clipboard operations without conflicting with terminal signals. Most Linux
apps support CUA natively.

## Scancode Invariant

The config maps **scancodes**, not key labels. The critical invariant:

| Scancode   | Mac Hardware | PC "Windows Mode" | PC "Mac Mode"  |
| ---------- | ------------ | ----------------- | -------------- |
| `leftalt`  | Option (⌥)   | Alt               | ⚠️ Command (⌘) |
| `leftmeta` | Command (⌘)  | Super/Win         | ⚠️ Option (⌥)  |

Mac hardware and PC keyboards in **Windows mode** produce identical scancodes
for the same physical positions. The config was written for Mac hardware and
works on any keyboard that uses the same scancode layout — which is the default
("Windows mode") for virtually all PC keyboards.

### The "Mac Mode" Trap

Many third-party keyboards (e.g., Compx, A4Tech, Keychron) have a firmware
toggle (typically `Fn+A` / `Fn+S` or `Fn+Q` / `Fn+W`) that swaps Alt↔Super
scancodes. Confusingly, this "Mac mode" produces scancodes that are the
**opposite** of what actual Mac hardware sends:

- **Mac hardware**: physical Command → `leftmeta` ✓
- **PC "Mac mode"**: physical Alt → `leftmeta` ✗ (swapped!)

**If the overview stops working on a tap of the physical Alt key, the keyboard
has likely been switched to "Mac mode." Press `Fn+A` (or the keyboard's
equivalent Windows-mode toggle) to restore correct scancode mapping.**

## What This Config Does NOT Do

- **Does not remap Control.** `Ctrl+C` in a terminal is still SIGINT. The
  Control key is pass-through.
- **Does not use `overload()` for the overview tap.** The `layer(meta)` on
  `leftalt` naturally emits `leftmeta` on tap (keyd's default behavior for
  unmapped layer activators), which triggers GNOME's `overlay-key`.
- **Does not use per-device `[ids]` sections.** The config applies to all
  keyboards (`[ids] *`), including the laptop's built-in keyboard. This is
  intentional — the semantic mapping should be consistent across all input
  devices.

## Relationship to Other Config

| System                          | Role                                      | Managed By                                       |
| ------------------------------- | ----------------------------------------- | ------------------------------------------------ |
| keyd (`/etc/keyd/default.conf`) | Scancode → semantic modifier mapping      | This repo (`system/keyd/default.conf`)           |
| GNOME `overlay-key`             | Which keycode opens overview              | gsettings (default `'Super'` — no change needed) |
| GNOME keybindings               | `toggle-overview`, `switch-windows`, etc. | gsettings (per-user state)                       |

The keyd config is the **single source of truth** for modifier semantics. GNOME
settings should not need to compensate for keyd — if they do, the keyd config is
wrong.

## References

- **Origin**: `asahi-env` repo, `docs/design/manifesto.md` §1 ("Adapt the
  Machine, Not the Human")
- **Bootstrap procedure**: `asahi-env` repo, `bootstrap.md` §2 ("Mac-like input
  - shortcuts")
- **keyd upstream**: https://github.com/rvaiya/keyd
- **keyd monitor caveat**: keyd issue #1202 — monitor shows virtual keyboard
  output, not raw input; aliased key names can be misleading
- **keyd modifier collapsing**: keyd issue #1119 — default internal config
  collapses left/right modifiers via aliases
