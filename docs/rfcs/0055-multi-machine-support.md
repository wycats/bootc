# RFC 0055: Multi-Machine Support

- **Status**: Draft
- **Created**: 2026-02-28
- **Related**: [RFC-0052](canon/0052-manifest-lifecycle.md) (manifest lifecycle), [RFC-0053](0053-bootstrap-and-repo-discovery.md) (bootstrap), [RFC-0047](0047-bkt-wrap.md) (wrappers)

## Summary

One repo, multiple machines. Manifests gain architecture and machine-context
awareness so that `bkt containerfile generate` produces correct Containerfiles
for each variant. Bootstrap, upstream binaries, external repos, Flatpaks,
wrappers, and distrobox configs all respect the target architecture.

## Motivation

The repo currently targets a single machine: an x86_64 Bazzite desktop. Adding
an ARM64 Fedora Silverblue VM on Apple Silicon Mac required:

- A hand-written `Containerfile.arm64` that duplicates ~90% of the main Containerfile
- Separate manifest files (`flatpak-apps.arm64.json`, `external-repos.arm64.json`)
- Hardcoded aarch64 URLs for upstream binaries (starship, lazygit)
- A separate `distrobox.arm64.ini` pointing to an ARM toolbox tag
- Manual exclusion of x86_64-only packages (Edge, 1Password desktop)

This duplication means:

- Adding a package requires editing two Containerfiles
- Adding a Flatpak requires editing two manifest files
- Upstream version bumps require updating hardcoded URLs
- Drift between variants is invisible until something breaks

## Design

### Machine Variants

A variant is a named configuration that selects a base image, architecture,
and set of overrides. Defined in a new top-level config file:

```json
{
  "variants": {
    "desktop": {
      "arch": "x86_64",
      "base_image": "ghcr.io/ublue-os/bazzite-gnome:stable",
      "image_tag": "latest",
      "features": ["gaming", "edge-browser"]
    },
    "mac-vm": {
      "arch": "aarch64",
      "base_image": "quay.io/fedora-ostree-desktops/silverblue:43",
      "image_tag": "arm64",
      "features": ["vm-guest-tools", "mac-host-integration"]
    }
  }
}
```

### Architecture-Aware Manifests

Instead of separate manifest files per variant, entries gain optional
architecture and feature annotations:

#### External Repos

```json
{
  "repos": [
    {
      "name": "code",
      "baseurl": "https://packages.microsoft.com/yumrepos/vscode",
      "packages": ["code"]
    },
    {
      "name": "microsoft-edge",
      "baseurl": "https://packages.microsoft.com/yumrepos/edge",
      "packages": ["microsoft-edge-stable"],
      "arch": ["x86_64"]
    },
    {
      "name": "1password",
      "baseurl": "https://downloads.1password.com/linux/rpm/stable/$basearch",
      "packages": ["1password", "1password-cli"],
      "arch_packages": {
        "x86_64": ["1password", "1password-cli"],
        "aarch64": ["1password-cli"]
      }
    }
  ]
}
```

#### Upstream Binaries

Asset patterns become arch-aware:

```json
{
  "name": "starship",
  "source": {
    "type": "github",
    "repo": "starship/starship",
    "asset_pattern": {
      "x86_64": "starship-x86_64-unknown-linux-gnu.tar.gz",
      "aarch64": "starship-aarch64-unknown-linux-musl.tar.gz"
    }
  }
}
```

#### Flatpak Apps

Apps gain an optional `arch` field. Absence means all architectures:

```json
{
  "id": "com.discordapp.Discord",
  "remote": "flathub",
  "arch": ["x86_64"]
}
```

#### System Packages

Packages gain optional `arch` and `feature` fields:

```json
{
  "packages": [
    "curl",
    "distrobox",
    { "name": "chromium", "arch": ["aarch64"] },
    { "name": "open-vm-tools-desktop", "feature": "vm-guest-tools" }
  ]
}
```

#### Wrappers

Wrappers in `image-config.json` gain `arch`:

```json
{
  "name": "msedge-wrapper",
  "type": "wrapper",
  "arch": ["x86_64"],
  "target": "/usr/lib/opt/microsoft/msedge/microsoft-edge",
  "slice": "app-msedge.slice"
}
```

### Containerfile Generation

`bkt containerfile generate` gains a `--variant` flag:

```bash
bkt containerfile generate                    # default variant (desktop)
bkt containerfile generate --variant mac-vm   # ARM variant
bkt containerfile generate --all              # all variants
```

The generator:

1. Reads the variant config to determine arch, base image, and features
2. Filters all manifests by arch and feature
3. Produces a Containerfile with only the relevant stages
4. Writes to `Containerfile` (default) or `Containerfile.<variant>`

### Distrobox Config

The distrobox manifest gains variant awareness. The toolbox image tag
is derived from the variant's `image_tag`:

```json
{
  "containers": {
    "bootc-dev": {
      "image": "ghcr.io/wycats/bootc-toolbox:${variant.image_tag}"
    }
  }
}
```

### Bootstrap (`bkt bootstrap`)

RFC-0053 specifies `bkt bootstrap` as a CLI command replacing the shell script.
This RFC adds:

- Architecture detection (`uname -m`) for filtering manifests at runtime
- Proper Flatpak remote setup (using `.flatpakrepo` URLs with GPG keys)
- Extension enable/disable as typed enum (not string comparison)
- Distrobox image tag resolution from variant config
- Mac host integration setup (xdg-open wrapper, SSH config)

### Mac Host Integration

For the `mac-vm` variant, the image includes:

- **xdg-open forwarder** at `/usr/local/bin/xdg-open` — forwards HTTP(S) URLs
  to the Mac host via SSH, with reverse tunnel for OAuth callbacks
- **vscode:// protocol handler** — macOS app that redirects `vscode://` URLs
  back to the VM (installed on the Mac, not in the image)
- **SSH port forwarding config** — `Host silverblue` entry on the Mac for
  dev server access

These are configured via `~/.config/bootc/mac-host` (SSH destination of the
Mac). When this file is absent, the forwarder is a no-op passthrough.

### CI

The build workflow gains variant awareness:

- `build.yml` builds the default variant (x86_64, existing behavior)
- `build-arm64.yml` builds the `mac-vm` variant on ARM runners
- Both share the same manifests; the Containerfile is generated per-variant
- Toolbox images are built per-variant with arch-specific tags

Long-term, a single workflow could build all variants, but separate workflows
are simpler and allow different triggers (the ARM build doesn't need hourly
upstream polling).

## Implementation Plan

### Phase 1: Manifest Schema Changes

1. Add `arch` field to external-repos, flatpak-apps, system-packages, wrappers
2. Add `arch_packages` to external-repos for per-arch package lists
3. Add arch-aware `asset_pattern` to upstream manifest
4. Add `feature` field to system-packages
5. Update JSON schemas
6. Update `bkt` manifest loaders to filter by arch

### Phase 2: Variant Config

1. Create `manifests/variants.json` with variant definitions
2. Add `--variant` flag to `bkt containerfile generate`
3. Generate Containerfiles per-variant from shared manifests
4. Add `bkt containerfile check --variant` for CI
5. Remove `Containerfile.arm64` (generated, not hand-written)
6. Remove `*.arm64.json` manifest files (merged into main manifests)

### Phase 3: Bootstrap as `bkt bootstrap`

1. Implement `bkt bootstrap` command in Rust
2. Replace `scripts/bootc-bootstrap` shell script
3. Architecture-aware manifest filtering at runtime
4. Proper Flatpak remote setup with GPG
5. Typed extension enable/disable
6. Update systemd units to call `bkt bootstrap`

### Phase 4: Mac Host Integration

1. Formalize `mac-host` config in `bkt`
2. `bkt setup mac-host` command for initial SSH key exchange
3. xdg-open wrapper as a `bkt`-managed artifact
4. Document the vscode:// handler setup

## Migration

The current ARM files (`Containerfile.arm64`, `*.arm64.json`,
`distrobox.arm64.ini`) continue to work during migration. Once Phase 2 is
complete, they are replaced by generated output and can be deleted.

## Alternatives Considered

### Separate repos per machine

Rejected: defeats the purpose of a single source of truth. Changes would
need to be synchronized across repos.

### Branch-per-machine

Rejected: branches diverge. Cherry-picking shared changes is error-prone.

### Single Containerfile with ARG TARGETARCH

Considered but deferred: Docker's `ARG TARGETARCH` works for simple
conditionals but becomes unreadable with many arch-specific sections.
Generating separate Containerfiles from manifests is cleaner.
