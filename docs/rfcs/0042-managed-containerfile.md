# RFC 0042: Fully Managed Containerfile Generation

## Status

Implemented (2026-02-22)

## Summary

The Containerfile is fully generated from manifests by `bkt containerfile
generate`. Every piece of information in the Containerfile traces back
to a manifest file. Hand-editing the Containerfile is prohibited; all
changes flow through manifests.

> **Note**: This RFC originally included a proposal for a `bkt-build`
> helper binary. That proposal has been extracted to RFC-0049.

## The Axiom

> Every piece of information in the Containerfile must trace back to a
> manifest file.

If it's not in a manifest, it shouldn't be in the Containerfile. This
is what makes full generation possible and what prevents the hand-edit
bugs that plagued earlier iterations.

## Problem (Historical)

The multi-stage Containerfile (RFC 0041) worked, but had problems:

1. **Partially managed.** bkt owned 5 sections via `=== MARKERS ===`,
   but the other ~300 lines were hand-maintained. Hand-edits caused
   bugs (e.g., the `DNF_CACHE_EPOCH` silent-ignore bug).

2. **Latent RPM bug.** The SYSTEM_PACKAGES section listed external
   packages by name, so `dnf install` re-downloaded them from repos.
   The pre-downloaded RPMs in `/tmp/rpms/` sat unused.

## Solution: Full Generation

`bkt containerfile generate` produces the entire Containerfile from:

| Manifest                        | Generates                                    |
| ------------------------------- | -------------------------------------------- |
| `manifests/external-repos.json` | base stage repos + dl-\* stages + install-\* stages |
| `upstream/manifest.json`        | fetch-\*/build-\* stages + COPY --from lines |
| `manifests/system-packages.json`| SYSTEM_PACKAGES install section              |
| `manifests/host-shims.json`     | HOST_SHIMS section                           |
| System config manifests         | KERNEL_ARGUMENTS, SYSTEMD_UNITS, etc.        |

### `bkt containerfile generate`

Produces a complete Containerfile. The generator reads all manifests
and emits:

1. `base` stage (repos from external-repos.json)
2. `dl-*` stages (download RPMs for each external repo)
3. `install-*` stages (extract RPMs with `rpm -i --nodb --noscripts --nodeps`)
4. `fetch-*` stages (upstream binaries from manifest.json)
5. `build-*` stages (compiled upstreams like keyd, wrappers)
6. `image` stage assembly with `COPY --link --from=` for layer independence

### `bkt containerfile check`

Diffs the generated Containerfile against the committed one. CI fails
if they diverge. This enforces the axiom: if you want to change the
Containerfile, change a manifest.

### `bkt containerfile sync`

Regenerates the Containerfile and writes it to disk. Used after
manifest changes.

## Layer Independence

The generator implements RFC-0045's layer independence design:

- **Per-package install stages**: Each external RPM gets its own
  `install-*` stage that extracts files with `rpm -i --nodb --noscripts
  --nodeps`.

- **`COPY --link` assembly**: Each package's files are copied into the
  final image with `COPY --link --from=install-*`. The `--link` flag
  makes layers independent — changing one package doesn't invalidate
  others.

- **RPM DB finalization**: A single `RUN rpm -i --justdb` registers
  all packages in the RPM database after file payloads are in place.

- **Data-driven `/opt` relocation**: Packages that install to `/opt`
  (Edge, 1Password) have an `opt_path` field in the manifest. The
  generator handles relocation to `/usr/lib/opt/` automatically.

See RFC-0045 for the full design rationale.

## Implementation

The generator lives in `bkt/src/containerfile.rs`. Key functions:

| Function                    | Purpose                                      |
| --------------------------- | -------------------------------------------- |
| `emit_base_stage()`         | Base image + repo setup                      |
| `emit_dl_stages()`          | `FROM base AS dl-{repo}` for RPM downloads   |
| `emit_install_stages()`     | `FROM base AS install-{pkg}` for RPM extraction |
| `emit_upstream_stages()`    | `FROM base AS fetch-{name}` for binaries     |
| `emit_install_copies()`     | `COPY --link --from=install-*` assembly      |
| `emit_rpm_db_finalization()`| `RUN rpm -i --justdb` + ldconfig             |
| `emit_image_assembly()`     | Orchestrates the final image stage           |

## Relationship to Other RFCs

- **RFC 0041 (Multi-Stage Parallel Downloads)**: This RFC builds on
  0041's multi-stage architecture.

- **RFC 0045 (Layer Independence)**: The generator implements 0045's
  per-package install pattern and `COPY --link` assembly.

- **RFC 0049 (bkt-build Helper)**: A proposed build-time helper that
  would further simplify the generated stages. Not yet implemented.

- **RFC 0006 (Upstream Management)**: The generator consumes version
  pins from `upstream/manifest.json` managed by `bkt upstream`.
