# Migration Guide (Bazzite → bootc image)

This repository is a "config as code" bootc image definition based on `ghcr.io/ublue-os/bazzite:stable`.

## 1) Copy these dotfiles into this repo now

Your `system_profile.txt` only detected one dotfile, but you also called out that it missed your Nushell and toolbox setup.

Copy these into this repo (matching paths under `skel/`):

- `/home/wycats/.bashrc` → `skel/.bashrc`
- `/home/wycats/.config/nushell/config.nu` → `skel/.config/nushell/config.nu`
- `/home/wycats/.config/nushell/env.nu` → `skel/.config/nushell/env.nu`
- `/home/wycats/.config/containers/toolbox.conf` → `skel/.config/containers/toolbox.conf` (if you have it)

The `skel/.config/nushell/env.nu` scaffold already prepends these to PATH when they exist:

- `~/.nix-profile/bin`
- `~/.cargo/bin`
- `~/bin`

That PATH logic is what makes your toolbox containers pick them up too, since toolbox binds your home directory into the container.

Update: after inspecting your current toolbox container, your toolbox PATH is actually coming from the toolbox *image tag* (`localhost/fedora-nix-toolbox:latest`) via a baked-in `PATH=...` environment in the image. The Nushell `env.nu` scaffold in this repo is now conservative (no PATH changes by default) to avoid double-prepending.

Command:

- `cp -a /home/wycats/.bashrc ./skel/.bashrc`
- `cp -a /home/wycats/.config/nushell/config.nu ./skel/.config/nushell/config.nu`
- `cp -a /home/wycats/.config/nushell/env.nu ./skel/.config/nushell/env.nu`
- `cp -a /home/wycats/.config/containers/toolbox.conf ./skel/.config/containers/toolbox.conf` (if present)

## 2) Build and publish

Pushing to `main` triggers the GitHub Action workflow to build and push the image to GHCR as:

- `ghcr.io/<owner>/<repo>:latest`

(where `<owner>/<repo>` is the GitHub repository name you push this into).

This repo also includes scheduled rebuilds:

- Hourly: cheap poll of the upstream `ghcr.io/ublue-os/bazzite:stable` digest; rebuild only if it changed.
- Nightly (03:00 UTC): forced rebuild to pick up RPM updates even if upstream digest did not change.

## 2.5) First-login bootstrap (Flatpaks + GNOME extensions)

This image ships a user `systemd` oneshot that runs on first login to apply:

- Flatpak remotes + installs from `manifests/flatpak-remotes.txt` and `manifests/flatpak-apps.txt`
- GNOME Shell extensions from `manifests/gnome-extensions.txt`
- GNOME power defaults from `manifests/gsettings.txt` (AC: no auto-suspend; battery: unchanged)

Note: your current Flatpak manifest includes `system` installs; those may prompt once for admin authentication (via `pkexec`) on first login.

It is idempotent and keys off a hash of the manifest files. If you want to force it to re-run, delete:

- `~/.local/state/bootc-bootstrap/last-applied.sha256`

If it can't complete (for example, no network on first login), it will try again on the next login until it succeeds.

## Optional (Asahi-only) artifacts extracted from asahi-env

These are kept in this repo as “source of truth” files, but are **not enabled** by default (because they’re primarily relevant to Asahi hardware):

- NetworkManager iwd backend drop-in: `system/NetworkManager/conf.d/wifi_backend.conf`
- titdb unit template (trackpad deadzones): `system/asahi/titdb.service.template`
- Broadcom brcmfmac modprobe options (Asahi Wi‑Fi): `system/asahi/modprobe.d/brcmfmac.conf`

Optional (more general) NetworkManager tweak (also off by default):

- Disable Wi‑Fi power management: `system/NetworkManager/conf.d/default-wifi-powersave-on.conf`

To opt into this one, set `ENABLE_NM_DISABLE_WIFI_POWERSAVE=1` in `Containerfile` (it is `0` by default) and push to `main` to publish a new image.

To opt into the Asahi-focused NetworkManager backend switch, set `ENABLE_NM_IWD_BACKEND=1`.

Additional useful opt-ins:

- `ENABLE_JOURNALD_LOG_CAP=1` (cap persistent journal size; defaults to 1G max + keep 2G free)
- `ENABLE_LOGIND_LID_POLICY=1` (battery: suspend on lid close; AC/docked: ignore lid close)

To opt into the Asahi-specific ones below, set:

- `ENABLE_ASAHI_BRCMFMAC=1` (for `brcmfmac.conf`)
- `ENABLE_ASAHI_TITDB=1` (for `titdb.service`)

Note: `titdb.service.template` intentionally contains a placeholder `/dev/input/by-path/REPLACE_ME-event-mouse` and must be edited for the target device path before it’s usable.

## 3) Switch the laptop to the new image

Once the workflow publishes an image, switch using bootc:

- `sudo bootc switch --transport registry ghcr.io/<owner>/<repo>:latest`

Then reboot.

## 4) How updates are discovered/applied

Your laptop will see updates when the image tag you track (for example `:latest`) changes its digest in GHCR.

- Check current state: `sudo bootc status`
- Fetch and stage the next deployment: `sudo bootc upgrade`

Then reboot to boot into the staged deployment.

This repo’s GitHub Actions rebuilds/pushes on:

- Any push to `main` (your config changes)
- Hourly upstream digest changes (when `ghcr.io/ublue-os/bazzite:stable` moves)
- Nightly forced rebuild (to pick up RPM repo updates)

If you want to pin to an immutable identifier:

- `sudo bootc switch --transport registry ghcr.io/<owner>/<repo>:sha-<gitsha>`
