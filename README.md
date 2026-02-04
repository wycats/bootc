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

## The Two Workflows

### Install via GUI → Capture to Manifest

The natural flow: use your system normally, then sync changes to the repo.

```bash
# 1. Install something via the GUI
#    - Install a Flatpak via GNOME Software
#    - Enable an extension via Extension Manager
#    - Change a setting via GNOME Settings

# 2. Capture all changes to manifests (run on host, not in toolbox)
bkt capture --apply

# 3. Review and commit
cd ~/Code/Config/bootc
git diff manifests/
git add -A && git commit -m "feat: add new flatpak/extension"
git push
```

### Add to Manifest → Apply to System

The declarative flow: add to manifests, which installs immediately.

```bash
# 1. Add to manifest (installs immediately if not present)
bkt flatpak add org.gnome.Boxes

# 2. Commit and push
git add -A && git commit -m "feat: add boxes"
git push
```

**Note:** `bkt flatpak add` both updates the manifest AND installs the app.
For extensions, `bkt extension add` adds to manifest and enables (if already installed).

See [WORKFLOW.md](docs/WORKFLOW.md) for detailed patterns.

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

# Preview what capture would sync (system → manifest)
bkt capture --dry-run

# Enter dev environment
toolbox enter

# Add a host command shim (from toolbox)
bkt shim add nmcli
```

## Repository Layout

```
├── Containerfile                 # Host image definition
├── toolbox/Containerfile         # Dev environment definition
├── manifests/
│   ├── flatpak-apps.json        # Apps to install at first login
│   ├── flatpak-remotes.json     # Flatpak remote repositories
│   ├── gnome-extensions.json    # Extensions to enable
│   ├── gsettings.json           # GNOME settings to apply
│   ├── host-shims.json          # Commands to delegate to host
│   └── ...                      # See manifests/README.md for full list
├── scripts/
│   ├── bootc-bootstrap          # First-login automation
│   └── bootc-apply              # Apply manifests to system
├── bkt/                          # Unified manifest CLI (Rust)
├── system/                       # Configs baked into /etc
├── skel/                         # Default dotfiles for new users
└── upstream/                     # Pinned upstream versions
```

## Development

We use [Lefthook](https://github.com/evilmartians/lefthook) to manage git hooks for locally verifying formatting, linting, and schema synchronization.

To set up your environment:

1. Ensure `lefthook` is installed.
2. Run `lefthook install` in the repository root.

This ensures your commits remain compliant with CI checks.

## Documentation

| Doc                                       | Purpose                      |
| ----------------------------------------- | ---------------------------- |
| [WORKFLOW.md](docs/WORKFLOW.md)           | Day-to-day usage patterns    |
| [MIGRATION.md](docs/MIGRATION.md)         | Switching from stock Bazzite |
| [ARCHITECTURE.md](docs/ARCHITECTURE.md)   | System design overview       |
| [VISION.md](docs/VISION.md)               | Project philosophy           |

## Philosophy

1. **The repo is the source of truth.** Your machine follows the repo.

2. **Local changes should flow upstream.** Like a change? One command makes it permanent.

3. **Updates are boring.** CI rebuilds. Machines pull. Reboots apply cleanly.

4. **Recovery is trivial.** Bad update? Reboot, pick previous deployment, done.

5. **Dev happens in the toolbox.** Host runs apps and games. Toolbox builds software.
