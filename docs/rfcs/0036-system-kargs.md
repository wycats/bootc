# RFC 0036: Kernel Arguments Enhancement

- Feature Name: `kargs_enhancement`
- Start Date: 2026-02-02
- RFC PR: #90
- Tracking Issue: (leave this empty)

## Summary

Enhance the existing `bkt admin kargs` command to:
1. Use bootc-native `/usr/lib/bootc/kargs.d/` TOML files instead of `rpm-ostree kargs`
2. Support immediate local application via usroverlay (RFC 0034 dependency)
3. Add PR workflow integration (`--pr`, `--local`, `--pr-only` flags)
4. Include `/proc/cmdline` in the `list` output

This RFC supersedes the manifest-only approach currently implemented and aligns kernel argument management with the Immediate Development Axiom.

## Motivation

### The Problem

The current `bkt admin kargs` implementation:
- Only updates `manifests/system-config.json` (no immediate effect)
- Generates `rpm-ostree kargs --append` in the Containerfile
- Has no PR workflow integration
- Doesn't show active kernel arguments from `/proc/cmdline`

This violates the Immediate Development Axiom: changes should take effect immediately, then be captured for persistence.

### Real-World Example

When adding zswap memory tuning (commit `0578855`), we had to:
1. Manually create `/usr/lib/bootc/kargs.d/10-zswap.toml`
2. Manually edit the Containerfile to generate the file
3. Commit and push
4. Wait for CI to build
5. `bootc upgrade` and reboot

The desired workflow:
```bash
bkt admin kargs append zswap.enabled=1 zswap.compressor=lz4 zswap.zpool=zsmalloc
# → Writes to /usr/lib/bootc/kargs.d/ via usroverlay
# → Updates Containerfile
# → Creates PR
# → Reboot to test immediately
```

## Guide-level Explanation

### Current vs Enhanced CLI

| Current | Enhanced |
|---------|----------|
| `bkt admin kargs append <arg>` | Same, plus immediate application |
| `bkt admin kargs delete <arg>` | Same, plus immediate application |
| `bkt admin kargs list` | Enhanced: shows `/proc/cmdline` too |
| (none) | `--pr`, `--local`, `--pr-only` flags |

### Enhanced Behavior

```bash
# Add kernel arguments (immediate + persistent)
$ bkt admin kargs append zswap.enabled=1 zswap.compressor=lz4
⟳ Enabling usroverlay for /usr modifications...
✓ Created /usr/lib/bootc/kargs.d/99-bkt.toml
✓ Updated Containerfile (KERNEL_ARGUMENTS section)
✓ Created PR #42: "feat: Add zswap kernel arguments"
ℹ Reboot to apply kernel arguments

# List all kernel arguments with sources
$ bkt admin kargs list --source
ARGUMENT                    SOURCE                              STATUS
zswap.enabled=1             /usr/lib/bootc/kargs.d/99-bkt.toml  pending reboot
zswap.compressor=lz4        /usr/lib/bootc/kargs.d/99-bkt.toml  pending reboot
kvm.report_ignored_msrs=0   /proc/cmdline                       active
root=UUID=...               /proc/cmdline                       active

# Local-only (for testing, no PR)
$ bkt admin kargs append --local debug
✓ Created /usr/lib/bootc/kargs.d/99-bkt.toml
ℹ Local only - not persisted to Containerfile
ℹ Reboot to apply kernel arguments
```

### Flags

| Flag | Effect |
|------|--------|
| `--pr` | Create a PR with the changes (default) |
| `--local` | Only apply locally via usroverlay, don't update Containerfile |
| `--pr-only` | Only update Containerfile, don't apply locally |
| `--dry-run` | Show what would happen |
| `--source` | (for `list`) Show where each argument comes from |

## Reference-level Explanation

### Storage Mechanism Change

**Current**: `rpm-ostree kargs --append` in Containerfile
```dockerfile
# === KERNEL_ARGUMENTS (managed by bkt) ===
RUN rpm-ostree kargs \
    --append=zswap.enabled=1 \
    --append=zswap.compressor=lz4
# === END KERNEL_ARGUMENTS ===
```

**Enhanced**: bootc-native kargs.d TOML files
```dockerfile
# === KERNEL_ARGUMENTS (managed by bkt) ===
RUN mkdir -p /usr/lib/bootc/kargs.d && \
    printf '%s\n' \
        '# Managed by bkt - do not edit manually' \
        'kargs = [' \
        '    "zswap.enabled=1",' \
        '    "zswap.compressor=lz4",' \
        '    "zswap.zpool=zsmalloc",' \
        '    "zswap.max_pool_percent=25"' \
        ']' \
        > /usr/lib/bootc/kargs.d/99-bkt.toml
# === END KERNEL_ARGUMENTS ===
```

### Why kargs.d Over rpm-ostree?

| Aspect | rpm-ostree kargs | kargs.d TOML |
|--------|------------------|--------------|
| bootc-native | ❌ rpm-ostree specific | ✅ Official bootc mechanism |
| Architecture filtering | ❌ No | ✅ `match-architectures` |
| Immediate testing | ❌ Creates deployment | ✅ Works with usroverlay |
| File-based | ❌ Command-based | ✅ Declarative files |

### Immediate Application (RFC 0034 Dependency)

For immediate testing before image rebuild:

1. Check if `/usr` is writable
2. If not, prompt to enable usroverlay (`bootc usr-overlay`)
3. Write `/usr/lib/bootc/kargs.d/99-bkt.toml`
4. Inform user that reboot is needed

Note: Kernel arguments cannot take effect without a reboot, but the kargs.d file can be written immediately for testing on next reboot.

### Manifest Integration

The existing `manifests/system-config.json` with `kernel_arguments_append` and `kernel_arguments_delete` arrays remains the source of truth. The Containerfile generator changes from emitting `rpm-ostree kargs` to emitting kargs.d file creation.

### Persistence Flow

```
bkt admin kargs append <args>
    ↓
Update manifests/system-config.json
    ↓
[If not --pr-only]
    ↓
Write /usr/lib/bootc/kargs.d/99-bkt.toml (via usroverlay)
    ↓
[If not --local]
    ↓
Update Containerfile KERNEL_ARGUMENTS section
    ↓
[If --pr or default]
    ↓
Create PR with changes
```

## Scope

### In Scope

- Kernel arguments via `/usr/lib/bootc/kargs.d/`
- Immediate application via usroverlay
- PR workflow integration

### Out of Scope (Future Work)

- **sysctl settings**: Different mechanism (`/etc/sysctl.d/`), doesn't require usroverlay, could be `bkt admin sysctl`
- **Architecture filtering**: The kargs.d format supports it, but CLI exposure is deferred
- **Named presets**: e.g., `bkt admin kargs preset zswap` for common configurations

## Rationale and Alternatives

### Why Enhance `bkt admin kargs` Instead of New Command?

1. **Command already exists**: `bkt admin kargs` is implemented with `append`, `delete`, `list`
2. **Avoid duplication**: Creating `bkt system kargs` would have two commands for the same purpose
3. **Consistent namespace**: Kernel arguments are privileged operations, fitting `bkt admin`

### Alternatives Considered

1. **New `bkt system kargs` command**: Rejected to avoid duplication
2. **Keep rpm-ostree approach**: Rejected because kargs.d is more bootc-native
3. **Manifest-only (current)**: Violates Immediate Development Axiom

## Drawbacks

1. **Breaking change**: Containerfile output format changes (rpm-ostree → kargs.d)
2. **Requires usroverlay**: For immediate application (RFC 0034 dependency)
3. **Reboot still required**: Kernel args can't change at runtime

## Prior Art

- Current `bkt admin kargs` implementation
- RFC 0004: bkt-admin (defines admin command structure)
- RFC 0034: usroverlay integration (enables immediate Tier 1 changes)
- bootc documentation on `/usr/lib/bootc/kargs.d/`

## Unresolved Questions

1. **Migration**: How to handle existing `rpm-ostree kargs` in Containerfiles?
2. **Multiple files**: Should we support multiple kargs.d files (e.g., `10-zswap.toml`, `20-debug.toml`)?
3. **Validation**: Should we validate known kernel parameters?

## Implementation Plan

1. Update `bkt/src/commands/admin.rs` kargs subcommand
2. Change Containerfile generator from rpm-ostree to kargs.d output
3. Add usroverlay detection and `/usr/lib/bootc/kargs.d/` writing
4. Add `--pr`, `--local`, `--pr-only` flags
5. Enhance `list` to include `/proc/cmdline`
6. Update tests and documentation

## Related RFCs

- **RFC 0004** (bkt-admin): Parent RFC for admin commands - should be updated to reflect kargs.d approach
- **RFC 0034** (usroverlay): Dependency for immediate application
- **RFC 0035** (admin update): Could integrate kargs changes into update flow
