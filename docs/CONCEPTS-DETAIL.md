# Under the Hood: How bkt Tames the Linux Packaging Zoo

_The architecture behind the simplicity_

_Follows [CONCEPTS.md](CONCEPTS.md) — read that first._

---

## Part 1: The Packaging Zoo

CONCEPTS.md told a clean story: install things, capture them, reproduce
them. But it glossed over a messy reality.

On a modern Linux desktop, "installing software" means at least eight
different things:

| Method          | Example             | Where it lives                | Who manages it                 |
| --------------- | ------------------- | ----------------------------- | ------------------------------ |
| Flatpak         | Firefox, Steam      | Flatpak runtime               | `flatpak` CLI / GNOME Software |
| System RPM      | gcc, htop           | `/usr` (immutable)            | `rpm-ostree` / Containerfile   |
| AppImage        | Obsidian            | `~/Applications/`             | GearLever                      |
| Homebrew        | jq, gh              | `/home/linuxbrew/`            | `brew`                         |
| Distrobox       | cargo, node         | Dev container                 | `distrobox` exports            |
| GitHub release  | starship, lazygit   | `/usr/bin/` (image)           | Upstream pinning               |
| GNOME Extension | Dash to Dock        | `~/.local/share/gnome-shell/` | Extension Manager              |
| GSettings       | key bindings, theme | dconf database                | `gsettings` / GNOME Settings   |

Each has different installation semantics, different update mechanisms,
different locations, and different ideas about what "uninstall" means.

**bkt doesn't replace any of these.** It captures and reproduces the
_result_ of using them.

---

## Part 2: One Model, Many Domains

CONCEPTS.md showed four capture domains: Flatpak, Extensions,
GSettings, and dev tools. The real number is eight — and each one
has its own idea of what "capture" means.

**Flatpaks:** Capture means recording the app ID, remote, scope
(user vs system), branch, commit hash, and any permission overrides.
This is enough to reproduce the exact installation:

```json
{
  "id": "org.mozilla.firefox",
  "remote": "flathub",
  "scope": "system",
  "branch": "stable",
  "commit": "a1b2c3d4e5f6"
}
```

**AppImages:** Capture means recording the app as managed by GearLever.
AppImages are self-contained binaries, so the manifest just tracks
which ones you've chosen to keep.

**Homebrew:** Capture means recording the formula name. Homebrew
handles its own dependency resolution — bkt just needs the leaf
packages.

**GSettings:** Capture means recording specific key/value pairs you've
explicitly chosen to track. Unlike other subsystems, GSettings capture
requires a schema filter — you specify _which_ schema to capture from.
This is intentional: you don't want to capture window positions or
recent file lists. Because of this, GSettings is excluded from the
`bkt capture` meta-command and must be captured individually.

**GNOME Extensions:** Capture means recording the extension UUID and
whether it's enabled. Installation happens at bootstrap time; day-to-day
sync just toggles enable/disable.

**Distrobox:** Capture means recording the container configuration —
image, exported binaries, and environment. The dev container is where
your build tools live; capture ensures you can recreate it.

Each of these is a **subsystem** — a domain with its own capture logic,
its own manifest format, and its own apply behavior. bkt provides the
framework; each subsystem provides the domain knowledge.

---

## Part 3: The Boundary Problem

CONCEPTS.md introduced the dev sandbox — a container where you
experiment with dev tools, invisible to your daily workflow. But it
didn't explain *how* that invisibility works.

On an atomic Linux desktop, software lives in **three different
worlds**:

```
┌──────────────────────────────────────────────────┐
│                    The Host                        │
│  Immutable /usr, your $HOME, system services      │
│                                                    │
│  ┌──────────────────┐  ┌──────────────────────┐  │
│  │  Flatpak Sandbox  │  │   Dev Container      │  │
│  │                    │  │                      │  │
│  │  Firefox, Steam,   │  │  cargo, node, gcc,   │  │
│  │  VS Code           │  │  your dev tools      │  │
│  └──────────────────┘  └──────────────────────┘  │
└──────────────────────────────────────────────────┘
```

Each world has its own filesystem, its own binaries, its own `PATH`.
But you want to type `cargo build` in a terminal and have it just work,
regardless of which world `cargo` lives in.

**Shims** solve this. A shim is a tiny script on the host that
delegates to the right world:

```bash
# You type:
$ cargo build

# What actually happens:
# 1. Host finds ~/.local/bin/distrobox/cargo (a shim)
# 2. Shim delegates to the dev container
# 3. Container's cargo runs with your $HOME mounted
# 4. You see the output as if cargo were local
```

This is why the dev sandbox from CONCEPTS.md feels invisible —
every exported command has a shim that handles the boundary crossing
for you.

There's a second kind of shim for the reverse direction. Inside the
dev container, you sometimes need to run host commands — `bootc`,
`systemctl`, `flatpak`, `podman`. These **host shims** live inside
the container and delegate to the host via `flatpak-spawn --host`.

**bkt manages both kinds of shims.** Distrobox export shims let you
use container tools from the host. Host shims let you use host tools
from the container. `bkt apply` ensures both sets are in sync.

---

## Part 4: Pinning and Trust

Some tools don't come from a package manager at all. They're standalone
binaries downloaded from GitHub releases:

- **starship** (shell prompt)
- **lazygit** (terminal git UI)
- **getnf** (nerd font installer)

These are baked into the OS image, not installed at runtime. But how
do you track which version you have? How do you verify the download
hasn't been tampered with?

**Upstream pinning** solves this:

```json
{
  "name": "starship",
  "source": {
    "type": "github",
    "repo": "starship/starship",
    "asset_pattern": "starship-x86_64-unknown-linux-gnu.tar.gz"
  },
  "pinned": {
    "version": "v1.21.1",
    "url": "https://github.com/starship/starship/releases/download/v1.21.1/...",
    "sha256": "abc123..."
  }
}
```

The manifest tracks the _intent_ (`asset_pattern` — what to look for)
and the _resolved state_ (`pinned` — exactly what to download). This
is a two-step process:

1. `bkt upstream pin <name> <version>` — sets the version and clears
   the checksum
2. `bkt upstream lock` — resolves the URL and computes the SHA256

The Containerfile build then fetches from that exact URL with integrity
verification.

**No API calls at build time.** The pin is resolved once, committed to
the repo, and consumed deterministically by the build.

---

## Part 5: The Containerfile as Output

CONCEPTS.md mentioned that system packages go through the Containerfile.
But it didn't explain the key insight:

> **The Containerfile is generated, not written.**

You never edit the Containerfile directly. It's produced by
`bkt containerfile generate` from your manifest files:

```
manifests/system-packages.json    ──┐
manifests/image-config.json       ──┤
manifests/external-repos.json     ──┼──▶  bkt containerfile generate  ──▶  Containerfile
upstream/manifest.json            ──┤
manifests/host-shims.json         ──┘
```

Every line in the Containerfile traces back to a manifest. If it's not
in a manifest, it doesn't belong in the Containerfile. This is enforced
by CI — if the committed Containerfile doesn't match what the generator
would produce, the build fails.

**Why does this matter?**

Because it means the Containerfile is a **build artifact**, not a source
file. You interact with manifests (structured JSON with schemas and
validation). The Containerfile is the compiled output — optimized for
Docker's layer caching, with parallel build stages, but not something
you need to understand or maintain.

```bash
# You do this:
$ bkt system add htop

# bkt does this:
# 1. Adds "htop" to system-packages.json
# 2. Regenerates the Containerfile
# 3. Creates a PR with both changes
# 4. CI builds the new image
# 5. You reboot into it
```

---

## Part 6: Resource Controls

Some applications are resource hogs. VS Code and Electron-based
browsers can consume unbounded memory, and on Linux, an out-of-memory
kill can take down your entire desktop session.

bkt solves this with **wrapper binaries** — tiny compiled programs that
launch applications under systemd resource controls:

```
You click "VS Code" in GNOME
        │
        ▼
/usr/bin/code (wrapper binary)
        │
        ▼
systemd-run --user --slice=app-vscode.slice --scope ...
        │
        ▼
/usr/share/code/bin/code (the real VS Code)
```

The wrapper:

1. Detects if it's being called from VS Code's remote CLI (and passes
   through directly if so)
2. Launches the real binary under a systemd scope
3. The scope belongs to a **slice** with memory limits

If VS Code exceeds its memory budget, systemd kills _VS Code_ — not
your GNOME Shell, not your terminal, not your other apps.

**You never see the wrapper.** It replaces the original binary path,
so GNOME's app launcher, terminal commands, and `.desktop` files all
go through it automatically.

The wrapper source code is generated from manifest data and compiled
inside the Containerfile build — no external build step, no committed
binaries. It's fully manifest-driven.

---

## Part 7: Diagnostics

With all these moving parts — shims, containers, Flatpaks, wrappers,
manifests — how do you know if everything is working?

```bash
$ bkt doctor
✓ gh CLI: gh version 2.x.x
✓ gh auth: Logged in to github.com
✓ git: git version 2.x.x
✓ git user.name: configured
✓ git user.email: configured
✓ repo.json: found
✓ Distrobox shims directory exists
✓ Distrobox wrappers present
✓ All cargo bin exports have shims
✓ cargo resolves to distrobox shim
✓ node resolves to distrobox shim
✓ pnpm resolves to distrobox shim
```

`bkt doctor` checks every layer of the system:

- Are the PR workflow prerequisites in place (gh, git, repo config)?
- Are the dev container shims in place?
- Do dev tools resolve to the right binaries (distrobox, not host)?
- Do cargo bin exports have corresponding shims?

When something breaks, doctor tells you _what_ and _how to fix it_.

---

## Part 8: The Full Architecture

Putting it all together:

```
┌─────────────────────────────────────────────────────────────┐
│                     Your Repository                          │
│                                                              │
│  manifests/              upstream/           system/          │
│  ├─ flatpak-apps.json    ├─ manifest.json    ├─ systemd/     │
│  ├─ flatpak-remotes.json │                   ├─ keyd/        │
│  ├─ gnome-extensions.json│                   └─ ...          │
│  ├─ gsettings.json       │                                   │
│  ├─ system-packages.json │                                   │
│  ├─ external-repos.json  │                                   │
│  ├─ homebrew.json        │                                   │
│  ├─ appimage-apps.json   │                                   │
│  ├─ host-shims.json      │                                   │
│  ├─ host-binaries.json   │                                   │
│  ├─ toolbox-packages.json│                                   │
│  ├─ image-config.json    │                                   │
│  ├─ base-image-          │                                   │
│  │  assumptions.json     │                                   │
│  └─ distrobox.json       │                                   │
│                                                              │
│  Containerfile  ◄── generated from manifests                 │
└──────────────────────────┬──────────────────────────────────┘
                           │
              ┌────────────┼────────────┐
              ▼            ▼            ▼
        CI builds     bkt apply    bkt capture
        OS image      (runtime)    (snapshot)
              │            │            │
              ▼            ▼            ▼
        Base image    Synced apps   Updated
        with system   extensions    manifests
        packages      settings
```

**Manifests are the source of truth.** Everything else — the
Containerfile, the built image, the running system — is derived from
them.

---

## Summary

| Concept                     | What it means                                                        |
| --------------------------- | -------------------------------------------------------------------- |
| **Subsystem**               | A domain (Flatpak, GSettings, etc.) with its own capture/apply logic |
| **Shim**                    | A bridge that makes commands work across container boundaries        |
| **Upstream pinning**        | Version-locked external binaries with integrity verification         |
| **Generated Containerfile** | The build recipe is an output, not a source file                     |
| **Wrapper**                 | A compiled binary that adds resource controls to an application      |
| **Doctor**                  | System-wide diagnostics that check every layer                       |

---

## The Takeaway

CONCEPTS.md told you the _what_: capture your desktop, reproduce it
anywhere.

This document told you the _how_: bkt handles eight different packaging
domains, bridges three different execution environments, pins external
dependencies with cryptographic verification, generates its own build
recipe from structured data, and wraps resource-hungry apps in systemd
controls.

**All of this is invisible when it works.** You just use your desktop.
bkt handles the complexity.
