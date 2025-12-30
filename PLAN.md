# Plan: Bazzite → Workstation as Code

This document captures the migration plan and ongoing operational model for this "workstation as code" setup.

> ⚠️ **Security reminder:** This repo and its container images are public.
> Never commit secrets, API keys, passwords, or personal data.

## Vision

**"Workstation as Code"** — the entire system configuration lives in git, builds into immutable container images, and flows to machines through the standard bootc update mechanism.

**What this solves:**
- Eliminates configuration drift ("snowflake" machines)
- Makes system state reproducible and auditable (git history = system history)
- Provides safe migration with atomic rollbacks
- Stays current with upstream Bazzite while maintaining customizations

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                    Host (ghcr.io/wycats/bootc)                  │
│  • Immutable Bazzite base                                       │
│  • Desktop/gaming (Flatpak, Steam)                              │
│  • Update: bootc upgrade → reboot                               │
└─────────────────────────────────────────────────────────────────┘
         │ shares $HOME
         ▼
┌─────────────────────────────────────────────────────────────────┐
│              Toolbox (ghcr.io/wycats/bootc-toolbox)             │
│  • Ephemeral dev environment                                    │
│  • Nix, Cargo, proto in PATH                                    │
│  • Update: ujust toolbox-update (no reboot)                     │
└─────────────────────────────────────────────────────────────────┘
         │
         ▼
┌─────────────────────────────────────────────────────────────────┐
│                         $HOME (persists)                        │
│  • ~/.cargo, ~/.nix-profile, ~/.proto                           │
│  • ~/.local/toolbox/bin (toolbox-only PATH)                     │
│  • User data, dotfiles, app state                               │
└─────────────────────────────────────────────────────────────────┘
```

## Three-Tier Configuration Model

| Tier | When Applied | Mechanism | Examples |
|------|--------------|-----------|----------|
| **Baked** | Image build time | Containerfile | RPM packages, keyd config, fonts, ujust recipes |
| **Bootstrapped** | First login (idempotent) | systemd user oneshot | Flatpaks, GNOME extensions, toolbox creation |
| **Optional** | User-activated | `ujust enable-*` | Remote play, hardware-specific tweaks |

## Phase 0: Inventory (Done)

Capture baseline with `./scripts/build-system-profile`:
- RPM packages → decide what to bake vs leave as layered
- Flatpaks → add to manifests
- GNOME extensions → add to manifests
- /etc diffs → incorporate into image or accept as local state

## Phase 1: GHCR Publishing (Done)

### Setup Checklist

1. **Workflow permissions** — `.github/workflows/build.yml` has `packages: write`

2. **GHCR package settings** (https://github.com/users/wycats/packages/container/bootc/settings):
   - Add `bootc` repo under "Manage Actions access" with **Write** role
   - Set visibility to **Public** (required for unauthenticated `bootc upgrade`)

3. **Validation** — push a commit and confirm `:latest` digest updates in GHCR

### What Gets Published

| Image | Purpose | Update Trigger |
|-------|---------|----------------|
| `ghcr.io/wycats/bootc:latest` | Host OS | Push to main, upstream digest change, nightly rebuild |
| `ghcr.io/wycats/bootc:sha-<gitsha>` | Pinned builds | Every push |
| `ghcr.io/wycats/bootc-toolbox:latest` | Dev environment | Push to main |

## Phase 2: Host Migration

### First Switch

```bash
# Capture current state (for reference if needed)
./scripts/build-system-profile --output system_profile.json

# Switch to the custom image
sudo bootc switch --transport registry ghcr.io/wycats/bootc:latest

# Reboot to apply
sudo reboot
```

### Rollback

If something goes wrong:
- **At boot:** Select previous deployment from boot menu
- **After boot:** `sudo bootc rollback` then reboot

### Post-Switch Validation

```bash
ujust check-drift   # Should show only optional-tier differences
```

- [ ] Home data present
- [ ] Flatpaks working
- [ ] `bootc-bootstrap` ran (check `~/.local/state/bootc-bootstrap/`)
- [ ] Toolbox created and functional
- [ ] `ujust check-drift` exits 0

## Phase 3: Normal Operations

### Host Updates (requires reboot)

```
Edit repo → push to main
    ↓
GitHub Actions builds and pushes :latest
    ↓
On host: sudo bootc upgrade
    ↓
Reboot to apply
```

### Toolbox Updates (no reboot)

```
Edit toolbox/Containerfile → push to main
    ↓
GitHub Actions builds and pushes :latest
    ↓
On host: ujust toolbox-update
    ↓
Next `toolbox enter dev` uses new image
```

The toolbox container is ephemeral. `$HOME` (including `~/.cargo`, `~/.nix-profile`, `~/.local/toolbox/bin`) persists across recreations.

## Toolbox Strategy

### Design Principles

1. **Toolbox = primary dev environment** (like WSL on Windows)
2. **PATH via container metadata** — works regardless of entry method (fixes VS Code issues)
3. **`~/.local/toolbox/bin`** — toolbox-specific scripts, in toolbox PATH only
4. **Ephemeral container, persistent home** — recreate on image update, data survives

### Toolbox Container Name

Fixed name `dev` so integrations (ddterm, etc.) keep working:
```bash
toolbox enter dev
```

### VS Code Integration

The `code` wrapper in `~/.local/toolbox/bin/code`:
```bash
# Inside toolbox, launches host VS Code attached to this container
flatpak-spawn --host /usr/bin/code --folder-uri "vscode-remote://attached-container+${hex_name}${folder}"
```

## Bootstrap Service

`bootc-bootstrap.service` runs on login and handles:

1. Flatpak remotes (from manifests)
2. Flatpak apps (from manifests)
3. GNOME extensions (from manifests)
4. Toolbox creation/recreation (if image digest changed)
5. `~/.local/toolbox/bin` setup

Idempotent and hash-cached — only re-runs when manifests change.

## Optional Features

Shipped in image but disabled by default:

| Feature | Enable | Disable | Status |
|---------|--------|---------|--------|
| Remote Play (tty2 + Steam gamepad UI) | `ujust enable-remote-play` | `ujust disable-remote-play` | `ujust remote-play-status` |

## Operational Tools

| Tool | Purpose | Command |
|------|---------|--------|
| **check-drift** | Verify system matches manifests | `ujust check-drift` |
| **bootc-bootstrap** | Re-apply bootstrap tier | `ujust bootc-bootstrap` |
| **build-system-profile** | Full state dump (dev) | `./scripts/build-system-profile` |

## Future Considerations

- **Secrets handling** — pattern for API keys, credentials (1Password CLI integration?)
- **Skel sync for existing users** — mechanism to update dotfiles in existing home directories
- **Multi-machine variants** — if needed, handle via ujust runtime detection rather than separate builds
- **Toolbox Containerfile** — currently referenced but not implemented
