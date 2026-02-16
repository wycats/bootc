# RFC 0045: Aggressive Layer Minimization

## Status

Draft (revised after prepare audit)

## Problem

The image assembly stage (`FROM base AS image`) currently has **75
RUN/COPY instructions**, each producing a Docker layer. Because Docker
invalidates every layer after the first changed one, a single config
file edit near the top of the stage cascades into **50+ rebuilt layers**.

Concrete examples of the cascade:

| Change                                | First invalidated layer | Layers rebuilt |
| ------------------------------------- | ----------------------- | -------------- |
| `system/fontconfig/99-emoji-fix.conf` | Layer 23                | 53             |
| `manifests/flatpak-apps.json`         | Layer 30                | 46             |
| `skel/.config/nushell/config.nu`      | Layer 72                | 4              |
| Any external RPM version bump         | Layer 1 (dl-\* COPY)    | 75             |

This wastes CI build time, registry bandwidth, and `bootc upgrade`
download size. The parallel pre-build stages (dl-\*, fetch-\*, build-\*)
are well-isolated, but their outputs are assembled into a single serial
stage that undoes most of the benefit.

## Design Principle: Minimize Delta Layers

The goal is not just fewer layers — it's fewer **changed** layers for
any given commit. The ideal is:

> For any single-concern change, at most **2–3 layers** in the final
> image should differ from the previous build.

## Why Not `COPY --from=collector / /`?

The naive approach — build everything in parallel collector stages and
merge with `COPY --from=collector / /` — is **broken**. Each collector
starts `FROM base`, so `COPY --from=collector / /` copies the **entire
base image filesystem** (~2-4GB), not just the delta. The last COPY
wins for every file, destroying the RPM database and producing a
corrupt image.

## Strategy: Hybrid Collector Architecture

### Core Idea

Use a **hybrid** approach:

1. **RPM work stays inline** in the `image` stage — `dnf install`
   needs the base image's package manager and RPM database, and the
   results can't be cleanly extracted as a delta.

2. **`collect-config`** — a parallel stage that assembles all static
   configuration (manifests, scripts, systemd units, skel, optional
   features, host shims, keyd config, polkit rules, etc.) into a
   filesystem subtree at known paths. Imported via targeted
   directory-level COPYs.

3. **Upstream COPYs** remain as individual `COPY --from=fetch-*`
   instructions (already enumerated by the generator).

4. **`fc-cache`** runs in the final `image` stage after all COPYs,
   since it depends on both RPM-installed fonts and upstream fonts.

### Stage Architecture

```
tools ──→ base ──┬──→ dl-code          ──┐
                 ├──→ dl-microsoft-edge ──┤
                 ├──→ dl-1password      ──┤
                 ├──→ fetch-starship    ──┤
                 ├──→ fetch-lazygit     ──┤
                 ├──→ fetch-getnf       ──┤
                 ├──→ fetch-bibata      ──┤
                 ├──→ fetch-jbmono      ──┤
                 ├──→ build-keyd        ──┤
                 ├──→ fetch-whitesur    ──┤
                 │                        │
                 ├──→ collect-config ─────┤  (parallel with all above)
                 │                        │
                 └──→ image ←─────────────┘
```

### The `collect-config` Stage

This is the biggest win. All static configuration — manifests, scripts,
systemd units, skel files, polkit rules, keyd config, optional features,
host shims, kernel args — gets assembled in a parallel stage that starts
`FROM scratch` (not `FROM base`) to produce a clean overlay.

```dockerfile
FROM scratch AS collect-config

# System config
COPY system/keyd/default.conf /etc/keyd/default.conf
COPY system/polkit-1/rules.d/50-bkt-admin.rules /etc/polkit-1/rules.d/50-bkt-admin.rules
COPY system/etc/topgrade.toml /etc/topgrade.toml
COPY system/etc/sysctl.d/99-bootc-vm-tuning.conf /etc/sysctl.d/99-bootc-vm-tuning.conf
COPY system/etc/opt/edge/policies/managed/performance.json /etc/opt/edge/policies/managed/performance.json

# Bootstrap manifests
COPY manifests/flatpak-remotes.json /usr/share/bootc-bootstrap/flatpak-remotes.json
COPY manifests/flatpak-apps.json /usr/share/bootc-bootstrap/flatpak-apps.json
COPY manifests/gnome-extensions.json /usr/share/bootc-bootstrap/gnome-extensions.json
COPY manifests/gsettings.json /usr/share/bootc-bootstrap/gsettings.json
COPY manifests/host-shims.json /usr/share/bootc-bootstrap/host-shims.json
COPY repo.json /usr/share/bootc/repo.json

# Scripts and CLI
COPY scripts/bootc-bootstrap /usr/bin/bootc-bootstrap
COPY scripts/bootc-apply /usr/bin/bootc-apply
COPY scripts/bootc-repo /usr/bin/bootc-repo
COPY scripts/bkt /usr/bin/bkt

# Systemd units
COPY systemd/user/bootc-bootstrap.service /usr/lib/systemd/user/bootc-bootstrap.service
COPY systemd/user/bootc-capture.service /usr/lib/systemd/user/bootc-capture.service
COPY systemd/user/bootc-capture.timer /usr/lib/systemd/user/bootc-capture.timer
COPY systemd/system/bootc-apply.service /usr/lib/systemd/system/bootc-apply.service
COPY systemd/user/dbus-broker.service.d/override.conf /usr/lib/systemd/user/dbus-broker.service.d/override.conf

# Skel
COPY skel/.config/nushell/config.nu /etc/skel/.config/nushell/config.nu
COPY skel/.config/nushell/env.nu /etc/skel/.config/nushell/env.nu

# Optional features (staging area — conditionals applied in image stage)
COPY system/remote-play/ /usr/share/bootc-optional/remote-play/
COPY system/NetworkManager/conf.d/ /usr/share/bootc-optional/NetworkManager/conf.d/
COPY system/asahi/ /usr/share/bootc-optional/asahi/
COPY system/systemd/journald.conf.d/ /usr/share/bootc-optional/systemd/journald.conf.d/
COPY system/systemd/logind.conf.d/ /usr/share/bootc-optional/systemd/logind.conf.d/

# ujust and distrobox
COPY ujust/60-custom.just /usr/share/ublue-os/just/60-custom.just
COPY distrobox.ini /etc/distrobox/distrobox.ini

# Fontconfig override (fc-cache runs in image stage after all fonts are present)
COPY system/fontconfig/99-emoji-fix.conf /etc/fonts/conf.d/99-emoji-fix.conf
```

**Key design choice**: `collect-config` starts `FROM scratch`, not
`FROM base`. This means `COPY --from=collect-config / /` only transfers
the files explicitly placed there — no base image duplication. The
tradeoff is that `collect-config` cannot run any `RUN` instructions
(no shell, no filesystem). All it does is COPY files into place.

The operations that require `RUN` (chmod, symlinks, host shims, kernel
args, optional feature conditionals) stay in the `image` stage as a
**single consolidated RUN** instruction.

**Trigger**: any config file, manifest, script, systemd unit, or skel change.
**Delta**: 1 layer in the final image (the `COPY --from=collect-config`).

### The New `image` Stage

```dockerfile
FROM base AS image

# ── RPMs (inline — needs package manager) ────────────────────────────────────
COPY --from=dl-code /rpms/ /tmp/rpms/
COPY --from=dl-microsoft-edge /rpms/ /tmp/rpms/
COPY --from=dl-1password /rpms/ /tmp/rpms/
RUN dnf install -y /tmp/rpms/*.rpm <system-packages> && dnf clean all
RUN <opt-relocation + tmpfiles>
RUN rm -rf /tmp/rpms /tmp/external-repos.json /usr/bin/bkt-build

# ── Upstream binaries/fonts/icons ────────────────────────────────────────────
COPY --from=fetch-starship /usr/bin/starship /usr/bin/starship
COPY --from=fetch-lazygit /usr/bin/lazygit /usr/bin/lazygit
COPY --from=fetch-getnf /usr/bin/getnf /usr/bin/getnf
COPY --from=fetch-bibata-cursor /usr/share/icons/... /usr/share/icons/...
COPY --from=fetch-jetbrains-mono-nerd-font /usr/share/fonts/... /usr/share/fonts/...
COPY --from=build-keyd /usr/bin/keyd /usr/bin/keyd
# ... (remaining keyd + whitesur COPYs)

# ── Configuration overlay ────────────────────────────────────────────────────
COPY --from=collect-config / /

# ── Post-overlay setup (single RUN) ─────────────────────────────────────────
RUN set -eu; \
    chmod 0755 /usr/bin/bootc-bootstrap /usr/bin/bootc-apply ...; \
    <keyd-enable-symlink>; \
    <systemd-enable-symlinks>; \
    <host-shims>; \
    <kernel-args>; \
    <optional-feature-conditionals>

# ── Font cache (depends on RPM fonts + upstream fonts + fontconfig) ──────────
RUN fc-cache -f

# ── RPM snapshot ─────────────────────────────────────────────────────────────
RUN rpm -qa --qf '%{NAME}\t%{EVR}\n' | sort > /usr/share/bootc/rpm-versions.txt
```

**~20 layers** in the image stage, down from 75. The critical
improvement: config changes only rebuild **2 layers** (collect-config

- the consolidated RUN), not 40-53.

### Delta Analysis: Before vs. After

| Change                    | Before (layers rebuilt) | After (layers rebuilt)                                        |
| ------------------------- | ----------------------- | ------------------------------------------------------------- |
| VS Code RPM update        | 75                      | ~18 (RPM COPY cascades)                                       |
| Starship version bump     | 53                      | ~10 (from upstream COPY onward)                               |
| Config file edit          | 4–53                    | **3** (collect-config COPY + setup RUN + fc-cache + snapshot) |
| Nushell skel change       | 4                       | **3** (same — config overlay)                                 |
| Multiple concerns at once | 75                      | 3–18 (depends on what changed)                                |

The biggest win is config changes: from up to 53 layers down to 3.
RPM changes still cascade through the inline section, but that's
unavoidable without the broken `COPY / /` approach.

### Why `FROM scratch` for collect-config

Using `FROM scratch` instead of `FROM base` solves the fundamental
problem identified in the prepare audit:

- `FROM base` + `COPY --from=collector / /` copies the **entire base
  image** (~2-4GB), destroying the RPM database and bloating layers.
- `FROM scratch` + `COPY --from=collector / /` copies **only the files
  explicitly placed in the collector** — a clean overlay of config
  files, typically a few hundred KB.

The tradeoff is no `RUN` capability in the collector (no shell). But
config assembly is almost entirely COPY operations — the few `RUN`
operations (chmod, symlinks, conditionals) consolidate into a single
RUN in the `image` stage.

## Implementation Phases

### Phase 1: Hybrid Collector (High Impact)

Restructure `containerfile.rs` to emit:

1. **`emit_collect_config()`** — new function that emits the
   `FROM scratch AS collect-config` stage with all static config COPYs
2. **Refactored `emit_image_assembly()`** — the `image` stage now has:
   - RPM COPYs + `dnf install` (inline, unchanged)
   - `/opt` relocation + tmpfiles (inline, unchanged)
   - Cleanup (inline, unchanged)
   - Upstream COPYs (individual `COPY --from=fetch-*`, unchanged)
   - `COPY --from=collect-config / /` (new — single config overlay)
   - Consolidated `RUN` for chmod, symlinks, host shims, kernel args,
     optional feature conditionals (new — replaces many individual RUNs)
   - `fc-cache -f` (moved after all COPYs)
   - `rpm -qa` snapshot (unchanged)

The generator already controls the full Containerfile via
`generate_full_containerfile()`, so this is a refactor of
`emit_image_assembly` plus a new `emit_collect_config` called
before it.

**Verification**: `bkt containerfile check` passes; `rpm -qa` diff
between old and new image is empty; binary checksums match.

### Phase 2: Per-Upstream Manifest Fragments (Future)

Currently every fetch-\* stage copies the full `upstream/manifest.json`.
A change to any upstream entry invalidates all fetch stages.

Split into per-upstream fragments so each fetch stage only depends on
its own entry. This is orthogonal to Phase 1 and can be done later.

## Ordering Constraints

The `COPY --from` order in the `image` stage matters for correctness:

1. **RPM COPYs + dnf install** first — establishes package filesystem
2. **`/opt` relocation + tmpfiles** — depends on RPM-installed files
3. **Cleanup** — removes build artifacts
4. **Upstream COPYs** — overlays binaries and fonts from fetch stages
5. **`COPY --from=collect-config / /`** — overlays all static config
   (highest priority — config wins over package defaults)
6. **Consolidated RUN** — chmod, symlinks, host shims, kernel args,
   optional feature conditionals (depends on files from all above)
7. **`fc-cache -f`** — depends on RPM fonts + upstream fonts + fontconfig
8. **`rpm -qa` snapshot** — captures final RPM state

## Edge Cases

### /opt Relocation

The `/opt` relocation (`cp -a /opt/. /usr/lib/opt/`) depends on what
RPMs installed into `/opt`. This stays inline in the `image` stage
immediately after `dnf install`.

### fc-cache

`fc-cache -f` depends on both RPM-installed fonts and upstream fonts
(from fetch stages) plus fontconfig rules (from collect-config). It
runs in the `image` stage after all COPYs — this is the correct
placement since it needs the complete font set.

### Optional Feature Conditionals

The `ARG ENABLE_*` + `RUN if [...]` pattern for optional features
requires a shell. Since `collect-config` is `FROM scratch` (no shell),
the conditional logic stays in the consolidated RUN in the `image`
stage. The optional feature _files_ are staged to
`/usr/share/bootc-optional/` by `collect-config`, and the consolidated
RUN copies them to their final locations based on the ARG values.

### RPM Version Snapshot

`rpm -qa` must run in the final `image` stage after all COPYs, since
it reflects the installed RPM state. This is the last instruction.

### Host Shims

The base64-decode-and-write pattern for host shims requires a shell
(`echo ... | base64 -d > /path`). This stays in the consolidated RUN
in the `image` stage, not in `collect-config`.

### Systemd Enable Symlinks

Creating symlinks like `ln -sf ... /usr/lib/systemd/system/multi-user.target.wants/`
requires a shell. These stay in the consolidated RUN. Multiple services
(keyd, bootc-apply) write to the same `multi-user.target.wants/`
directory, so they must be in the same RUN to avoid conflicts.

## Risks

- **COPY --from overlay semantics**: `COPY --from=collect-config / /`
  does a file-level merge, not a replace. Files from the collector
  overwrite existing files at the same path, but deletions don't
  propagate. This is fine — the collector only adds files.

- **Layer size**: The `COPY --from=collect-config / /` produces one
  layer instead of many small ones. This is actually better for
  `bootc upgrade` since ostree works at the file level, not the layer
  level. For registry pulls, larger layers compress better.

- **Generator complexity**: `emit_image_assembly` is split into two
  functions (`emit_collect_config` + refactored `emit_image_assembly`),
  but each is simpler than the current monolith.

- **FROM scratch limitations**: No RUN capability in `collect-config`.
  All operations requiring a shell (chmod, symlinks, base64 decode,
  conditionals) must be in the `image` stage's consolidated RUN.

## Success Criteria

- Image assembly stage has ~20 layers (down from 75)
- Config-only changes rebuild ≤ 3 layers (collect-config COPY +
  consolidated RUN + fc-cache)
- `bkt containerfile check` passes
- `rpm -qa` output is identical to current image
- CI build time is equal or faster (collect-config runs in parallel)

## Non-Goals

- Reducing the number of parallel pre-build stages (these are already
  well-isolated)
- Changing the base image or package selection
- Modifying the `bkt-build` binary interface
- Full collector approach (collect-rpms, collect-upstream) — blocked
  by the `COPY / /` problem for stages that start `FROM base`
