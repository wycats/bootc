# RFC 0036: Kernel Arguments Enhancement

- **Status**: Withdrawn (Absorbed)
- Feature Name: `kargs_enhancement`
- Start Date: 2026-02-02
- RFC PR: #90
- Tracking Issue: (leave this empty)

> **📦 Absorbed into RFC-0004**
>
> This RFC documented gaps in `bkt admin kargs`. The content has been absorbed
> into [RFC-0004: Tier 1 — Image-Bound State](../0004-bkt-admin.md), which is
> now the canonical home for all Tier 1 (image-time) configuration including
> kernel arguments.
>
> The gaps documented here (no `/usr/lib/bootc/kargs.d` integration, no
> `/proc/cmdline` visibility) are now listed in RFC-0004's "Current Gaps"
> section.

## Summary

`bkt admin kargs` is currently manifest-only. It updates `manifests/system-config.json` by appending/removing kernel arguments and lists the manifest entries. There is no `/usr/lib/bootc/kargs.d` integration, no immediate application, and no PR workflow flags in this command.

## Motivation

### The Problem

Kernel arguments are currently managed only in the manifest, so changes are staged for the next image build but do not affect the running system. There is also no way to view the active kernel command line through `bkt admin kargs`.

## Guide-level Explanation

### Current CLI

| Command                        | Behavior                 |
| ------------------------------ | ------------------------ |
| `bkt admin kargs append <arg>` | Append to manifest       |
| `bkt admin kargs remove <arg>` | Mark removal in manifest |
| `bkt admin kargs list`         | Show manifest entries    |

### Current Behavior

### Manifest Format

`bkt admin kargs` writes to `manifests/system-config.json`:

```json
{
  "$schema": "../schemas/system-config.schema.json",
  "kargs": {
    "append": ["zswap.enabled=1", "zswap.compressor=lz4"],
    "remove": ["quiet"]
  }
}
```

### Behavior

- `append` adds arguments to `kargs.append` and removes them from `kargs.remove`.
- `remove` adds arguments to `kargs.remove` and removes them from `kargs.append`.
- `list` only shows manifest-managed arguments.

## Gaps

- No `/usr/lib/bootc/kargs.d` integration.
- No immediate application or `/proc/cmdline` visibility.
- No PR workflow flags on `bkt admin kargs`.
