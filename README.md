# bootc Config as Code (Bazzite base)

- Container build definition: [Containerfile](Containerfile)
- Package analysis: [docs/ANALYSIS.md](docs/ANALYSIS.md)
- Migration steps: [docs/MIGRATION.md](docs/MIGRATION.md)
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
