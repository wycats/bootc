# RFC 0055: Multi-Machine Support

- **Status**: Draft
- **Created**: 2026-02-28
- **Updated**: 2026-02-28
- **Related**: [RFC-0052](canon/0052-manifest-lifecycle.md) (manifest lifecycle), [RFC-0053](0053-bootstrap-and-repo-discovery.md) (bootstrap), [RFC-0047](0047-bkt-wrap.md) (wrappers)
- **See also**: RFC-0056 (Mac Host Integration, planned)

## Summary

One repo, multiple machines. Manifests gain architecture awareness so that
`bkt` produces correct Containerfiles, applies the right state, and captures
changes appropriately for each machine. The shell bootstrap script is replaced
by `bkt bootstrap` in Rust. All machine-specific decisions are visible in the
manifest entries themselves.

## Motivation

The repo currently targets a single machine: an x86_64 Bazzite desktop. Adding
an ARM64 Fedora Silverblue VM on Apple Silicon Mac required:

- A hand-written `Containerfile.arm64` that duplicates ~90% of the main Containerfile
- Separate manifest files (`flatpak-apps.arm64.json`, `external-repos.arm64.json`)
- Hardcoded aarch64 URLs for upstream binaries (starship, lazygit)
- A separate `distrobox.arm64.ini` pointing to an ARM toolbox tag
- Manual exclusion of x86_64-only packages (Edge, 1Password desktop)
- A shell bootstrap script that crashed on ARM due to jq parsing bugs,
  wrong extension enable/disable ordering, and missing GPG keys

This duplication means:

- Adding a package requires editing two Containerfiles
- Adding a Flatpak requires editing two manifest files
- Upstream version bumps require updating hardcoded URLs
- Drift between variants is invisible until something breaks
- Bootstrap bugs affect all machines and are hard to fix in shell

## Design Principles

This RFC extends the existing principles from RFC-0052:

- **Single source of truth**: one set of manifests, not per-machine copies
- **Visible change**: machine-specific decisions are visible in the manifest
  entry itself (via `arch` field), not in a separate config file
- **Uniform architecture**: every subsystem handles arch filtering the same way
- **Subsystem autonomy**: each subsystem decides how to determine arch
  availability during capture (some check locally, some query the network)

## Architecture Awareness

### The `arch` Field

Manifest entries gain an optional `arch` field. When absent, the entry applies
to all architectures. When present, it lists the architectures where the entry
is relevant:

```json
{ "id": "com.discordapp.Discord", "remote": "flathub", "arch": ["x86_64"] }
```

This is the **only** filtering mechanism. There is no `features` system, no
variant config file, no cross-referencing. If you look at a manifest entry,
you can tell exactly which machines it applies to. This is consistent with
RFC-0052's principle that all state is visible in the manifest.

### Runtime Architecture Detection

At runtime, `bkt` determines the current architecture via `uname -m`:

- `x86_64` on the Bazzite desktop
- `aarch64` on the ARM VM

This is stored at `~/.local/state/bkt/arch` during bootstrap (consistent with
the `~/.local/state/bkt/repo-path` pattern from RFC-0053). All runtime commands
(`apply`, `capture`, `drift`, `bootstrap`) read it.

No variant name or variant config file is needed at runtime. The architecture
is the discriminator.

### Build-Time Variant Config

For Containerfile generation and CI, a minimal variant config defines the
base image and image tag per architecture:

```json
{
  "variants": {
    "desktop": {
      "arch": "x86_64",
      "base_image": "ghcr.io/ublue-os/bazzite-gnome:stable",
      "image_tag": "latest",
      "toolbox_tag": "latest"
    },
    "mac-vm": {
      "arch": "aarch64",
      "base_image": "quay.io/fedora-ostree-desktops/silverblue:43",
      "image_tag": "arm64",
      "toolbox_tag": "arm64"
    }
  }
}
```

This is a **build-time** artifact only. It is not read at runtime. It tells
`bkt containerfile generate --variant mac-vm` which base image to use and
which tag to push to. The manifest entries themselves carry the `arch` filter.

## Architecture-Aware Manifests

### External Repos

Repos gain `arch` (skip entire repo) and `arch_packages` (per-arch package
lists within a single repo):

```json
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
```

### Upstream Binaries

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

When `asset_pattern` is a string, it applies to all architectures (e.g.,
arch-independent assets like cursor themes). When it is an object, each key
is an architecture.

### Flatpak Apps

```json
{ "id": "com.discordapp.Discord", "remote": "flathub", "arch": ["x86_64"] }
```

Absence of `arch` means all architectures. Most Flatpaks are multi-arch and
need no annotation.

### System Packages

Packages gain optional `arch`:

```json
{
  "packages": [
    "curl",
    "distrobox",
    { "name": "chromium", "arch": ["aarch64"] },
    { "name": "open-vm-tools-desktop", "arch": ["aarch64"] }
  ]
}
```

String entries (like `"curl"`) apply to all architectures. Object entries
can specify `arch`.

### Wrappers

```json
{
  "name": "msedge-wrapper",
  "type": "wrapper",
  "arch": ["x86_64"],
  "target": "/usr/lib/opt/microsoft/msedge/microsoft-edge",
  "slice": "app-msedge.slice"
}
```

### GNOME Extensions and GSettings

These are architecture-independent. No `arch` field needed. Extensions are
JavaScript; gsettings are pure config. They apply to all machines.

## Capture with Variants

Capture is the primary workflow: install something on your machine, run
`bkt capture`, and the manifest updates. In a multi-machine world, capture
must decide whether to add an `arch` annotation.

### Subsystem-Driven Availability

Each subsystem determines arch availability using its own logic:

| Subsystem | How it determines availability | Network? |
|-----------|-------------------------------|----------|
| **Flatpak** | Query Flathub API for `arches` field | Yes |
| **System packages** | Fedora repos publish arch metadata | Yes |
| **External repos** | Check repo metadata for `$basearch` | Yes |
| **GNOME extensions** | Always multi-arch (JavaScript) | No |
| **GSettings** | Always multi-arch (config) | No |
| **Upstream binaries** | Check if asset patterns exist for both arches | Yes |
| **Wrappers** | Depends on wrapped binary's availability | Inherits |

The subsystem trait gains a method:

```rust
fn arch_availability(&self, entry: &Entry) -> ArchAvailability {
    // AllArch — no annotation needed
    // SpecificArch(vec!["x86_64"]) — add arch field
    // Unknown — no annotation (default to shared)
}
```

### Capture Rules

1. If the subsystem reports `AllArch`, the entry has no `arch` field (shared).
2. If the subsystem reports `SpecificArch`, the entry gets an `arch` field.
3. If the subsystem reports `Unknown`, the entry has no `arch` field. The
   assumption is shared; if it fails on another machine, `bkt apply` handles
   it gracefully (see below).
4. If an entry already exists in the manifest with an `arch` field, capture
   does not change it. Manual `arch` annotations are authoritative.

### Apply with Unavailable Entries

`bkt apply` must handle "entry exists in manifest but not available for this
arch" as a non-fatal condition. This is the safety net for entries that are
shared in the manifest but only available on some architectures:

- Flatpak not available on Flathub for this arch → skip with info message
- Package not in repo for this arch → skip with info message
- Upstream binary has no asset pattern for this arch → skip with info message

This is distinct from a *failure* (network error, permission denied). Arch
unavailability is expected and silent; failures are reported.

## Containerfile Generation

`bkt containerfile generate` gains a `--variant` flag:

```bash
bkt containerfile generate                    # default variant (desktop)
bkt containerfile generate --variant mac-vm   # ARM variant
bkt containerfile generate --all              # all variants
```

The generator:

1. Reads the variant config to determine arch, base image, and toolbox tag
2. Filters all manifests by arch
3. Produces a Containerfile with only the relevant stages
4. Writes to `Containerfile` (default) or `Containerfile.<variant>`

The distrobox config is also generated per-variant, substituting the
toolbox tag from the variant config.

## Bootstrap as `bkt bootstrap`

The shell script `scripts/bootc-bootstrap` is replaced by `bkt bootstrap`,
a Rust command that uses the same manifest types and subsystem architecture
as the rest of `bkt`.

This eliminates the class of bugs encountered during the ARM prototype:

- **tmpdir scope crash**: Rust's `Drop` handles cleanup, no trap/scope issues
- **Extension enable/disable**: Typed enum, not string comparison on `"true"`
- **Flatpak GPG keys**: Proper URL handling, not shell string matching
- **jq parsing**: Serde deserialization, not `jq -r` in a loop

`bkt bootstrap` filters manifests by the current architecture (`uname -m`)
at runtime. It does not need a variant config file.

The systemd unit changes from:

```ini
ExecStart=/usr/bin/bootc-bootstrap apply
```

to:

```ini
ExecStart=/usr/bin/bkt bootstrap
```

## CI

- `build.yml` builds the default variant (x86_64, existing behavior)
- `build-arm64.yml` builds the `mac-vm` variant on ARM runners
- Both use `bkt containerfile generate --variant` to produce Containerfiles
- `bkt containerfile check --variant` validates each in CI
- Toolbox images are built per-variant with arch-specific tags

## Two Workflows Validation

### Workflow 1: Install via GUI → Capture

**On the ARM VM:**

1. Install a Flatpak via GNOME Software (e.g., Telegram)
2. Run `bkt capture`
3. The Flatpak subsystem checks Flathub: Telegram has aarch64 and x86_64 builds
4. Entry added to `manifests/flatpak-apps.json` with no `arch` field (shared)
5. `git diff manifests/` shows the new entry
6. Commit and push
7. On the x86_64 desktop, `bkt apply` installs Telegram (available on x86_64)

**On the ARM VM (single-arch app):**

1. Install a Flatpak that only exists on aarch64
2. Run `bkt capture`
3. The Flatpak subsystem checks Flathub: only aarch64 builds exist
4. Entry added with `"arch": ["aarch64"]`
5. On the x86_64 desktop, `bkt apply` skips it (arch doesn't match)

### Workflow 2: Add to Manifest → Apply

**Adding a shared package:**

1. Edit `manifests/system-packages.json`, add `"htop"`
2. `bkt apply` installs htop on the current machine
3. On the other machine, `bkt apply` also installs htop

**Adding an arch-specific package:**

1. Edit `manifests/system-packages.json`, add `{ "name": "chromium", "arch": ["aarch64"] }`
2. On the ARM VM, `bkt apply` installs chromium
3. On the x86_64 desktop, `bkt apply` skips it (arch doesn't match)

## Implementation Plan

### Phase 1: `bkt bootstrap` in Rust + Manifest Schema Changes

Bootstrap is the front door for every new machine. It must be correct first.

1. Implement `bkt bootstrap` command in Rust
2. Replace `scripts/bootc-bootstrap` shell script
3. Architecture-aware manifest filtering at runtime (`uname -m`)
4. Proper Flatpak remote setup with GPG keys
5. Typed extension enable/disable (respects `enabled: false`)
6. Add `arch` field to all manifest types (external-repos, flatpak-apps,
   system-packages, wrappers, upstream binaries)
7. Add `arch_packages` to external-repos
8. Add arch-aware `asset_pattern` to upstream manifest
9. Update JSON schemas
10. Update `bkt` manifest loaders to filter by arch
11. Update `bkt apply` to handle arch-unavailable entries gracefully

### Phase 2: Variant Config + Containerfile Generation

1. Create `manifests/variants.json` with variant definitions
2. Add `--variant` flag to `bkt containerfile generate`
3. Generate Containerfiles per-variant from shared manifests
4. Generate distrobox config per-variant
5. Add `bkt containerfile check --variant` for CI
6. Remove `Containerfile.arm64` (now generated)
7. Remove `*.arm64.json` and `distrobox.arm64.ini` (merged into main manifests)

### Phase 3: Capture Arch Awareness

1. Add `arch_availability()` to subsystem trait
2. Implement per-subsystem availability checks (Flathub API, repo metadata)
3. Update `bkt capture` to annotate entries based on subsystem availability
4. Update `bkt drift` to report per-arch drift

## Migration

The current ARM files (`Containerfile.arm64`, `*.arm64.json`,
`distrobox.arm64.ini`) continue to work during migration. Once Phase 2 is
complete, they are replaced by generated output and can be deleted.

The shell bootstrap script is replaced in Phase 1. The systemd unit is
updated to call `bkt bootstrap` instead of `bootc-bootstrap apply`.

## Out of Scope

### Mac Host Integration

Cross-machine integration (xdg-open forwarding, vscode:// protocol handler,
SSH port forwarding, `~/.config/bootc/mac-host`) is a separate problem domain
from multi-arch manifests. It will be addressed in a future RFC-0056.

The xdg-open wrapper and related infrastructure built during the prototype
continue to work. RFC-0056 will formalize them as `bkt`-managed features.

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

### Features / tags system

Considered and rejected: a `feature` field on manifest entries would require
cross-referencing a variant config to understand which machines an entry
applies to. This violates RFC-0052's principle that all state is visible in
the manifest entry itself. The `arch` field is self-describing; features are
not. If a second axis of differentiation is needed in the future (e.g., two
x86_64 machines with different capabilities), it can be addressed in a
follow-up RFC with proper design for capture, drift, and visibility.
