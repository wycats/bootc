# Vision: Capture-First Configuration Management

## The Core Loop

`bkt` implements a **capture-first** workflow for managing a Linux workstation. The philosophy is:

1. **Capture** is the daily verb — you make changes interactively, then capture them into manifests
2. **Apply** is for bootstrap and recovery — it reconstructs your environment from manifests

This inverts the traditional "edit config, then apply" model. Instead, your running system is the source of truth, and manifests are derived artifacts that enable reproducibility.

## Two-Tier Architecture

The system has two distinct tiers with different change semantics:

### Tier 1: Immutable Root Filesystem (bootc)

Changes here require a **reboot** to take effect:

- System packages (`dnf install`)
- Kernel arguments
- Systemd units
- System configuration in `/etc`

These are managed via the main `Containerfile` and `bootc` image builds.

### Tier 2: User-Space (Live Changes)

Changes here take effect **immediately**:

- Flatpaks
- AppImages
- GNOME extensions
- GSettings
- Distrobox containers

These are the primary domain for the capture-first workflow.

## The Immediate Development Axiom

> **The running system is the source of truth. Changes should take effect immediately wherever possible, then be captured into manifests so they persist across reboots and can be replayed on fresh systems.**

## The Upstream Trust Axiom

> **Prefer the distro's standard repositories. Only pin specific versions or use external sources when the package is missing, broken, or significantly outdated in the upstream distribution.**

**Implication:** If a package exists in Fedora, use `dnf install`. Consuming the standard generic artifact is a feature, not a bug. If you must pin something (e.g. via `upstream/manifest.json` or a direct URL download), you **MUST** document exactly *why* the upstream package was insufficient.

This principle means:

1. **Make changes interactively** using familiar tools (GUI apps, CLI commands)
2. **Changes take effect immediately** on the running system
3. **Capture to manifests** records the change for persistence and reproducibility
4. **Manifests are derived artifacts**, not the primary interface

### The Persistence Model

The two-tier architecture can be further subdivided by persistence characteristics:

| Domain                           | Immediate Mechanism                                        | Persistence Mechanism                    | Survives Reboot?            |
| -------------------------------- | ---------------------------------------------------------- | ---------------------------------------- | --------------------------- |
| **Tier 2: User-space**           | Direct tools (GNOME Software, Extension Manager, Settings) | `bkt capture` → manifest                 | ✅ Yes                      |
| **Tier 1b: usr binaries**        | `bootc usr-overlay`                                        | Encode in Containerfile → rebuild image  | ❌ No (until image rebuild) |
| **Tier 1a: Base image packages** | ❌ None (deferred)                                         | `bkt system add` → PR → rebuild → reboot | ❌ No (requires full cycle) |
| **Distrobox container**          | `bkt dev install` / direct `dnf`                           | `bkt capture` → manifest                 | ✅ Yes                      |
| **Host binaries (fetchbin)**     | `bkt fetchbin install`                                     | Manifest in `~/.local/share/fetchbin`    | ✅ Yes                      |

### The Three Patterns

#### Pattern 1: Native Tool + Capture

For domains with existing GUI/CLI tools, the workflow is:

```
User action (GNOME Software, Extension Manager, Settings)
    ↓
Change takes effect immediately
    ↓
bkt capture → manifest updated
    ↓
git commit/push (manual or auto)
```

**Examples:** Flatpaks, GNOME extensions, gsettings

#### Pattern 2: bkt Command = Execute + Record

For domains without native tools, or where we want atomic execute+record:

```
bkt <subsystem> install <thing>
    ↓
Executes immediately (dnf, fetchbin, etc.)
    ↓
Records to manifest automatically
    ↓
Optionally creates PR
```

**Examples:** `bkt dev install`, `bkt fetchbin install`, `bkt flatpak add`

This is the **command punning** philosophy: commands that execute immediately AND propagate changes to the distribution. The `--pr` / `--local` / `--pr-only` flags control propagation:

- Default: Execute + create PR
- `--local`: Execute only (for testing)
- `--pr-only`: PR only (for remote preparation)

#### Pattern 3: Overlay + Encode

For Tier 1 changes that need immediate testing:

```
bootc usr-overlay (enable writable /usr)
    ↓
Make changes (copy binaries, edit scripts)
    ↓
Test on running system
    ↓
Encode in Containerfile/scripts
    ↓
Rebuild image → changes persist
```

**Examples:** Testing new bkt versions, hotfixing system scripts

See [RFC 0034: Usroverlay Integration](rfcs/0034-usroverlay-integration.md) for details.

### The Guiding Principle

> **"You are maintaining your own distribution. Every local change should persist. Every persistent change should be auditable."**

This means:

- **No silent drift** — Changes made interactively are captured, not lost
- **No manual bookkeeping** — The system tracks what you did
- **No reboot surprises** — What you see now is what you get after reboot (via capture mechanism)
- **Full reproducibility** — A fresh system can be reconstructed from manifests

### Technical Constraints

#### No Custom Python Scripts

**Axiom**: This repository must not contain or depend on custom Python scripts.

All tooling in this repository is implemented in Rust (the `bkt` CLI) or minimal shell scripts for bootstrap/glue. Python is explicitly excluded because:

1. **Dependency complexity** — Python scripts require a Python runtime and often additional packages, adding fragile dependencies to the immutable image
2. **Two languages, one job** — Rust already handles all complex logic; adding Python creates maintenance burden and context-switching
3. **Reproducibility** — Python version and package management (pip, venv, system packages) introduces variability that conflicts with the immutable image philosophy
4. **Shell suffices for glue** — Simple orchestration tasks use POSIX shell; complex logic belongs in Rust

**Allowed:**

- Rust code (`bkt/`, `fetchbin/`)
- Minimal shell scripts (`scripts/bootc-apply`, `scripts/bootc-bootstrap`)
- Containerfile/Dockerfile syntax
- JSON manifests

**Not allowed:**

- Python scripts (`.py` files)
- Dependencies on Python packages
- Shelling out to Python from Rust or shell scripts

## The Distrobox Strategy

Development tools (compilers, runtimes, package managers) live in a distrobox container:

- **`bootc-dev`** — The primary development container
- **Host exports** — Selected binaries are exported to `/usr/bin` on the host

### Toolchain Philosophy

**Axiom**: Toolchain _installers_ are baked into the image; toolchain _state_ lives in the user's home directory.

| Component       | Lives In                        | Why                                             |
| --------------- | ------------------------------- | ----------------------------------------------- |
| `rustup` binary | Image (`/usr/local/bin/rustup`) | Available immediately on container creation     |
| `~/.rustup/`    | User's home (bind-mounted)      | Toolchains persist across container rebuilds    |
| `proto` binary  | Image (`/usr/local/bin/proto`)  | Available immediately on container creation     |
| `~/.proto/`     | User's home (bind-mounted)      | Node versions persist across container rebuilds |

### Init Hooks Axiom

**Init hooks must be idempotent AND bring the box up to date with what would happen on a fresh bootstrap.**

This means:

1. **Idempotent** — Running the hook twice produces the same result as running it once
2. **Bootstrap-equivalent** — After the hook runs, the container is in the same state as a freshly provisioned machine

#### Valid Init Hooks

```bash
# ✅ rustup update - idempotent, ensures latest stable toolchain
rustup update stable

# ✅ proto install - idempotent, ensures node is available
proto install node

# ✅ Set default toolchain - idempotent, ensures consistency
rustup default stable
```

#### Invalid Init Hooks

```bash
# ❌ Appending to a file - NOT idempotent (grows on each run)
echo "export PATH=$PATH" >> ~/.bashrc

# ❌ Installing a specific version - NOT bootstrap-equivalent
# (a fresh bootstrap would get the latest, not this pinned version)
rustup install 1.70.0

# ❌ Downloading something to a temp location - NOT bootstrap-equivalent
curl -o /tmp/setup.sh https://example.com/setup.sh
```

The key insight: if you ran `bkt apply` on a brand new machine, the init hooks should leave you in an identical state to an existing machine that has been using those hooks for months.

### Toolbox Image Update Workflow

The intended workflow for changing the toolbox image is:

1. Get the toolbox behaving correctly **locally**.
2. Update [toolbox/Containerfile](toolbox/Containerfile) to encode exactly what worked.
3. Open a PR and let CI rebuild/publish the image.
4. Validate that upgrading the distrobox works end-to-end once the PR image is available.

## Export Paths

The distrobox exports binaries from the container to the host. For toolchains installed via rustup and proto:

```json
{
  "bins": {
    "from": ["~/.cargo/bin", "~/.proto/bin", "~/.proto/shims"]
  }
}
```

Why three paths?

- `~/.cargo/bin` — Rust toolchain binaries (`cargo`, `rustc`, `rustfmt`, etc.)
- `~/.proto/bin` — Proto-managed tool binaries (`node`, `npm`, etc.)
- `~/.proto/shims` — Proto shims that delegate to the correct version

## Host PATH Architecture

**Axiom**: PATH is set once, declaratively, via `environment.d`. Shell rc files must not modify PATH.

> **Note**: This axiom supersedes earlier guidance (e.g., RFC-0017) that suggested `PATH="$HOME/.local/bin:$PATH"`. That pattern reintroduces PATH inheritance and is now deprecated.

### Why `environment.d`?

The traditional approach of setting PATH in `.bashrc` or `.profile` is fragile:

| Context                     | Sources `.bashrc`? | Sources `.profile`? |
| --------------------------- | ------------------ | ------------------- |
| Interactive terminal        | ✅                 | Depends             |
| VS Code integrated terminal | ❌ Often not       | ❌                  |
| Systemd user services       | ❌                 | ❌                  |
| GUI apps (GNOME)            | ❌                 | ❌                  |
| SSH sessions                | Depends            | ✅                  |

This leads to "works in my terminal but not in VS Code" bugs.

### The Solution

Use `~/.config/environment.d/10-distrobox-exports.conf`:

```bash
# Complete PATH for host environment.
# distrobox shims take priority, then user bins, then system bins.
# NOTE: Do NOT reference $PATH here - we define the complete path to avoid
# inheriting stale or unwanted entries (like ~/.cargo/bin from toolbox).
PATH="$HOME/.local/bin/distrobox:$HOME/.local/bin:/usr/local/sbin:/usr/local/bin:/usr/bin"
```

Key principles:

1. **Complete, not incremental** — Don't append to `$PATH`; define the full path
2. **Shims first** — `~/.local/bin/distrobox` contains shims that route to the container
3. **No toolchain paths** — `~/.cargo/bin`, `~/.proto/*` are NOT in host PATH; shims handle them
4. **Shell-agnostic** — Works for bash, zsh, nushell, fish, and GUI apps

### What Goes Where

| Directory                                       | Purpose                             | In host PATH? |
| ----------------------------------------------- | ----------------------------------- | ------------- |
| `~/.local/bin/distrobox/`                       | Distrobox shims (cargo, node, etc.) | ✅ First      |
| `~/.local/bin/`                                 | User binaries, direct symlinks      | ✅ Second     |
| `/usr/local/sbin`, `/usr/local/bin`, `/usr/bin` | System binaries                     | ✅ Last       |
| `~/.cargo/bin/`                                 | Rust toolchain (used by container)  | ❌ Never      |
| `~/.proto/bin/`, `~/.proto/shims/`              | Node toolchain (used by container)  | ❌ Never      |

### Shell RC Files

Shell rc files (`.bashrc`, `.bash_profile`, `.profile`, `.zshrc`) should **not** modify PATH. They may:

- Source system defaults (`/etc/bashrc`)
- Set non-PATH environment variables (`GPG_TTY`, `EDITOR`)
- Define aliases and functions
- Hand off to another shell (e.g., `exec nu`)

### Cargo/Rustup Integration

The `~/.cargo/env` file traditionally adds `~/.cargo/bin` to PATH. This must be neutered:

```bash
# ~/.cargo/env
# This file is intentionally empty.
# Toolchains are accessed via distrobox shims, not directly from the host.
# See: ~/.local/bin/distrobox/cargo
```

### Verification

After login, verify PATH is correct:

```bash
# Should show shims directory first
echo $PATH | tr ':' '\n' | head -5

# Should resolve to shim, not ~/.cargo/bin
which cargo  # Should be ~/.local/bin/distrobox/cargo
```

## Future Work

### Manifest-Driven Toolbox Containerfile

Currently, the `toolbox/Containerfile` is manually maintained. It should eventually use the same managed-sections pattern as the main `Containerfile`:

```dockerfile
# === TOOLBOX_PACKAGES (managed by bkt) ===
RUN dnf install -y \
    git \
    gcc \
    ...
# === END TOOLBOX_PACKAGES ===
```

This would allow `bkt containerfile` to manage both the base image and the toolbox image from their respective manifests.

### Auto-Capture Service

A systemd user service that periodically captures drift and commits changes:

```
~/.config/systemd/user/bkt-capture.timer
```

See [RFC 0007: Drift Detection](rfcs/0007-drift-detection.md) for details.
