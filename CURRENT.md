# Current State

_Last updated: 2026-02-10_

One-sentence: this repo extends atomic Linux (bootc/ostree) into the user layer —
making user environment (Flatpaks, extensions, gsettings, dev tools) declarative,
reproducible, and recoverable.

---

## The System at a Glance

Bullet list:

- Base image: Bazzite 43 (Fedora 43 / bootc / ostree / composefs), GNOME Shell
- Dev environment: bootc-dev distrobox (Fedora Toolbox 42)
- CLI tool: bkt (Rust, 27 commands, 9 subsystems)
- Manifests: 12 JSON files, 16 JSON schemas
- Containerfiles: 2 (host image + toolbox)
- Rust crates: 2 (bkt + fetchbin)
- Shell scripts: 8 (in scripts/)
- Systemd units: 4 (1 system service, 2 user services, 1 user timer)
- RFCs: 10 canon · 26 active · 3 withdrawn

---

## Core Loops

Four loops. For each: one-line description, status (✅ Working), key components,
relevant commands.

### Capture Loop (system → manifests → git)

- Timer-driven via bootc-capture.timer / bootc-capture.service
- Covers: flatpaks, extensions, appimages, homebrew, distrobox, system packages (rpm-ostree layered)
- GSettings and fetchbin capture are per-subsystem commands, not in the meta-capture
- Commands: `bkt capture`, `bkt capture --dry-run`

### Bootstrap Loop (login → apply manifests)

- Shell script: scripts/bootc-bootstrap, runs as systemd user oneshot
- Deployment-gated: checks manifest hash, skips if already applied
- Applies (inside hash gate): remotes → flatpaks → extensions → gsettings → shims
- Also runs: distrobox assemble + composefs fontconfig cache workaround before the hash gate

### Image Build Loop (repo → Containerfile → CI → machine)

- Containerfile builds from ghcr.io/ublue-os/bazzite-gnome:stable
- Bakes: system packages, keyd, fonts, fontconfig, polkit rules, skel files, systemd units/overrides
- GitHub Actions builds and pushes to ghcr.io
- `bootc upgrade` + reboot to apply

### Dev Environment Loop (manifest → toolbox image)

- toolbox/Containerfile from registry.fedoraproject.org/fedora-toolbox:42
- `bkt dev install/remove` → updates toolbox manifest + installs via dnf in toolbox
- Toolbox Containerfile is static today (managed sections are TODO)
- Transparent delegation: host ↔ toolbox via distrobox-host-exec / distrobox enter
- 3 command targets: Host, Dev, Either

---

## Subsystems

Table with 9 rows. Columns: Subsystem, Manifest File, Capture, Apply, Drift Comparison, CLI.

| Subsystem | Manifest              | Capture | Apply         | Drift Comparison | CLI             |
| --------- | --------------------- | ------- | ------------- | ---------------- | --------------- |
| Flatpak   | flatpak-apps.json     | ✅      | ✅            | ✅               | `bkt flatpak`   |
| Extension | gnome-extensions.json | ✅      | ✅            | ✅               | `bkt extension` |
| GSettings | gsettings.json        | ✅      | ✅            | ✅               | `bkt gsetting`  |
| AppImage  | appimage-apps.json    | ✅      | ✅            | ✅               | `bkt appimage`  |
| Homebrew  | homebrew.json         | ✅      | ✅            | ✅               | `bkt homebrew`  |
| Fetchbin  | host-binaries.json    | ✅      | ✅            | ✅               | `bkt fetchbin`  |
| Distrobox | distrobox.json        | ✅      | ✅            | partial          | `bkt distrobox` |
| Shim      | host-shims.json       | —       | ✅            | FS check only    | `bkt shim`      |
| System    | system-packages.json  | ✅      | Image rebuild | —                | `bkt system`    |

Notes:

- "Drift Comparison" means the subsystem can compare manifest vs live state
- Flatpak, Extension, GSettings, AppImage, Homebrew, and Fetchbin have comparison code scattered across capture, profile, and status modules
- `bkt drift check` is a stub — it doesn't compose these subsystem comparisons yet
- Shim capture is not needed (shims are generated from manifest, not discovered)
- System subsystem manages package lists but changes land via image rebuild, not at runtime

---

## Baked Configuration

What lives in the Containerfile and system/ directory. List the areas:

- System packages (dnf install in Containerfile)
- keyd (Mac keyboard remapping) — system/keyd/
- Fonts (Inter RPM + JetBrainsMono Nerd Font)
- fontconfig (emoji rendering fix) — system/fontconfig/
- NetworkManager — system/NetworkManager/ (optional, off by default)
- polkit rules — system/polkit-1/
- systemd overrides (journald, logind) — system/systemd/ (optional, off by default)
- Asahi hardware support — system/asahi/ (optional, off by default)
- Remote play (tty2 auto-login service) — system/remote-play/ (optional, off by default)
- Skel files (shell profile, cargo config, etc.) — skel/

---

## CLI Command Reference

Group by purpose. Just command names with brief descriptions, not full docs.

### Core Workflow

- `apply` — apply manifests to system
- `capture` — capture live system state to manifests
- `status` — show system state summary with drift indicators

### Subsystem Management

- `flatpak`, `extension`, `gsetting`, `appimage`, `homebrew`, `fetchbin`, `distrobox`, `shim` (one per registered subsystem)
- `skel` — manage skeleton files (not a registered subsystem)

### Image & System

- `system` — host system package management (baked tier)
- `dev` — toolbox package management
- `admin` — privileged operations (bootc, systemctl, kargs, systemd)
- `containerfile` — sync manifests → Containerfile
- `base` — base image assumptions
- `upstream` — upstream image tracking and pinning

### Observability

- `doctor` — system health checks
- `drift` — drift detection (stub — not yet implemented)
- `profile` — per-subsystem diff view (missing/extra/changed)

### Infrastructure

- `repo` — repository management
- `schema` — JSON schema operations
- `completions` — shell completions
- `changelog` — version management
- `build-info` — build metadata
- `local` — ephemeral local change tracking

---

## Key Gaps

### 1. Drift Detection (Priority #1)

Per-subsystem comparison infrastructure exists (Flatpak, Extension, GSettings,
AppImage, Homebrew, Fetchbin). What's missing: `bkt drift check` should compose
these into a unified report. Currently a stub. `bkt status` has partial drift
data. `bkt profile diff` shows per-subsystem detail. These don't share code.
RFC: 0007

### 2. GSettings Auto-Discovery

No mechanism to discover new gsettings keys the user has changed.
Capture records known keys only.
RFC: 0016

### 3. Upstream Tracking Automation

Manual pinning works (upstream/manifest.json + bazzite-stable.digest).
No automated detection of upstream updates.
RFC: 0006

### 4. Bootstrap / Apply Convergence

bootc-bootstrap (shell, login) and bkt apply (Rust, manual) both provision
the user environment. Different subsystem ordering, neither knows the other's state.
This is a code organization tension, not a capability gap — both work.

### 5. system-config.json

The manifest module and CLI commands (bkt admin kargs, bkt admin systemd)
are implemented, but the manifest file hasn't been created yet. First use
of these commands will create it.

---

## Architectural Notes

### Three Configuration Tiers

1. **Baked** — in the Containerfile, requires reboot (packages, keyd, fonts, systemd)
2. **Bootstrapped** — applied on first login after deployment (flatpaks, extensions, gsettings, shims)
3. **Optional** — user-activated (appimages, homebrew, fetchbin, distrobox extras)

### Execution Phases

Subsystems declare execution phases: Infrastructure → Packages → Configuration.
Ordering is defined in SubsystemRegistry but apply/capture still use a hard-coded sequence.

### Three PR Modes

Commands that modify manifests support:

- Default: execute + create PR
- `--local`: execute only
- `--pr-only`: PR only

### Transparent Delegation

Every command has a target (Host, Dev, Either). Running a Host command in toolbox
auto-delegates via distrobox-host-exec, and vice versa.

---

## Documentation State

### Accurate

- README.md, VISION.md — align with "Why This Exists" framing (audit dated 2026-02-10)
- 10 canon RFCs in docs/rfcs/canon/ — verified against code
- docs/reports/2026-02-10-codebase-audit.md — comprehensive audit

### Needs Review

- ARCHITECTURE.md, WORKFLOW.md — may have stale references
- manifests/README.md — uses outdated command names

### Reference

- QUALITY.md — quality backlog
- PLAN.md — original roadmap (historical)
- docs/history/ — bootstrap and migration history
