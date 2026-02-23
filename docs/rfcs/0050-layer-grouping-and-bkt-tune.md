# RFC-0050: Layer Grouping and `bkt tune`

| Status | Draft |
|--------|-------|
| Created | 2026-02-23 |
| Depends on | RFC-0045 (Layer Minimization) |

## Problem Statement

RFC-0045 introduced per-package install stages with `COPY --link` for O(1) layer invalidation. However, this creates many deployment layers, which can hit **btrfs's hardlink limit (1024 per file)** in the ostree repository.

The current approach optimizes for build caching but doesn't account for deployment constraints. We need a way to:

1. Keep per-package build stages (for cache efficiency)
2. Control deployment layer count (for ostree/btrfs compatibility)
3. Make intelligent grouping decisions based on package characteristics

## Solution

### 1. Layer Group Manifest Field

Add a `layer_group` field to external repo definitions:

```json
{
  "name": "code",
  "display_name": "Visual Studio Code",
  "packages": ["code", "code-insiders"],
  "layer_group": "independent"
}
```

**Values:**
- `"independent"` - Package gets its own deployment layer (high-churn, large packages)
- `"bundled"` - Package is grouped with other bundled packages (low-churn, smaller packages)

**Default:** `"bundled"` (conservative - minimizes layer count)

### 2. Containerfile Generation Changes

The generator maintains per-package *build* stages but consolidates *deployment* layers:

```dockerfile
# Build stages (unchanged - one per package for cache efficiency)
FROM base AS install-code
RUN rpm -i --nodb --noscripts --nodeps /tmp/rpms/code/*.rpm

FROM base AS install-microsoft-edge
RUN rpm -i --nodb --noscripts --nodeps /tmp/rpms/microsoft-edge/*.rpm

FROM base AS install-1password
RUN rpm -i --nodb --noscripts --nodeps /tmp/rpms/1password/*.rpm

# Deployment layers (grouped by layer_group)
# Independent packages get their own COPY --link
COPY --link --from=install-code / /
COPY --link --from=install-microsoft-edge / /

# Bundled packages share a merged stage
FROM base AS install-bundled
COPY --from=install-1password / /
# Future bundled packages would be added here

COPY --link --from=install-bundled / /
```

**Key insight:** Build caching works at the stage level. Deployment layering works at the `COPY --link` level. These can be decoupled.

### 3. `bkt tune` Command Namespace

New command namespace for proactive optimization:

```
bkt tune layers    # Analyze and suggest layer groupings
bkt tune prune     # Clean up ostree deployments/objects
```

#### `bkt tune layers`

Analyzes external packages and suggests `layer_group` assignments:

```
$ bkt tune layers

Analyzing external packages...

Package          Size      Update Freq    Current Group    Suggested
─────────────────────────────────────────────────────────────────────
code             180 MB    weekly         (none)           independent
microsoft-edge   210 MB    weekly         (none)           independent
1password        45 MB     monthly        (none)           bundled

Recommendation: 2 independent layers, 1 bundled layer (3 total)

To apply: bkt tune layers --apply
```

**Analysis heuristics:**
- **Size threshold:** Packages > 100 MB benefit from independent layers (worth the layer cost)
- **Update frequency:** Weekly+ updates benefit from independent layers (frequent cache hits)
- **Combined score:** Large AND frequent → independent; otherwise → bundled

**Data sources:**
- Size: Query from RPM repo metadata or cached download
- Frequency: Heuristic based on package type, or historical data if available

#### `bkt tune prune`

Wrapper around ostree maintenance:

```
$ bkt tune prune

Current deployments:
  0: 904e0bb... (staged)
  1: 3490817... (booted)
  2: e5c8d85... (rollback)

Unreferenced objects: 1,247 (estimated 2.3 GB)

Actions available:
  --keep-rollback    Keep rollback deployment (default)
  --remove-rollback  Remove rollback to free hardlinks
  --prune-objects    Remove unreferenced objects

$ bkt tune prune --remove-rollback --prune-objects
Removed deployment e5c8d85...
Pruned 1,247 objects (2.3 GB freed)
```

### 4. Schema Changes

Update `schemas/external-repos.schema.json`:

```json
{
  "layer_group": {
    "type": "string",
    "enum": ["independent", "bundled"],
    "default": "bundled",
    "description": "Controls deployment layer grouping. 'independent' packages get their own layer; 'bundled' packages share a layer."
  }
}
```

## Implementation Plan

### Phase 1: Manifest and Generator (fixes immediate issue)
1. Add `layer_group` field to schema and manifest
2. Update `emit_install_copies()` to group by `layer_group`
3. Add merged stage for bundled packages
4. Regenerate Containerfile

### Phase 2: `bkt tune layers`
1. Create `commands/tune.rs` with `TuneArgs` enum
2. Implement `layers` subcommand with analysis logic
3. Add `--apply` flag to update manifest

### Phase 3: `bkt tune prune`
1. Implement wrapper around `ostree admin` commands
2. Add safety checks and dry-run support

## Alternatives Considered

### Squash all layers at build time
- **Pro:** Eliminates layer count concerns entirely
- **Con:** Loses all caching benefits; every change rebuilds everything
- **Verdict:** Too aggressive; we want selective optimization

### Use `--squash` flag in podman build
- **Pro:** Simple to implement
- **Con:** All-or-nothing; can't preserve some layer boundaries
- **Verdict:** Doesn't give us the control we need

### Migrate ostree repo to ext4
- **Pro:** 65,000 hardlink limit instead of 1,024
- **Con:** Requires filesystem migration; not always possible
- **Verdict:** Not a software solution; out of scope

## Success Criteria

1. Containerfile generates with configurable layer grouping
2. Default configuration stays under 10 deployment layers for external RPMs
3. `bkt tune layers` provides actionable recommendations
4. `bkt tune prune` safely manages ostree maintenance
5. No more "Too many links" errors on btrfs systems

## References

- [RFC-0045: Layer Minimization](0045-layer-minimization.md)
- [ostree hardlink issue](https://github.com/ostreedev/ostree/issues/3489)
- [btrfs hardlink limit](https://btrfs.readthedocs.io/en/latest/btrfs-man5.html) (1024 per file)
