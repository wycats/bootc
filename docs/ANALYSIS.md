# System Profile Analysis

Source: `system_profile.txt`

## Layered packages detected

| Package | Fedora repos? (fedora+updates) | Notes |
|---|---:|---|
| 1password | No | Requires a third-party repo (vendor/COPR/ublue config). |
| 1password-cli | No | Requires a third-party repo (vendor/COPR/ublue config). |
| code | No | Fedora does not ship VS Code as `code`; likely a third-party repo. |
| gh | Yes | Available in Fedora repos. |
| keyd | No | Not in Fedora repos under this name. |
| lazygit | No | Not in Fedora repos under this name (may be from third-party repos). |
| microsoft-edge-stable | No | Requires Microsoft’s repo. |
| papirus-icon-theme | Yes | Available in Fedora repos. |
| starship | No | Not in Fedora repos under this name. |
| toolbox | Yes | Available in Fedora repos. |

## Dotfiles detected

- `/home/wycats/.bashrc`

## Dotfiles you called out as missing

- `/home/wycats/.config/nushell/config.nu`
- `/home/wycats/.config/nushell/env.nu`
- `/home/wycats/.config/containers/toolbox.conf` (if present)

## Current toolbox setup (inspected)

- Toolbox version: `0.3`
- Container name: `fedora-toolbox-42`
- Container image tag: `localhost/fedora-nix-toolbox:latest`
- The custom toolbox image has PATH baked into the image environment (host podman inspect):
	- `PATH=/home/wycats/.nix-profile/bin:/home/wycats/.cargo/bin:/home/wycats/.local/bin:...`
- The toolbox container bind-mounts your home at `/var/home/wycats`, and the image `Cmd` includes `--home /home/wycats --home-link`, which is how `/home/wycats/...` paths resolve inside the container.

## asahi-env tool scan (portable `/etc` writers)

I scanned `wycats/asahi-env` for tool codepaths that write to `/etc/*`.

Findings:

- `asahi-setup spotlight` modifies GNOME gsettings (search + input-source keybindings) and may patch `/etc/keyd/default.conf`.
	- We do **not** bake GNOME gsettings into the bootc image (they’re per-user/per-session state).
	- The keyd config is managed as a concrete file in this repo: `system/keyd/default.conf`.
- `bazzite-setup` can enable the `dspom/keyd` COPR by downloading a `.repo` file to `/etc/yum.repos.d/_copr-dspom-keyd.repo`.
	- This is intentionally a runtime trust decision in asahi-env; this repo does not bake COPR enablement by default.

Extracted file artifacts from asahi-env’s runbook live under `system/` and are wired as opt-in `COPY` lines in `Containerfile`.
