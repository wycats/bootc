# Workstation as Code

> ⚠️ **This repository and its container images are public.** Do not commit
> secrets, passwords, API keys, personal data, or any sensitive information.
> Machine-specific files like `system_profile.json` are gitignored for this
> reason.

This repo defines a complete, reproducible Linux workstation built on [Bazzite](https://bazzite.gg) (uBlue/Fedora Atomic). The entire system configuration lives in git, builds into immutable container images, and flows to machines through the standard bootc update mechanism.

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                    Host (ghcr.io/wycats/bootc)                  │
│  • Immutable Bazzite base                                       │
│  • Desktop apps via Flatpak                                     │
│  • Gaming (Steam, gamescope)                                    │
│  • Update: bootc upgrade → reboot                               │
└─────────────────────────────────────────────────────────────────┘
         │ shares $HOME
         ▼
┌─────────────────────────────────────────────────────────────────┐
│              Toolbox (ghcr.io/wycats/bootc-toolbox)             │
│  • Ephemeral dev environment (like WSL)                         │
│  • Nix, Cargo, proto toolchains                                 │
│  • GPG signing for git commits                                  │
│  • VS Code attaches from host                                   │
│  • Update: ujust toolbox-update (no reboot needed)              │
└─────────────────────────────────────────────────────────────────┘
```

**Key insight:** Both images are ephemeral; `$HOME` persists. Toolbox-specific user binaries live in `~/.local/toolbox/bin` (in toolbox PATH only).

## Three-Tier Configuration Model

| Tier | When | Mechanism | Examples |
|------|------|-----------|----------|
| **Baked** | Image build | Containerfile | RPM packages, keyd config, ujust recipes, fonts |
| **Bootstrapped** | First login | systemd user oneshot | Flatpaks, GNOME extensions, toolbox setup |
| **Optional** | User-activated | `ujust enable-*` | Remote play, hardware-specific tweaks |

## Update Flows

**Host image** (requires reboot):
```
Edit repo → push → GitHub Actions builds → bootc upgrade → reboot
```

**Toolbox image** (no reboot):
```
Edit repo → push → GitHub Actions builds → ujust toolbox-update → next shell
```

## Quick Reference

| What | Where |
|------|-------|
| Host image definition | [Containerfile](Containerfile) |
| Toolbox image definition | [toolbox/Containerfile](toolbox/Containerfile) |
| Flatpak apps | [manifests/flatpak-apps.txt](manifests/flatpak-apps.txt) |
| GNOME extensions | [manifests/gnome-extensions.txt](manifests/gnome-extensions.txt) |
| ujust recipes | [ujust/60-custom.just](ujust/60-custom.just) |
| Migration plan | [PLAN.md](PLAN.md) |
| Package analysis | [docs/ANALYSIS.md](docs/ANALYSIS.md) |

## Toolbox as Primary Dev Environment

The toolbox is where development happens. Terminals (ddterm, etc.) are configured to run `toolbox enter dev` as the shell command.

**Why this works:**
- PATH is set via container metadata, not shell init — works regardless of how you enter
- `~/.local/toolbox/bin` holds toolbox-specific scripts (like `code` wrapper)
- VS Code on host attaches to the container via "Remote - Containers"
- Home directory is shared, so files persist across toolbox recreation

**The `code` wrapper** (in `~/.local/toolbox/bin/code`):
```bash
# Launches host VS Code attached to this container
flatpak-spawn --host /usr/bin/code --folder-uri "vscode-remote://attached-container+${hex_name}${folder}"
```

## Bootstrap Service

On login, `bootc-bootstrap.service` ensures the system matches manifests:

1. **Flatpak remotes** — configured from [manifests/flatpak-remotes.txt](manifests/flatpak-remotes.txt)
2. **Flatpak apps** — installed/updated from [manifests/flatpak-apps.txt](manifests/flatpak-apps.txt)
3. **GNOME extensions** — installed/enabled from [manifests/gnome-extensions.txt](manifests/gnome-extensions.txt)
4. **Toolbox** — recreated if image digest changed (preserves container name)
5. **Toolbox bin** — ensures `~/.local/toolbox/bin/code` exists

The service is idempotent and hash-cached — it only runs when manifests change.

## Optional Features

Shipped in the image but disabled by default. Activate via ujust:

| Feature | Enable | Disable |
|---------|--------|---------|
| Remote Play (tty2 + Steam gamepad UI) | `ujust enable-remote-play` | `ujust disable-remote-play` |

## System Profile (Migration Preflight)

Capture current machine state for migration planning:

```bash
./scripts/build-system-profile --output system_profile.json --text-output system_profile.txt
```

Query with jq:
```bash
jq '.rpm_ostree.booted["requested-packages"]' system_profile.json
jq '.flatpak_manifest_diff' system_profile.json
```

## Source Files

### Host configuration (baked into image)
- keyd: [system/keyd/default.conf](system/keyd/default.conf)
- NetworkManager (optional): [system/NetworkManager/conf.d/](system/NetworkManager/conf.d/)
- systemd tweaks (optional): [system/systemd/](system/systemd/)

### Shell defaults (skel)
- [skel/.bashrc](skel/.bashrc)
- [skel/.config/nushell/](skel/.config/nushell/)

### Upstream pinning (auto-updated by CI)
- [upstream/bazzite-stable.digest](upstream/bazzite-stable.digest)
- [upstream/getnf.ref](upstream/getnf.ref), [upstream/getnf.sha256](upstream/getnf.sha256)
