# RFC 0053: Bootstrap and Repo Discovery

- **Status**: Partially Implemented
- **Created**: 2026-02-24
- **Updated**: 2026-02-24
- **Absorbs**: [RFC-0051](0051-boot-services.md) (boot services)
- **Related**: [RFC-0052](canon/0052-manifest-lifecycle.md) (manifest lifecycle), [RFC-0054](0054-change-workflow.md) (change workflow)

> **Implementation Status:**
>
> | Feature | Status | Notes |
> |---------|--------|-------|
> | Bootstrap (flatpak/extensions/gsettings/shims) | ✅ Implemented | `scripts/bootc-bootstrap` (shell) |
> | Boot-time capture (apply-boot) | ✅ Implemented | `scripts/bootc-apply` (shell) |
> | Systemd units | ✅ Implemented | `bootc-bootstrap.service`, `bootc-apply.service` |
> | Repo discovery (cwd walk-up + cache) | ✅ Implemented | `find_repo_path()` in `repo.rs` |
> | Repo clone during bootstrap | ✅ Implemented | `clone_repo()` in `bootc-bootstrap` script |
> | `bkt bootstrap` CLI command | ❌ Not started | Bootstrap is shell scripts, not a bkt command |
> | `bkt admin apply-boot` CLI command | ❌ Not started | Apply-boot is a shell script |

## Summary

Bootstrap is a one-time transition: system manifests seed the initial state,
then the repo becomes the single source of truth for all subsystems.
Repo discovery provides a consistent way to locate that repo from any context.

## Motivation

Bootstrap is distinct from normal runtime operations:

- It is the only time system manifests are read.
- It establishes the initial repo and the local cache of its path.
- After it completes, every `bkt` command uses repo manifests and the system
  manifests are ignored permanently.

This aligns with RFC-0052: the repo is the single source of truth.
Bootstrap is the transition mechanism that makes that possible.

## System Manifests

System manifests exist only to seed bootstrap.

- During image build, `manifests/*.json` are copied to
  `/usr/share/bootc-bootstrap/`.
- These are read-only snapshots of the build-time repo state.
- Only `bkt bootstrap` reads them, and only on first login.
- No other command reads from `/usr/share/bootc-bootstrap/`.

## Bootstrap Sequence

`bkt bootstrap` runs on first login via a systemd user unit.
It performs the following steps in order:

1. Check a completion marker and exit early if bootstrap already ran.
2. Apply flatpak remotes from system manifests.
3. Apply flatpak apps from system manifests.
4. Apply gsettings from system manifests.
5. Apply GNOME extensions from system manifests.
6. Clone the user repo using `/usr/share/bootc/repo.json` coordinates.
7. Cache the repo path to `~/.local/state/bkt/repo-path`.
8. Record completion marker to avoid re-running.

The completion marker is stored at `~/.local/state/bkt/bootstrap.done`.

## Repo Discovery

All commands that need manifests call `find_repo_path()`.
It uses a strict fallback chain:

1. Walk up from `cwd` looking for a `manifests/` directory.
2. Read cached path from `~/.local/state/bkt/repo-path`.
3. Clone from GitHub using `/usr/share/bootc/repo.json` coordinates.

Rules:

- When step 1 succeeds, the cache is updated to the discovered path.
  This makes repo moves self-healing.
- Step 3 is the first-boot fallback and only runs if steps 1 and 2 fail.

Repo tooling:

- `bkt repo path` prints the resolved repo path.
- `bkt repo set <path>` manually overrides the cached path.

## Apply-Boot (Post-Boot Capture)

`bkt admin apply-boot` runs at system boot via a systemd system unit.

Behavior:

1. Check if the deployment changed since the last boot.
2. If changed, run `bkt capture` to record layered packages.
3. Record the current deployment checksum to prevent re-running.

Marker file: `/var/lib/bkt/last-applied-deployment`.

## The Transition

Bootstrap is a one-way transition:

- System manifests under `/usr/share/bootc-bootstrap/` are read once.
- After the repo is cloned, all `bkt` commands operate on repo manifests.
- The system manifests are never read again.

This is the moment where control passes from image state to repo state.

## Implementation: Code Changes Required

1. Add `/usr/share/bootc/repo.json` parsing and GitHub clone logic.
2. Implement `find_repo_path()` with the three-step fallback chain.
3. Add repo path cache read/write in `~/.local/state/bkt/repo-path`.
4. Add `bkt repo path` and `bkt repo set <path>` commands.
5. Update bootstrap to read manifests only from `/usr/share/bootc-bootstrap/`.
6. Add bootstrap completion marker at `~/.local/state/bkt/bootstrap.done`.
7. Ensure all manifest-loading code paths call `find_repo_path()`.
8. Replace boot-time scripts with `bkt` commands (see unit files below).

## Systemd Units

`/etc/systemd/user/bkt-bootstrap.service`:

```ini
[Unit]
Description=First-login bootstrap (Flatpak + GNOME + repo clone)
After=graphical-session.target

[Service]
Type=oneshot
ExecStart=/usr/bin/bkt bootstrap
RemainAfterExit=yes

[Install]
WantedBy=default.target
```

`/etc/systemd/system/bkt-apply-boot.service`:

```ini
[Unit]
Description=Capture layered packages after image deployment
After=local-fs.target network-online.target
Wants=network-online.target
ConditionPathExists=/usr/bin/bkt

[Service]
Type=oneshot
ExecStart=/usr/bin/bkt admin apply-boot
RemainAfterExit=yes
StandardOutput=journal
StandardError=journal
TimeoutStartSec=600

[Install]
WantedBy=multi-user.target
```
