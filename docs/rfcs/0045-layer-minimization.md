# RFC 0045: Layer Independence

## Status

Implemented (2026-02-22)

## Goal

A change to one concern in the manifest should invalidate **O(1)
layers** in the final image. This property should follow from the
structure, not from careful ordering.

## Background

The image is built with BuildKit (`docker/build-push-action@v6` +
`docker/setup-buildx-action@v3`) and uses multi-stage builds for
parallel fetching and compilation. The generator (`bkt containerfile
generate`) controls the full Containerfile.

### Current Architecture

The Containerfile has good **build parallelism**: dl-\*, fetch-\*,
build-\*, and collect-config stages all run concurrently. But the
final `FROM base AS image` stage assembles their outputs into a
**linear layer stack**. Any change to a layer invalidates every layer
below it.

The image stage currently has **34 layer-producing instructions** (20
COPY + 14 RUN). A config file edit can cascade into 10+ rebuilt
layers. An RPM version bump cascades into all 34.

### Why Linear Layers Are the Wrong Model

The concerns in the image are largely independent:

- External RPMs (code, edge, 1password) don't share files
- Upstream binaries (starship, lazygit, keyd) don't interact
- Config files don't depend on which RPMs are installed
- Wrappers are standalone compiled binaries

Forcing these into a linear stack means a starship bump rebuilds the
keyd, whitesur, config, wrapper, and font-cache layers — none of
which changed.

### Why Multi-Stage Builds Alone Don't Solve This

Multi-stage builds give us parallel _builds_ — the dl-\*, fetch-\*,
and build-\* stages all run concurrently. But they don't give us
independent _output layers_. When the stages are assembled into the
final image via `COPY --from`, those COPYs form a linear sequence.
Changing one still invalidates everything after it.

This distinction matters because it's easy to conflate "parallel
stages" with "independent layers" and conclude the problem is solved.
It isn't. Parallel stages reduce _build time_. Independent layers
reduce _invalidation scope_. They require different mechanisms.

Earthfile (from Earthly) was originally considered for this project
because it models the output image as a DAG of independent concerns
rather than a linear stack. BuildKit's `COPY --link` flag provides
the same independence property within the standard Containerfile
format, without requiring a different build tool.

## Design: `COPY --link` for Layer Independence

BuildKit's `COPY --link` flag creates layers that are **independent
of the layer stack below them**. A linked COPY is computed in
isolation and spliced into the image without invalidating subsequent
layers. This is the mechanism that gives us the O(1) property.

### What `--link` Changes

Without `--link`:

```
Layer N:   COPY --from=fetch-starship ...    ← if this changes
Layer N+1: COPY --from=fetch-lazygit ...     ← this rebuilds (unnecessary)
Layer N+2: COPY --from=build-keyd ...        ← this rebuilds (unnecessary)
```

With `--link`:

```
Layer N:   COPY --link --from=fetch-starship ...   ← if this changes
Layer N+1: COPY --link --from=fetch-lazygit ...    ← untouched
Layer N+2: COPY --link --from=build-keyd ...       ← untouched
```

### What `--link` Cannot Do

`--link` only applies to `COPY` instructions. `RUN` instructions are
always linear — they depend on the full filesystem state of the
previous layer. This means:

- `RUN dnf install` cannot use `--link` (needs base filesystem + RPM DB)
- `RUN fc-cache` cannot use `--link` (needs all fonts present)
- The consolidated post-overlay `RUN` cannot use `--link`

The strategy is to minimize the number of `RUN` layers and maximize
the number of `COPY --link` layers.

## Design: Per-Package RPM Install Stages

The biggest remaining cascade is the RPM install. Currently, all
external RPMs (code, edge, 1password) plus system packages are
installed in a single `RUN dnf install` layer. Bumping Edge rebuilds
a ~2GB layer that includes VS Code and 1Password.

RPM install has distinct phases:

1. **Download** — fetch `.rpm` files (already parallelized in dl-\* stages)
2. **Extract file payloads** — unpack files to the filesystem
3. **Run scriptlets** — `%post` scripts (`ldconfig`, icon cache, etc.)
4. **Update RPM database** — record installation in `/var/lib/rpm/`

Phases 2-4 are currently fused into a single `dnf install`. But `rpm`
natively supports splitting them:

- `rpm -i --nodb --noscripts --nodeps` — extract files only, skip DB and scriptlets
- `rpm -i --justdb` — update DB only, skip file extraction

The file payloads for external RPMs are well-separated:

| RPM                   | Primary location         | Shared dirs                                             |
| --------------------- | ------------------------ | ------------------------------------------------------- |
| code                  | `/usr/share/code/`       | `/usr/bin/code`, `/usr/share/applications/`             |
| microsoft-edge-stable | `/opt/microsoft/msedge/` | `/usr/bin/microsoft-edge-stable`, `/usr/share/appdata/` |
| 1password             | `/opt/1Password/`        | `/usr/share/applications/`, `/usr/share/icons/`         |

The "shared dirs" contain only a few `.desktop` files and icons — no
conflicting files.

### Per-Package Install Stages

Each external RPM gets its own install stage that extracts files
without touching the RPM database:

```dockerfile
FROM base AS install-code
COPY --from=dl-code /rpms/ /tmp/rpms/
RUN rpm -i --nodb --noscripts --nodeps /tmp/rpms/*.rpm && rm -rf /tmp/rpms

FROM base AS install-edge
COPY --from=dl-microsoft-edge /rpms/ /tmp/rpms/
RUN set -eu; \
    rpm -i --nodb --noscripts --nodeps /tmp/rpms/*.rpm; \
    # Relocate /opt to /usr/lib/opt for ostree compatibility
    mkdir -p /usr/lib/opt; \
    cp -a /opt/. /usr/lib/opt/; \
    rm -rf /opt/* /tmp/rpms

FROM base AS install-1password
COPY --from=dl-1password /rpms/ /tmp/rpms/
RUN set -eu; \
    rpm -i --nodb --noscripts --nodeps /tmp/rpms/*.rpm; \
    mkdir -p /usr/lib/opt; \
    cp -a /opt/. /usr/lib/opt/; \
    rm -rf /opt/* /tmp/rpms
```

These stages run in parallel. Each produces a filesystem delta
containing only that package's files.

**`/opt` relocation**: On ostree, `/opt` is a symlink to `/var/opt`
(persistent, mutable) and is NOT updated on upgrade. RPMs that
install to `/opt` (Edge, 1Password) must have their files relocated
to `/usr/lib/opt/` (immutable image layer) during the install stage
itself. This is critical: `COPY --link` layers cannot see the
filesystem below them, so a post-COPY relocation RUN would not find
the files. Each install stage that touches `/opt` handles its own
relocation.

### Assembly with `--link`

In the final image, each package's files are copied independently:

```dockerfile
COPY --link --from=install-code /usr/share/code/ /usr/share/code/
COPY --link --from=install-code /usr/bin/code /usr/bin/code
COPY --link --from=install-code /usr/share/applications/code*.desktop /usr/share/applications/

COPY --link --from=install-edge /usr/lib/opt/microsoft/ /usr/lib/opt/microsoft/
COPY --link --from=install-edge /usr/bin/microsoft-edge-stable /usr/bin/microsoft-edge-stable

COPY --link --from=install-1password /usr/lib/opt/1Password/ /usr/lib/opt/1Password/
COPY --link --from=install-1password /usr/share/applications/1password.desktop /usr/share/applications/
```

Note that Edge and 1Password COPYs reference `/usr/lib/opt/` — the
relocated path from the install stage, not `/opt/`.

Bumping Edge rebuilds only the `install-edge` stage and its COPY
layers. VS Code and 1Password layers are untouched.

### DB Finalization

After all file payloads are in place, a single `RUN` finalizes the
RPM database and runs necessary scriptlets:

```dockerfile
COPY --from=dl-code /rpms/ /tmp/rpms/
COPY --from=dl-microsoft-edge /rpms/ /tmp/rpms/
COPY --from=dl-1password /rpms/ /tmp/rpms/
RUN rpm -i --justdb --nodeps /tmp/rpms/*.rpm && ldconfig && rm -rf /tmp/rpms
```

This layer is small (just DB writes) and rebuilds whenever any
external RPM changes — but it's the only shared layer.

## Target Architecture

```
tools ──→ base ──┬──→ dl-code ──→ install-code ────────┐
                 ├──→ dl-edge ──→ install-edge ────────┤
                 ├──→ dl-1pw  ──→ install-1pw  ────────┤
                 ├──→ fetch-starship ──────────────────┤
                 ├──→ fetch-lazygit ───────────────────┤
                 ├──→ fetch-getnf ─────────────────────┤
                 ├──→ fetch-bibata ────────────────────┤
                 ├──→ fetch-jbmono ────────────────────┤
                 ├──→ build-keyd ──────────────────────┤
                 ├──→ fetch-whitesur ──────────────────┤
                 ├──→ build-wrappers ──────────────────┤
                 ├──→ collect-config ──────────────────┤
                 │                                     │
                 └──→ image ←──────────────────────────┘
                       │
                       ├─ RUN dnf install <system-pkgs>  (no external RPMs)
                       ├─ RUN tmpfiles + cleanup
                       ├─ COPY --link --from=install-code ...
                       ├─ COPY --link --from=install-edge ...
                       ├─ COPY --link --from=install-1pw ...
                       ├─ COPY --link --from=fetch-starship ...
                       ├─ COPY --link --from=fetch-lazygit ...
                       ├─ COPY --link --from=fetch-getnf ...
                       ├─ COPY --link --from=fetch-bibata ...
                       ├─ COPY --link --from=fetch-jbmono ...
                       ├─ COPY --link --from=build-keyd ...
                       ├─ COPY --link --from=fetch-whitesur ...
                       ├─ COPY --link --from=collect-config / /
                       ├─ COPY --link --from=build-wrappers ...
                       ├─ RUN rpm --justdb + ldconfig
                       ├─ RUN consolidated post-overlay setup
                       ├─ RUN fc-cache -f
                       └─ RUN rpm -qa snapshot
```

### The `collect-config` Stage

All static configuration — manifests, scripts, systemd units, skel
files, polkit rules, keyd config, optional feature staging, memory
slices, oomd tuning, nushell config — is assembled in a parallel
stage starting `FROM scratch`:

```dockerfile
FROM scratch AS collect-config
COPY system/fontconfig/99-emoji-fix.conf /etc/fonts/conf.d/99-emoji-fix.conf
COPY system/keyd/default.conf /etc/keyd/default.conf
# ... (all static config COPYs — generated from image-config.json modules)
```

`FROM scratch` means `COPY --from=collect-config / /` transfers only
the files explicitly placed there — no base image duplication. The
tradeoff is no `RUN` capability (no shell). Operations requiring a
shell (chmod, symlinks, host shims, optional feature conditionals)
go in the consolidated `RUN` in the image stage.

### The Consolidated `RUN`

A single post-overlay `RUN` handles everything that needs a shell:

- `chmod` on scripts and binaries
- systemd enable symlinks
- host shims (base64 decode + chmod + symlink)
- optional feature conditionals (`if [ "$ARG" = "1" ]; then install ...; fi`)
- kernel arg file creation
- staging directory creation

This replaces the current 6 optional-feature RUNs + 1 host-shims
RUN + the existing consolidated RUN = **8 RUNs collapsed into 1**.

### Build Stage Consolidation

Multi-output build stages use a staging root so their outputs can be
copied in a single `COPY --link`:

**keyd** (currently 7 COPYs → 1):

```dockerfile
FROM base AS build-keyd
RUN ... && make -C /tmp/keyd PREFIX=/usr DESTDIR=/out FORCE_SYSTEMD=1 install
# In image stage:
COPY --link --from=build-keyd /out/ /
```

**whitesur** (currently 2 COPYs → 1):

```dockerfile
FROM base AS fetch-whitesur
RUN ... && ./install.sh -d /out/usr/share/icons
# In image stage:
COPY --link --from=fetch-whitesur /out/ /
```

**wrappers** (currently 2 COPYs → 1):

```dockerfile
FROM rust:slim AS build-wrappers
RUN ... -o /out/usr/bin/code ... -o /out/usr/bin/microsoft-edge-stable
# In image stage:
COPY --link --from=build-wrappers /out/ /
```

## Invalidation Matrix

| Manifest change              | Stages rebuilt         | Image layers invalidated                  |
| ---------------------------- | ---------------------- | ----------------------------------------- |
| Bump starship version        | fetch-starship         | **1**                                     |
| Bump Edge RPM                | dl-edge, install-edge  | **1** COPY + DB finalization              |
| Add new external RPM         | new dl + install stage | **1** new COPY + DB finalization          |
| Remove external RPM          | remove stage           | **-1** COPY + DB finalization             |
| Edit keyd config             | —                      | **1** (collect-config)                    |
| Add flatpak manifest         | —                      | **1** (collect-config)                    |
| Change wrapper source        | build-wrappers         | **1**                                     |
| Add system package           | —                      | **1** (dnf install layer)                 |
| Change optional feature file | —                      | **1** (collect-config) + consolidated RUN |

Every concern is O(1). The only shared layers are the DB finalization
RUN (rebuilds when any external RPM changes) and the consolidated RUN
(rebuilds when collect-config or any ARG changes).

## Layer Budget

| Layers  | What                                                                              |
| ------- | --------------------------------------------------------------------------------- |
| 1       | `RUN dnf install` (system packages only)                                          |
| 1       | `RUN` tmpfiles + cleanup                                                          |
| 3       | `COPY --link` per-package installs (code, edge, 1password)                        |
| 1       | `RUN` RPM DB finalization                                                         |
| 7       | `COPY --link` upstream (starship, lazygit, getnf, bibata, jbmono, keyd, whitesur) |
| 1       | `COPY --link` collect-config                                                      |
| 1       | `COPY --link` wrappers                                                            |
| 1       | `RUN` consolidated post-overlay                                                   |
| 1       | `RUN` fc-cache                                                                    |
| 1       | `RUN` rpm snapshot                                                                |
| **~18** | **Total**                                                                         |

## Implementation Notes

Implemented 2026-02-22. Key changes to `bkt/src/containerfile.rs`:

### Generator Functions Added

- `emit_install_stages()` — Emits `FROM base AS install-{name}` stages
  for each external RPM. Each stage runs `rpm -i --nodb --noscripts
--nodeps` to extract files without touching the RPM database.
  Packages with `opt_path` get `/opt` → `/usr/lib/opt` relocation
  inline.

- `emit_install_copies()` — Emits `COPY --link --from=install-{name} / /`
  for each external RPM. The `--link` flag makes each layer independent.

- `emit_rpm_db_finalization()` — Emits a single `RUN` that copies all
  RPMs from dl-\* stages and runs `rpm -i --justdb --nodeps` followed
  by `ldconfig`. This registers packages in the RPM database after
  file payloads are in place.

### Generator Functions Removed

- `emit_rpm_collection()` — No longer needed; RPMs are installed
  per-package in install-\* stages.

- `emit_opt_relocation()` — Replaced by data-driven relocation via
  `opt_path` field in `external-repos.json`.

### Manifest Changes

- `external-repos.json` — Added `opt_path` field to repos that install
  to `/opt`. Values: `microsoft-edge: "microsoft"`, `1password: "1Password"`.

- `external-repos.schema.json` — Added `opt_path` as optional string.

### Phase 3 (Build Stage Consolidation) — Deferred

The `DESTDIR=/out` consolidation for keyd, whitesur, and wrappers is
not yet implemented. Current implementation achieves the O(1) property
for external RPMs; build stage consolidation is a future optimization.

## Constraints

- `COPY --link` layers cannot see layers below them during build.
  Fine for `COPY --from` since they copy from other stages.
- `RUN` instructions cannot use `--link`. DB finalization,
  consolidated setup, fc-cache, and rpm snapshot remain linear.
- `collect-config` is `FROM scratch` — no `RUN` capability.
- `/opt` relocation must happen inside each install stage, not in
  the final image. `COPY --link` layers are computed in isolation
  and cannot see the filesystem below them — a post-COPY `RUN`
  would not find files placed by a linked COPY. Each install stage
  that touches `/opt` relocates to `/usr/lib/opt/` before output.
- `fc-cache` depends on RPM fonts + upstream fonts + fontconfig.
  It runs after all COPYs.
- `rpm -qa` snapshot must be last.

## Success Criteria

- A single-concern manifest change invalidates ≤ 2 layers
- `bkt containerfile check` passes
- `rpm -qa` output identical to current image
- CI build time equal or faster
- `bootc upgrade` after a config-only change transfers minimal data
