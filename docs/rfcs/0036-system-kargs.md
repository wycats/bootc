# RFC 0036: Kernel Arguments Enhancement

- Feature Name: `kargs_enhancement`
- Start Date: 2026-02-02
- RFC PR: #90
- Tracking Issue: (leave this empty)

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
