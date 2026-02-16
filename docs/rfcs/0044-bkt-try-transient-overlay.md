# RFC 0044: `bkt try` — Transient Overlay with Declarative Capture

## Status

Draft

## Problem

On an atomic Linux system (bootc/ostree), installing a new RPM package
requires a full pipeline: edit manifest → commit → PR → CI build →
`bootc upgrade` → reboot. This is correct for production convergence,
but creates a painful feedback loop for exploration:

- "Does `ripgrep` exist in Fedora repos?" → can't just `dnf install` it
- "Will this package conflict with my base image?" → won't know until CI
- "I need `strace` right now to debug something" → 20-minute pipeline wait

The atomic Linux community's answer is "use `toolbox`/`distrobox`" for
ephemeral tools, but that doesn't help for packages that need host-level
integration (systemd units, udev rules, `/usr/bin` presence for desktop
entries, etc.).

`rpm-ostree usroverlay` exists as a primitive — it mounts a writable
tmpfs overlay on `/usr`, allowing `dnf install` to work immediately.
But used alone, it's exactly the kind of untracked drift that atomic
distros exist to prevent: the running system silently diverges from the
declared image, and the next reboot loses everything.

## Insight

The atomic Linux objection to mutable `/usr` is really about
**untracked drift**, not mutability per se. If the mutation and the
declaration happen as a single atomic user action, the overlay becomes
a _preview_ of a pending declarative change — not an escape hatch from
the model.

bkt is already the system that turns user intent into declarative
manifests. It can make the overlay safe by ensuring that every transient
installation is simultaneously captured as a manifest change entering
the convergence pipeline.

## Solution

A new `bkt try <package>` command that:

1. Installs the package into a transient `/usr` overlay (available now)
2. Records the package in `manifests/system-packages.json` (source of truth)
3. Regenerates the Containerfile (`bkt containerfile generate`)
4. Commits, pushes, and creates a PR (enters the pipeline)

The overlay is explicitly ephemeral — lost on reboot. But by the time
the user reboots, the PR has (ideally) been merged, the image built,
and `bootc upgrade` staged. The system converges to the declared state
seamlessly.

### User Experience

```
$ bkt try ripgrep
→ Unlocking /usr overlay...
→ Installing ripgrep via dnf...
✓ ripgrep 14.1.0 installed (available now)
→ Added to manifests/system-packages.json
→ Containerfile regenerated
→ PR #125 created: "feat: add ripgrep to system packages"
ℹ ripgrep is available now. It will persist after reboot once PR #125
  is merged and the image is built.
```

```
$ bkt try --remove ripgrep
→ Removed from manifests/system-packages.json
→ Containerfile regenerated
→ PR #126 created: "chore: remove ripgrep from system packages"
ℹ ripgrep is still available until reboot (overlay cannot remove
  base-image packages). It will be gone in the next image build.
```

### Command Design

```
bkt try <package> [<package>...]    # Install + capture
bkt try --remove <package>          # Capture removal (overlay can't undo)
bkt try --status                    # Show overlay state vs manifest state
bkt try --reset                     # Reboot reminder (overlay is tmpfs)
```

## Architecture

### Phase 1: Core `bkt try`

```
bkt try <package>
  │
  ├─ Check: is <package> already in system-packages.json?
  │   └─ Yes → "already declared, skipping"
  │
  ├─ Overlay: sudo rpm-ostree usroverlay (idempotent)
  │   └─ Already unlocked → skip
  │   └─ Locked → unlock (mounts tmpfs overlay on /usr)
  │
  ├─ Install: sudo dnf install -y <package>
  │   └─ Failure → report error, do NOT update manifest
  │   └─ Success → package is usable immediately
  │
  ├─ Capture: add <package> to manifests/system-packages.json
  │
  ├─ Regenerate: bkt containerfile generate
  │
  └─ PR: git checkout -b try/<package>, commit, push, create PR
```

### Phase 2: `bkt try --status`

Shows the delta between the overlay and the declared image:

```
$ bkt try --status
Overlay packages (not yet in image):
  ripgrep 14.1.0    PR #125 (building...)
  strace 6.8        PR #126 (merged, awaiting upgrade)

Manifest packages (not yet installed locally):
  htop              (in manifest, will appear after reboot)
```

This requires comparing:

- `rpm -qa` on the running system (overlay + base)
- `manifests/system-packages.json` (declared state)
- Base image package list (from `rpm-versions.txt` label or snapshot)

### Phase 3: Upstream `bkt try`

Extend to upstream binaries (not just RPMs):

```
$ bkt try --upstream lazygit
→ Fetching lazygit v0.44.0...
→ Installed to /usr/bin/lazygit (overlay)
→ Updated upstream/manifest.json
→ PR #127 created
```

This uses the existing `bkt-build fetch` infrastructure to download
and install into the overlay, then captures the pin in the upstream
manifest.

## Overlay Mechanics

### `rpm-ostree usroverlay` (aka `ostree admin unlock`)

- Mounts a **writable tmpfs** overlay on top of the composefs `/usr`
- All writes go to the tmpfs upper layer (RAM-backed)
- Reads fall through to the immutable lower layer
- **Transient by default** — lost on reboot
- `--hotfix` flag would persist across reboots, but we explicitly
  do NOT use this — transience is a feature, not a bug
- Requires root (polkit rule already exists for wheel group)
- Idempotent-ish: errors if already unlocked, but we can detect and skip

### What works in the overlay

- `dnf install` — installs packages, creates files in `/usr`
- Binaries are immediately available in `$PATH`
- Systemd units can be enabled (symlinks land in overlay)
- Desktop entries appear after `update-desktop-database`

### What doesn't work

- **Package removal** — can't remove files from the lower (immutable)
  layer. `dnf remove` of a base-image package will appear to succeed
  but the files remain visible through the overlay.
- **Kernel modules** — may install but won't load until reboot
  (at which point the overlay is gone)
- **Large packages** — overlay is RAM-backed, so installing 500MB of
  packages eats 500MB of RAM. Fine for CLI tools, risky for large apps.
- **Conflicts with base image** — if the overlay package ships a file
  that already exists in the base image, the overlay version wins.
  This is usually fine but can cause confusion.

## Relationship to Existing Tiers

This fits naturally into the existing tier model:

| Tier         | Scope                   | Mechanism                  | Convergence                      |
| ------------ | ----------------------- | -------------------------- | -------------------------------- |
| Tier 1       | Image (immutable)       | Containerfile + bootc      | Reboot                           |
| **Tier 1.5** | **Overlay (transient)** | **`bkt try` + usroverlay** | **Reboot (converges to Tier 1)** |
| Tier 2       | User session            | flatpak, gsettings, shims  | Login / `bootc-bootstrap`        |
| Tier 3       | Toolbox                 | distrobox, brew            | Immediate                        |

Tier 1.5 is a **preview layer** — it lets you experience a Tier 1
change before it's baked into the image. The key property is that it
always converges: the overlay is lost on reboot, and the image (built
from the manifest change) takes over.

## Safety Properties

1. **No untracked drift.** Every `bkt try` creates a manifest change.
   The overlay cannot diverge from the declared state because the
   declaration happens simultaneously with the installation.

2. **Convergence guaranteed.** The overlay is tmpfs — it vanishes on
   reboot. The image built from the updated manifest is the permanent
   state. Even if the PR is never merged, the system returns to its
   declared state on reboot.

3. **Idempotent.** Running `bkt try ripgrep` twice is safe — the second
   invocation detects it's already in the manifest and skips.

4. **Reversible.** `bkt try --remove` updates the manifest. The overlay
   can't actually remove the package, but the next image build will
   exclude it. The user gets a clear message about this.

5. **Auditable.** Every transient installation has a corresponding PR.
   `bkt try --status` shows the full delta between overlay, manifest,
   and base image.

## Open Questions

### 1. PR workflow for `bkt try`

Should `bkt try` create one PR per package, or batch multiple `try`
invocations into a single PR?

**Option A:** One PR per `bkt try` invocation. Simple, clear audit trail.
But could create PR noise if the user tries 5 packages in a row.

**Option B:** Accumulate tries into a `try/*` branch, one PR updated
with each new package. Cleaner, but more complex branch management.

### 2. Polkit integration

`rpm-ostree usroverlay` and `dnf install` require root. The existing
polkit rule (`50-bkt-admin.rules`) grants passwordless access for
`bootc` and `rpm-ostree` to the wheel group. Should `dnf` be added
to this rule, or should `bkt try` use a dedicated polkit action?

### 3. Memory budget for overlay

Since the overlay is RAM-backed, should `bkt try` check available
memory before installing and warn if the package is large?

### 4. Interaction with `bootc upgrade`

If `bootc upgrade` stages a new image while the overlay is active,
does the overlay survive until reboot? (It should — the overlay is
on the running deployment, not the staged one.) Need to verify.

### 5. Extension to non-RPM packages

Phase 3 proposes extending to upstream binaries. Should this also
cover COPR repos (`bkt try --copr <repo> <package>`)?

## Implementation Plan

### Phase 1: `bkt try` for RPM packages

1. Add `TryCommand` to `bkt/src/commands/`
2. Implement overlay unlock (detect state, call `rpm-ostree usroverlay`)
3. Implement `dnf install` via `CommandRunner`
4. Manifest update + containerfile regeneration (existing infrastructure)
5. PR creation (existing `bkt --pr` infrastructure)
6. Polkit rule update if needed

### Phase 2: Status and visibility

1. `bkt try --status` comparing overlay vs manifest vs base image
2. Integration with `bkt status` dashboard

### Phase 3: Upstream binaries

1. Extend `bkt try --upstream <name>` using `bkt-build fetch`
2. Overlay installation of fetched binaries
3. Manifest capture in `upstream/manifest.json`

## References

- `ostree admin unlock`: https://ostreedev.github.io/ostree/man/ostree-admin-unlock.html
- `rpm-ostree usroverlay`: alias for `ostree admin unlock`
- bootc image model: https://containers.github.io/bootc/
- RFC 0042: Managed Containerfile (provides `bkt containerfile generate`)
- RFC 0038: RPM-aware rebuild (provides package manifest infrastructure)
