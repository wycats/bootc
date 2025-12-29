# bootc Config as Code (Bazzite base)

> ⚠️ **This repository and its container images are public.** Do not commit
> secrets, passwords, API keys, personal data, or any sensitive information.
> Machine-specific files like `system_profile.json` are gitignored for this
> reason.

- Container build definition: [Containerfile](Containerfile)
- Package analysis: [docs/ANALYSIS.md](docs/ANALYSIS.md)
- Migration steps: [docs/MIGRATION.md](docs/MIGRATION.md)
- End-to-end plan: [PLAN.md](PLAN.md)
- Upstream base digest (auto-updated): [upstream/bazzite-stable.digest](upstream/bazzite-stable.digest)
- getnf pinning (auto-updated): [upstream/getnf.ref](upstream/getnf.ref), [upstream/getnf.version](upstream/getnf.version), [upstream/getnf.sha256](upstream/getnf.sha256)

## Source of truth

This repo is the source of truth for host configuration baked into the bootc image.

- keyd: [system/keyd/default.conf](system/keyd/default.conf) (extracted from wycats/asahi-env)
- NetworkManager (Asahi-only / optional): [system/NetworkManager/conf.d/wifi_backend.conf](system/NetworkManager/conf.d/wifi_backend.conf)
- NetworkManager (optional): [system/NetworkManager/conf.d/default-wifi-powersave-on.conf](system/NetworkManager/conf.d/default-wifi-powersave-on.conf)
- systemd (Asahi-only / template): [system/asahi/titdb.service.template](system/asahi/titdb.service.template)
- modprobe (Asahi-only / optional): [system/asahi/modprobe.d/brcmfmac.conf](system/asahi/modprobe.d/brcmfmac.conf)

## Bootstrap state (applied on first login)

These are applied by a user `systemd` oneshot (`bootc-bootstrap.service`) and re-run automatically when the manifests change:

- Flatpak remotes: [manifests/flatpak-remotes.txt](manifests/flatpak-remotes.txt)
- Flatpak apps: [manifests/flatpak-apps.txt](manifests/flatpak-apps.txt)
- GNOME extensions: [manifests/gnome-extensions.txt](manifests/gnome-extensions.txt)

## ujust customization

- Custom recipes shipped in the image: [ujust/60-custom.just](ujust/60-custom.just)

Remote play (Option B / tty2 + Steam gamepad UI):

- `ujust enable-remote-play` (installs unit + enables it)
- `ujust disable-remote-play`
- `ujust remote-play-status`

## System profile (migration preflight)

Generate a machine snapshot (JSON + optional text):

- `./scripts/build-system-profile --output system_profile.json --text-output system_profile.txt`

`jq` is expected to be available (it’s installed in the bootc image; and `./scripts/toolbox-gpg-setup` installs it in a toolbox).

Quick queries:

- `jq '.rpm_ostree.booted["requested-packages"]' system_profile.json`
- `jq '.etc_config_diff' system_profile.json`
- `jq '.flatpak_manifest_diff' system_profile.json`
- `jq '.gnome_extensions_enabled_vs_installed' system_profile.json`
