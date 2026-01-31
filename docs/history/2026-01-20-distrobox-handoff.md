# Distrobox Migration Handoff (2026-01-20)

## Summary

We successfully migrated the dev workflow from toolbox-first to distrobox host-first, using a deterministic manifest-driven approach. The key change is that we no longer discover binary locations at runtime; instead we declare them in `manifests/distrobox.json`, generate `distrobox.ini`, and apply exports via `bkt distrobox apply`.

This handoff captures what changed, what is now canonical, and how to continue safely from a host VS Code window (not remote). It also documents any assumptions and the operational state at the end.

---

## High-Level Goal Achieved

- ✅ Distrobox container created and configured
- ✅ Exported shims created on host for key dev tools
- ✅ PATH configured via environment.d for GUI / VS Code session
- ✅ RFC updated to reflect deterministic, manifest-first policy
- ✅ Implemented distrobox manifest, schema, and capture/apply commands

The workflow is now:

1. Edit `manifests/distrobox.json`
2. Run `bkt distrobox apply`
3. `distrobox.ini` is regenerated and container is assembled
4. Exported bins are created deterministically

---

## Canonical Files

### Manifest (source of truth)

- `manifests/distrobox.json`
  - Contains explicit exported binary paths, PATH policy, and container image.
  - Container image: `ghcr.io/ublue-os/bazzite-dx:latest`
  - Includes `init_hooks` for toolchain setup (proto installation).

### Generated target

- `distrobox.ini`
  - Generated from the manifest; includes expanded `~` paths.

### Schema

- `schemas/distrobox.schema.json`
  - Generated schema definition for the manifest format.

### RFC

- `docs/rfcs/0017-distrobox-integration.md`
  - Updated to reflect deterministic, policy‑based binary locations and path configuration.

---

## Implemented Code

### New Manifest Type

- `bkt/src/manifest/distrobox.rs`
  - Defines `DistroboxManifest` + `DistroboxContainer`
  - Validates:
    - `image` required
    - `env` may not contain PATH
    - `additional_flags` may not set PATH (`--env=PATH`, `-e PATH`, etc.)

### New Command

- `bkt/src/commands/distrobox.rs`
  - `bkt distrobox apply`
    - Generates `distrobox.ini`
    - Runs `distrobox assemble create --file distrobox.ini`
    - Runs `distrobox export` for each declared bin
    - Expands `~` to $HOME for runtime; collapses back to `~` when capturing
  - `bkt distrobox capture`
    - Parses `distrobox.ini` back into manifest
    - Extracts `path` and `env` from `additional_flags`
    - **Note:** Capture was implemented but not tested this session.

### Bug Fixed: Home Expansion

The initial implementation didn't expand `~` in paths before passing to `distrobox-export`. This caused exports to fail because distrobox doesn't expand `~` itself. Fixed by adding `expand_home()` (for apply) and `collapse_home()` (for capture) functions that translate between `~` in the manifest and `$HOME` at runtime.

### Integrated Commands

- `bkt/src/main.rs` (CLI wiring)
- `bkt/src/commands/mod.rs` (module registration)
- `bkt/src/commands/apply.rs` (subsystem integration)
- `bkt/src/commands/capture.rs` (subsystem integration)
- `bkt/src/commands/schema.rs` (schema generation)
- `bkt/src/context.rs` (new `CommandDomain::Distrobox` host‑only)

---

## Deterministic Policy Decisions (New Behavior)

### 1) Binary Locations

We do **not** use `which` or runtime discovery. Binary locations are explicit in the manifest.

Current manifest exports:

- `~/.cargo/bin/cargo`
- `~/.cargo/bin/rustc`
- `~/.cargo/bin/cargo-clippy`
- `~/.cargo/bin/cargo-fmt`
- `~/.proto/shims/node`
- `/usr/bin/npm`
- `/usr/local/bin/nu`

### 2) PATH

PATH is declared in the manifest `path` array and rendered via `--env=PATH=...` in `additional_flags`.

Current manifest path:

- `~/.local/bin`
- `~/.cargo/bin`
- `~/.proto/bin`
- `$PATH`

No toolbox‑specific directories are included now.

### 3) Export Location

Exported shims are targeted at `~/.local/bin` (manifest `exported_bins_path`).

### 4) Environment Variables

`env` is allowed, except `PATH` (must use `path`). This is enforced in validation.

---

## Operational State (End of Session)

### Host configuration

- `~/.config/environment.d/10-distrobox-exports.conf` created with:
  - `PATH="$HOME/.local/bin:$PATH"`

### Distrobox container

- `bootc-dev` container created and configured

### Exported shims

Verified created in host `~/.local/bin`:

- cargo
- rustc
- cargo-clippy
- cargo-fmt
- node
- npm
- nu

### Apply run

`bkt distrobox apply` executed successfully (via `cargo run`), exporting all bins.

### Capture run

`bkt distrobox capture` was implemented but **not tested** this session. If you need to capture the current state back to the manifest, test this command first.

---

## How to Continue in Host VS Code Window

1. Close the Dev Container window.
2. Open the repo on host normally.
3. Ensure GUI PATH has reloaded (log out/in or restart VS Code).
4. Use `cargo`, `rustc`, `npm`, etc. directly (shims should call into container).

If PATH hasn’t reloaded yet:

- Open a terminal and run: `export PATH="$HOME/.local/bin:$PATH"` for the session.

---

## Notes on Chat Persistence

Copilot chat is tied to the **remote window**, so it doesn’t persist when switching to host. This handoff file is meant to preserve state so you can continue in the host window.

---

## Follow‑ups / TODOs

- Install/ship the updated `bkt` binary on host so `bkt distrobox apply` works without `cargo run`.
- Decide if the manifest should move toolchain installs into the image (deterministic image-level toolchain vs. home-managed toolchain policy).
- Extend `bkt status` to show distrobox manifest status (optional).
- Decide if `bkt dev` should become a wrapper for `bkt distrobox` during transition.

---

# Session: 2026-01-22

## Bugs Fixed

### 1. Git checkout syntax error in PR workflow (`bkt/src/pr.rs`)

The PR creation workflow failed with:

```
fatal: 'bkt/dnf-install-gcc-1769115367' is not a commit and a branch '--' cannot be created from it
```

**Root cause**: Incorrect `--` placement in git checkout commands:

- Line 476: `["checkout", "-b", "--", &branch]` → `["checkout", "-b", &branch]`
- Line 537: `["checkout", "--", &config.default_branch]` → `["checkout", &config.default_branch]`

### 2. PATH configuration causing host toolchain usage

When running `cargo build` from VS Code, it used the host's `~/.cargo/bin/cargo` directly instead of the distrobox shim, causing "linker `cc` not found" errors.

**Root cause**: The `environment.d` config appended to `$PATH` instead of setting a complete PATH:

```
# Old (broken)
PATH="$HOME/.local/bin:$HOME/.local/bin/distrobox:$PATH"

# New (fixed)
PATH="$HOME/.local/bin/distrobox:$HOME/.local/bin:/usr/local/sbin:/usr/local/bin:/usr/bin"
```

**Additional cleanup**:

- Neutered `~/.cargo/env` to prevent accidental sourcing
- Simplified `~/.bashrc` to remove PATH manipulation (environment.d handles it)
- Simplified `~/.bash_profile` and `~/.profile`

## Key Findings: Distrobox Transparency

Investigated whether distrobox is transparent enough that tools don't need to know they're containerized.

### What works in distrobox (shared with host):

- ✅ D-Bus session bus (`/run/user/1000/bus`)
- ✅ `systemctl --user` commands
- ✅ `systemd-run --user --scope` (creating scopes)
- ✅ PID namespace (PID 1 is host systemd)
- ✅ Full filesystem access (home bind-mounted)

### What doesn't work:

- ❌ Direct cgroup writes (`/sys/fs/cgroup` is virtualized)
- ⚠️ Setuid binaries (depends on SELinux context, same as host)

### Conclusion for shim strategy:

| Tool type                | Shim behavior                         | Examples               |
| ------------------------ | ------------------------------------- | ---------------------- |
| Filesystem tools         | distrobox-enter wrapper (transparent) | rg, cargo, node, rustc |
| System integration tools | Direct symlink (host_only)            | locald, bkt            |

Most tools work fine through distrobox shims. The container is invisible for CLI tools.

## Design Decision: `host_only` Shim Option

For the rare case of tools built in the container that must run directly on the host (e.g., `locald` with setuid requirements), add a manifest option:

```json
{
  "shims": [
    { "name": "cargo", "source": "~/.cargo/bin/cargo" },
    { "name": "locald", "source": "~/.cargo/bin/locald", "host_only": true }
  ]
}
```

When `host_only: true`:

- Shim generator creates a direct symlink instead of distrobox-enter wrapper
- Binary runs on host, not through container

CLI:

```bash
bkt shim add locald              # Normal shim (distrobox wrapper)
bkt shim add locald --host-only  # Direct link, no container
```

This is expected to be rare — most tools work fine through shims.

## bkt Delegation Logic

The existing `bkt` delegation code in `delegation.rs` is from the toolbx era. It detects container environment and delegates host commands via `flatpak-spawn --host /usr/bin/bkt`.

### Recommendation: Remove Entirely

**The delegation logic should be removed.** It was designed for a world where `bkt` might run inside the container and need to "escape" to the host. With the host-only shim model, `bkt` always runs on the host.

**Why delegation is unnecessary**:

1. **`bkt` runs on host** — With host-only shim, `bkt` executes directly on the host, never inside distrobox
2. **Commands invoke container operations directly** — `bkt dev dnf install` should call `distrobox enter bootc-dev -- dnf install`, not re-exec itself inside the container
3. **Distrobox transparency** — For operations that need host (flatpak, systemctl), distrobox already shares those namespaces

**What to remove**:

- `bkt/src/delegation.rs` — entire module
- `maybe_delegate()` call in `main.rs`
- `RuntimeEnvironment` enum (or simplify)
- `--no-delegate` flag

**What to change**:

- `bkt dev dnf install X` → invoke `distrobox enter bootc-dev -- dnf install X`
- `bkt dev` subcommands orchestrate container operations, don't delegate

**No action taken this session** — just documenting the decision for future refactoring.

## Files Changed

- `bkt/src/pr.rs` - Fixed git checkout syntax
- `skel/.config/environment.d/10-distrobox-exports.conf` - Complete PATH, no inheritance
- `docs/VISION.md` - Added Host PATH Architecture section
- `docs/rfcs/0018-host-only-shims.md` - New RFC for host_only shim option
- `CURRENT.md` - Added testing ideas backlog

---

## Verification Commands (Host)

- `cargo --version`
- `rustc --version`
- `node --version`
- `npm --version`

These should now delegate into the distrobox container via the exported shims.
