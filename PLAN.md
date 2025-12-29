# Plan: Bazzite → bootc image (this repo)

This document captures the end-to-end plan for migrating a stock Bazzite install to tracking a custom bootc image built from this repo, while keeping the existing working setup safe and making updates fully automated.

## Goals

1. Switch from stock Bazzite to a bootc image derived from this repo without losing user data or surprises.
2. Make image publishing to GHCR fully automatic on changes to `main`.
3. Consume updates through the normal bootc/uBlue update flow (tag moves → `bootc upgrade` stages → reboot).

## Mental model (what changes vs what persists)

- **Immutable image (this repo builds it)**
  - Packages and system files baked into the image (see `Containerfile`).
  - Optional payloads shipped in the image but disabled by default (enabled via explicit actions).
  - `ujust` recipes shipped in the image.

- **Persistent state (already on your machine)**
  - Home directories and most app state (under `/var` and `/var/home/...`) should persist across bootc deployments.
  - Flatpaks are typically stored under `/var` and/or your home; switching images should not wipe them.

- **First-login bootstrap (runs per-user, idempotent)**
  - Flatpaks and GNOME extensions are applied on first login (and re-applied when manifests change) via `bootc-bootstrap`.

## Phase 0: Inventory and define “source of truth”

1. Capture a baseline snapshot of the current machine:
   - Run `./scripts/build-system-profile --output system_profile.json --text-output system_profile.txt`
   - Use the JSON as your migration checklist.

2. Move “real configuration” into the repo (source of truth):
   - System packages and files: update `Containerfile` / `system/`.
   - User-space apps: update `manifests/flatpak-*.txt`.
   - GNOME extensions: update `manifests/gnome-extensions.txt`.
   - Shell/toolbox defaults: keep dotfiles under `skel/`.

3. Accept that `/etc` drift is the common surprise:
   - If `etc_config_diff` cannot be gathered without root, decide whether to:
     - temporarily allow non-interactive root for inspection, or
     - proceed and handle surprises iteratively after the first boot.

## Phase 1: Make publishing to GHCR reliable

The workflow `.github/workflows/build.yml` is intended to build and push:

- `ghcr.io/<owner>/<repo>:latest`
- `ghcr.io/<owner>/<repo>:sha-<gitsha>`

Checklist:

1. Ensure Actions can push packages:
   - Repository workflow permissions must allow `GITHUB_TOKEN` to write packages.
   - The GHCR package must be associated with the repo and allow GitHub Actions access.

2. Validate on a trivial change:
   - Push a small commit to `main`.
   - Confirm a new `:latest` digest appears in GHCR.

## Phase 2: Safe switch on the host (stock Bazzite → your image)

Recommended first switch strategy:

1. Keep a fresh snapshot from Phase 0.
2. Switch to the published image:
   - `sudo bootc switch --transport registry ghcr.io/<owner>/<repo>:latest`

Notes:

- `bootc switch` stages a new deployment; the change takes effect after reboot.
- Have a rollback plan before rebooting:
  - Prefer boot menu selection of the previous deployment if something goes wrong.
  - Or use `sudo bootc rollback` once you’re back on a working boot.

3. After reboot, validate the invariants:
   - Home data present
   - Flatpaks still present
   - `bootc-bootstrap` applies manifests (or is able to retry on next login)

## Phase 3: Normal update loop (config changes → update arrives)

Once the machine is tracking `ghcr.io/<owner>/<repo>:latest`:

1. Change this repo → push to `main`.
2. GitHub Actions builds and pushes a new `:latest` image.
3. On the machine:
   - `sudo bootc upgrade` should fetch and stage the updated deployment.
   - Reboot to apply.

Optional verification during rollout:

- Use `:sha-<gitsha>` temporarily to pin a known build, then switch to `:latest` once you’re confident the pipeline is stable.

## Development loop: iterating on ujust recipes before a rebuild

If you want to try updated recipes immediately (without rebuilding/switching images), run host `ujust` against the repo file explicitly:

- `flatpak-spawn --host ujust -f /var/home/wycats/Code/Config/bootc/ujust/60-custom.just --list`

Once you rebuild/switch to an image containing the updated recipes, normal `ujust ...` will pick them up automatically.

## Current workstream: remote play / console mode (Option B)

This repo ships optional artifacts plus `ujust` commands for enabling Steam Remote Play style console mode (gamescope DRM + Steam gamepad UI on tty2).

- Enable: `ujust enable-remote-play`
- Disable: `ujust disable-remote-play`
- Status: `ujust remote-play-status`

The artifacts are shipped in the image as optional payload and installed to the host when enabled.
