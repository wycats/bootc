# RFC 0044: Tier 1 Preview and Convergence

- **Status**: Partially Implemented
- **Created**: 2026-02-10
- **Updated**: 2026-03-05
- **Absorbs**: RFC-0034 (usroverlay integration), RFC-0035 (admin update), RFC-0037 (bkt upgrade)
- **Related**: [RFC-0004](0004-bkt-admin.md) (Tier 1 model), [RFC-0042](0042-managed-containerfile.md) (Containerfile generation), [RFC-0052](canon/0052-manifest-lifecycle.md) (manifest lifecycle), [RFC-0054](0054-change-workflow.md) (change workflow)

## Summary

`bkt try` provides immediate preview of Tier 1 system package changes by installing into a transient `/usr` overlay while simultaneously capturing intent in manifests. The normal Tier 1 pipeline (manifest → PR → CI image build → `bootc upgrade` → reboot) remains the mechanism for permanence and convergence. This RFC defines the full lifecycle from "I want this package" to "it is in my image," and consolidates prior RFCs into one coherent model.

## The Problem

On an atomic Linux system, changing Tier 1 package state is intentionally heavyweight: update manifest, regenerate image definition, build in CI, stage update, and reboot. That is correct for deterministic convergence, but poor for exploration and short feedback loops. A user cannot quickly try a host-level package and decide whether to keep it without waiting for a full build pipeline.

`bkt try` bridges this gap by making Tier 1 changes immediately testable while preserving declarative convergence as the source of truth.

## The Overlay Primitive

This section records behavior verified from prior investigation and implementation context.

### `rpm-ostree usroverlay`

- Calls `ostree admin unlock --transient` under the hood
- Creates a tmpfs-backed overlayfs on `/usr`
- The overlay is writable; the underlying filesystem is unchanged
- Lost on reboot (the tmpfs is gone)
- `/etc` and `/var` changes from package scriptlets persist (they are not on the overlay)

### `dnf install` on the overlay

Once usroverlay unlocks `/usr`:

- `dnf install` works normally — full dependency resolution and RPM transactions
- The RPM database under `/usr/share/rpm` is updated in the overlay (ephemeral)
- Package updates (replacing already-installed packages) work in the running system
- COPR repos work when configured in `/etc/yum.repos.d/`
- No special package-manager behavior is required beyond using host `dnf`

### What persists across reboot

- `/etc` changes from RPM scriptlets (for example, config files and service links)
- `/var` changes (runtime or data directories)
- Nothing in `/usr` from the transient overlay

## Alternatives Considered: `rpm-ostree apply-live`

This analysis is included to avoid repeat investigation.

### What `apply-live` does

1. Creates a pending deployment via `rpm-ostree install` (dependency resolution, package fetch, full filesystem tree)
2. Calls `ostree admin unlock --transient` (same core primitive as usroverlay)
3. Diffs booted commit against the pending deployment commit
4. Copies changed files from pending deployment into the transient overlay
5. Updates `/etc` (persistent side effects)
6. Runs `systemd-tmpfiles` for `/run` and `/var`

### Why we do not use it

1. **Pending deployment conflict**: if a `bootc upgrade` is already staged, `rpm-ostree install` can replace that pending deployment.
2. **Replacement restrictions**: replacement of existing packages often requires `--allow-replacement`, reflecting live-replacement risk.
3. **Heavier workflow**: creates and materializes a full deployment just to copy a subset into an overlay.
4. **Cleanup workaround exists, but is not the model**: `rpm-ostree cleanup -p` can drop the pending deployment while overlay effects continue.

### When `apply-live` would be better

- When rpm-ostree’s deployment-level package resolution is specifically required
- When managing package state as ostree deployment metadata is required

### Bottom line

`usroverlay` + `dnf install` is the right primitive for `bkt try`: lighter-weight, naturally aligned with ephemeral preview, compatible with staged upgrades, and simpler to reason about in the Tier 1 lifecycle.

## Safety Analysis

### Safe to live-replace

Packages that install mostly self-contained files:

- Application packages (for example: VS Code, Edge, 1Password)
- Fonts, icon themes, and related assets
- Standalone CLI binaries

### Potentially unsafe

Packages that install:

- Shared libraries used by currently running processes
- systemd units requiring daemon state changes
- Kernel modules
- PAM modules and other core auth/runtime components

### Detection

Planned heuristic before install:

```rust
fn is_safe_to_live_replace(package: &str) -> bool {
  let files = rpm_query_files(package);
  !files.iter().any(|f| {
    f.starts_with("/usr/lib/systemd/") ||
    (f.starts_with("/usr/lib64/") && f.ends_with(".so")) ||
    (f.starts_with("/usr/lib/") && f.ends_with(".so"))
  })
}
```

For unsafe packages, `bkt try` should warn and allow override with `--force`.

## The `bkt try` Command

### Usage

```bash
# Try a package (install ephemerally + record in manifest)
bkt try htop

# Try multiple packages
bkt try htop btop neofetch

# Remove a previously tried package from manifest
bkt try --remove htop

# Show pending try state for this boot
bkt try --status

# Clean up tracked try side effects
bkt try --cleanup htop
```

### What `bkt try <package>` does

1. **Validate** package existence (`dnf repoquery`/validation path)
2. **Unlock** with `rpm-ostree usroverlay` if not already unlocked
3. **Prepare** runtime (`/var/lib/rpm-state`)
4. **Install** via host `dnf5 install -y <package>`
5. **Record** package in `manifests/system-packages.json`
6. **Regenerate** managed sections of `Containerfile`
7. **PR (optional)** create/update a `try/*` branch and PR in standard workflow

Immediate preview is provided by steps 2–4. Durable convergence is provided by steps 5–7 plus the normal Tier 1 pipeline.

### Overlay lifecycle

- Overlay survives while the current boot is running
- Overlay is discarded on reboot
- `bootc upgrade` stages a future deployment and does not clear the running overlay
- Permanence occurs only after CI image build + upgrade staging + reboot

## The Convergence Path

After `bkt try` captures package intent in manifests, the convergence path is:

1. Commit/push or PR merge
2. CI image build and publish
3. `bootc upgrade` to stage
4. reboot into the new image

### `bkt admin update` (absorbed from RFC-0035)

Proposed orchestration command for full convergence:

1. Capture drift (`bkt capture`)
2. Commit/push or PR workflow
3. Wait for CI status
4. Verify image readiness
5. Stage update with `bootc upgrade`
6. Report staged readiness

This command is not implemented yet; it remains the intended single-command convergence UX.

### `bkt upgrade` (absorbed from RFC-0037)

Proposed streamlined user command around `bootc upgrade`:

```bash
bkt upgrade              # Stage latest image
bkt upgrade --reboot     # Stage and reboot
bkt upgrade --status     # Show staged state
```

This command is not implemented yet; existing behavior remains under `bkt admin bootc` flows.

## COPR Support

COPR is a repository configuration concern, not an overlay limitation. Once `/usr` is unlocked, host `dnf` can resolve from configured repos.

Expected flow:

1. Enable repo (`dnf copr enable owner/project`)
2. Run `bkt try <package>`
3. Persist repo configuration for image builds via manifest/containerfile integration (future phase)

## Implementation Status

Status reflects current behavior in `bkt/src/commands/try_cmd.rs`.

| Feature | Status | Notes |
|---------|--------|-------|
| `bkt try <package>` | ✅ Implemented | Overlay install + capture in manifest + Containerfile section sync |
| `bkt try --remove` | ✅ Implemented | Removes from manifest; does not remove from immutable base |
| `bkt try --status` | ✅ Implemented | Shows pending try packages for current boot |
| `bkt try --cleanup` | ⚠️ Partial | Tracking removal only; cleanup actions not implemented |
| Safety analysis (file list check) | ❌ Not started | No pre-install file-list risk classification yet |
| COPR integration | ❌ Not started | No first-class `--copr` UX yet |
| `bkt admin update` | ❌ Not started | RFC concept only |
| `bkt upgrade` | ❌ Not started | RFC concept only |
| PR creation from `bkt try` | ✅ Implemented | Integrated via standard PR workflow in `try/*` branch flow |

## Implementation Plan

### Phase 1: Safety Analysis

1. Add pre-install file list inspection
2. Warn on high-risk package content (shared libraries, system units, core runtime paths)
3. Support explicit override for risky installs

### Phase 2: COPR Integration

1. Add first-class COPR enable/install UX in `bkt try`
2. Record COPR repo intent in manifest for generation
3. Handle keys and reproducible repo configuration in image builds

### Phase 3: Convergence Commands

1. Implement `bkt admin update` orchestration (capture → commit/push → CI wait → stage)
2. Implement `bkt upgrade` user-focused upgrade command
3. Align output and status reporting with existing `bkt` UX conventions
# RFC 0044: `bkt try` — Tier 1 Preview Mechanism

- **Status**: Draft
- **Extends**: [RFC-0004](0004-bkt-admin.md) (Tier 1 — Image-Bound State)

## Summary

`bkt try` provides **immediate preview of Tier 1 changes** while the normal
pipeline (manifest → PR → CI build → bootc upgrade) runs in the background.
It uses a transient `/usr` overlay that is lost on reboot, ensuring the system
always converges to the declared image state.

This is not a separate tier — it's a UX optimization that makes Tier 1 changes
feel as immediate as Tier 2, while preserving the declarative model.

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
  ├─ Prepare: ensure runtime environment for package installation
  │   └─ mkdir -p /var/lib/rpm-state (required for RPM scriptlets)
  │
  ├─ Install: sudo /usr/bin/dnf5 install -y <package>
  │   └─ Bypasses Bazzite's dnf wrapper
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

### Filesystem Topology

On an atomic Linux system (Bazzite/bootc), the filesystem has distinct zones:

| Path        | Mount Type             | Writable?                  | Persists Reboot? | Notes                      |
| ----------- | ---------------------- | -------------------------- | ---------------- | -------------------------- |
| `/`         | composefs              | ❌ No                      | N/A (immutable)  | Root filesystem from image |
| `/usr`      | composefs (or overlay) | ❌ No (✅ with usroverlay) | ❌ No            | Where packages install     |
| `/etc`      | btrfs subvol           | ✅ Yes                     | ✅ Yes           | System configuration       |
| `/var`      | btrfs subvol           | ✅ Yes                     | ✅ Yes           | Variable data, logs, state |
| `/var/home` | btrfs subvol           | ✅ Yes                     | ✅ Yes           | User home directories      |

The key insight: **`/etc` and `/var` are always writable and persistent**, even
when `/usr` is locked. This means:

- Systemd unit enablement (symlinks in `/etc/systemd/system/`) persists
- SELinux policy store (`/etc/selinux/targeted/active/`) is writable
- Package state (`/var/lib/rpm-state/`) is writable

### `rpm-ostree usroverlay` (aka `ostree admin unlock`)

- Mounts a **writable tmpfs** overlay on top of the composefs `/usr`
- All writes go to the tmpfs upper layer (RAM-backed)
- Reads fall through to the immutable lower layer
- **Transient by default** — lost on reboot
- `--hotfix` flag would persist across reboots, but we explicitly
  do NOT use this — transience is a feature, not a bug
- Requires root (polkit rule already exists for wheel group)
- Idempotent-ish: errors if already unlocked, but we can detect and skip

### Environment Preparation

RPM scriptlets expect certain directories to exist that may not be present on
a fresh atomic system (since packages are normally installed during image build,
not at runtime). Before running `dnf5 install`, `bkt try` must ensure:

```bash
# Required for RPM scriptlet state tracking
mkdir -p /var/lib/rpm-state
```

Without this, packages with `%pre` scriptlets (especially SELinux policy packages
like `*-selinux`) will fail with errors like:

```
cp: cannot create regular file '/var/lib/rpm-state/file_contexts': No such file or directory
```

The package files may still be extracted to `/usr`, but the scriptlet failure
means the package won't be properly registered and post-install actions (like
loading SELinux modules) won't run.

### Bazzite-Specific: Bypassing the dnf Wrapper

Bazzite ships a wrapper script at `/usr/bin/dnf` that blocks `install`/`remove`
commands on the host (redirecting users to documentation). The wrapper passes
through to `dnf5` inside containers.

`bkt try` must call `/usr/bin/dnf5` directly to bypass this wrapper:

```rust
// NOT: Command::new("dnf")
Command::new("/usr/bin/dnf5")
    .args(["install", "-y", package])
```

### What works in the overlay

- `dnf5 install` — installs packages, creates files in `/usr`
- Binaries are immediately available in `$PATH`
- Systemd units can be enabled (symlinks land in `/etc`, which persists!)
- SELinux modules can be loaded (policy store is in `/etc`)
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

### Side Effects That Persist

Because `/etc` and `/var` are always writable and persistent, some side effects
of `bkt try` survive reboot even though the package itself is gone:

| Side Effect           | Location                        | Persists? | Consequence                   |
| --------------------- | ------------------------------- | --------- | ----------------------------- |
| Systemd unit enabled  | `/etc/systemd/system/*.wants/`  | ✅ Yes    | Dangling symlink after reboot |
| SELinux module loaded | `/etc/selinux/targeted/active/` | ✅ Yes    | Module remains (harmless)     |
| Config files created  | `/etc/<package>/`               | ✅ Yes    | Orphaned config               |
| State/cache files     | `/var/lib/<package>/`           | ✅ Yes    | Orphaned data                 |

This is generally harmless — systemd ignores dangling symlinks, SELinux modules
for missing packages are inert, and orphaned config/data doesn't affect the system.
However, `bkt try --status` should surface these artifacts for awareness.

**Important:** If the user runs `bkt try <package>` and then decides NOT to merge
the PR, they should run `bkt try --cleanup <package>` to remove the persistent
side effects. (This is a Phase 2 feature.)

## Relationship to the Tier Model

The system has two tiers based on **change mechanism**:

- **Tier 1** ([RFC-0004](0004-bkt-admin.md)): Image-bound state. Change requires
  manifest → PR → CI build → bootc upgrade → reboot. No local modification possible.

- **Tier 2** ([RFC-0007](0007-drift-detection.md)): Runtime-persistent state.
  Change can happen locally, takes effect immediately, captured to manifest for
  reproducibility.

`bkt try` is **not a separate tier** — it's a **preview mechanism for Tier 1**.

The state being modified (system packages in `/usr`) is Tier 1 state. The change
mechanism is still Tier 1 (manifest → PR → build → upgrade). What `bkt try` adds
is **immediate preview**: you can use the package now while the pipeline runs.

| Aspect                 | Tier 1 (normal)      | Tier 1 (with `bkt try`)     | Tier 2          |
| ---------------------- | -------------------- | --------------------------- | --------------- |
| State location         | Image                | Image (+ transient overlay) | Runtime         |
| Change mechanism       | PR → build → upgrade | PR → build → upgrade        | Local + capture |
| Available immediately? | No                   | **Yes (preview)**           | Yes             |
| Survives reboot?       | Yes (after upgrade)  | **No (until upgrade)**      | Yes             |

The key property: `bkt try` **always converges to Tier 1**. The overlay is lost
on reboot, and the image (built from the manifest change) takes over. If the PR
is never merged, the system returns to its declared state on reboot.

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

## Design Decisions

### PR Workflow

Multiple `bkt try` invocations within the same session accumulate into a single
`try/*` branch and PR. A new terminal session starts a fresh branch. This
balances audit clarity with avoiding PR noise when exploring multiple packages.

```
$ bkt try ripgrep
→ PR #125 created: "feat(try): add ripgrep"

$ bkt try fd-find
→ PR #125 updated: "feat(try): add ripgrep, fd-find"

# Later, new session:
$ bkt try strace
→ PR #126 created: "feat(try): add strace"
```

### Privilege Escalation

`bkt try` uses `pkexec` for privileged operations, consistent with other `bkt admin`
commands. A polkit rule (`50-bkt-try.rules`) grants wheel group members passwordless
access to:

- `/usr/bin/rpm-ostree` (for `usroverlay`)
- `/usr/bin/dnf5` (for package installation)
- `/usr/bin/mkdir` (for creating `/var/lib/rpm-state`)

The Bazzite dnf wrapper is bypassed by calling `/usr/bin/dnf5` directly.

### Memory Budget

Before installing, `bkt try` queries the package download size via `dnf5 info`.
If the package exceeds 100MB, a warning is displayed:

```
⚠ cockpit-machines requires 450MB. The overlay is RAM-backed.
  Continue? [y/N]
```

This prevents accidental RAM exhaustion from large packages.

### Interaction with `bootc upgrade`

**Verified empirically:** The overlay survives `bootc upgrade`. The upgrade
stages a new deployment to be activated on reboot, but does not affect the
running system. The overlay remains on the current deployment's `/usr` until
reboot.

This is the desired behavior: you can `bkt try`, the PR merges, CI builds,
`bootc upgrade` stages the new image, and on reboot the overlay disappears
but the package is now baked into the real image.

### Persistent Side Effects

`bkt try` tracks side effects in `~/.local/state/bkt/try-pending.json`:

```json
{
  "cockpit": {
    "installed_at": "2026-02-18T23:02:00Z",
    "pr": 125,
    "branch": "try/cockpit",
    "services_enabled": ["cockpit.socket"],
    "selinux_modules": ["cockpit"]
  }
}
```

This enables:

- `bkt try --status` to show what's pending
- `bkt try --cleanup <package>` to undo persistent artifacts if the PR is abandoned
- Automatic cleanup prompts when a `try/*` PR is closed without merging

### Service Enablement

When a package ships a systemd unit with a matching name (e.g., `cockpit` →
`cockpit.socket` or `cockpit.service`), `bkt try` offers to enable it:

```
$ bkt try cockpit
→ Installing cockpit...
✓ cockpit installed
→ Found cockpit.socket. Enable it? [Y/n] y
→ Enabling cockpit.socket...
✓ cockpit.socket enabled (listening on :9090)
→ Added cockpit to system-packages.json
→ Added cockpit.socket (enabled) to systemd-services.json
→ PR #125 created
```

The service enablement is also captured in the manifest, so the final image
will have the service enabled by default.

### COPR and Third-Party Repos

COPR support is deferred to Phase 3. It adds complexity around repo enablement,
GPG key trust, and manifest representation. For now, `bkt try` only supports
packages from configured Fedora repos.

## Implementation Plan

### Phase 1: `bkt try` for RPM packages

1. Add `TryCommand` to `bkt/src/commands/`
2. Implement overlay unlock (detect state, call `rpm-ostree usroverlay`)
3. Implement environment preparation (`mkdir -p /var/lib/rpm-state`)
4. Implement `dnf5 install` via direct `/usr/bin/dnf5` call (bypassing wrapper)
5. Manifest update + containerfile regeneration (existing infrastructure)
6. PR creation (existing `bkt --pr` infrastructure)
7. Polkit rule update if needed

### Phase 2: Status and cleanup

1. `bkt try --status` comparing overlay vs manifest vs base image
2. `bkt try --cleanup <package>` to remove persistent side effects:
   - Disable systemd units that were enabled
   - Remove orphaned config in `/etc`
   - Optionally remove SELinux modules
3. Integration with `bkt status` dashboard

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
