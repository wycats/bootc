# Your Personal Linux Distribution

> ⚠️ **This repository and its container images are public.** Do not commit
> secrets, passwords, API keys, personal data, or any sensitive information.

This repository **is** your operating system. Not config management. Not dotfiles. The complete definition of a Linux distribution that builds into bootable images and flows to your machines through standard update mechanisms.

You're not a user of Bazzite. You're the maintainer of a distribution that happens to be based on Bazzite.

## The Core Loop

```
┌─────────────────────────────────────────────────────────────────────┐
│                         This Repository                             │
│                                                                     │
│  Containerfile           What packages, fonts, configs are baked   │
│  manifests/*.json        What apps, extensions get bootstrapped    │
│  toolbox/Containerfile   What your dev environment contains        │
└─────────────────────────────────────────────────────────────────────┘
                               │
                               ▼  push (or merge PR)
┌─────────────────────────────────────────────────────────────────────┐
│                        GitHub Actions                               │
│                                                                     │
│  Builds your images, pushes to ghcr.io                             │
│  Triggers: your commits, upstream updates, nightly                  │
└─────────────────────────────────────────────────────────────────────┘
                               │
                               ▼  images published
┌─────────────────────────────────────────────────────────────────────┐
│                        Your Machines                                │
│                                                                     │
│  Pull updates automatically (uupd) or manually (bootc upgrade)     │
│  Reboot to apply                                                   │
└─────────────────────────────────────────────────────────────────────┘
```

**Edit repo → push → CI builds → machines update.** That's the whole model.

## Two Images, One Home

| Image                          | What it is                     | Update                   |
| ------------------------------ | ------------------------------ | ------------------------ |
| `ghcr.io/wycats/bootc`         | Host OS (boots your machine)   | `bootc upgrade` + reboot |
| `ghcr.io/wycats/bootc-toolbox` | Dev container (where you code) | recreate toolbox         |

Both are disposable. `$HOME` persists across everything.

## Local Changes That Become Permanent

The key workflow insight: for things that don't require a reboot, you want **immediate local effect** AND **a PR to make it canonical**.

```bash
# Today: local-only
shim add nmcli

# Vision: apply locally AND open PR
shim add --pr nmcli
```

This applies to:

- Host command shims
- Flatpak apps
- GNOME extensions
- GSettings

One command, two effects: works now, becomes permanent via PR merge.

## Three Tiers of Configuration

| Tier             | Applied        | Requires reboot | Examples                        |
| ---------------- | -------------- | --------------- | ------------------------------- |
| **Baked**        | Image build    | Yes             | Packages, fonts, system units   |
| **Bootstrapped** | First login    | No              | Flatpaks, extensions, shims     |
| **Optional**     | User-activated | Depends         | Remote play, HW-specific tweaks |

## Quick Start

```bash
# Update your OS
sudo bootc upgrade && systemctl reboot

# Check for drift from declared state
check-drift

# Enter dev environment
toolbox enter

# Add a host command shim (from toolbox)
shim add nmcli
```

## Repository Layout

```
├── Containerfile                 # Host image definition
├── toolbox/Containerfile         # Dev environment definition
├── manifests/
│   ├── flatpak-apps.json        # Apps to install at first login
│   ├── gnome-extensions.json    # Extensions to enable
│   ├── host-shims.json          # Commands to delegate to host
│   └── gsettings.json           # GNOME settings to apply
├── scripts/
│   ├── bootc-bootstrap          # First-login automation
│   ├── check-drift              # Drift detection
│   └── shim                     # Shim management CLI
├── system/                       # Configs baked into /etc
├── skel/                         # Default dotfiles for new users
└── upstream/                     # Pinned upstream versions
```

## Documentation

| Doc                               | Purpose                      |
| --------------------------------- | ---------------------------- |
| [WORKFLOW.md](docs/WORKFLOW.md)   | Day-to-day usage patterns    |
| [MIGRATION.md](docs/MIGRATION.md) | Switching from stock Bazzite |
| [PLAN.md](PLAN.md)                | Architecture decisions       |

## Philosophy

1. **The repo is the source of truth.** Your machine follows the repo.

2. **Local changes should flow upstream.** Like a change? One command makes it permanent.

3. **Updates are boring.** CI rebuilds. Machines pull. Reboots apply cleanly.

4. **Recovery is trivial.** Bad update? Reboot, pick previous deployment, done.

5. **Dev happens in the toolbox.** Host runs apps and games. Toolbox builds software.
