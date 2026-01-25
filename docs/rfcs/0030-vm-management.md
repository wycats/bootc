````markdown
# RFC-0030: VM Management

- **Status**: Draft
- **Created**: 2026-01-24
- **Depends on**: RFC-0028

## Summary

Add a declarative VM management subsystem that mirrors the existing manifest-driven workflow. A new `manifests/vms.json` defines VM configuration, and `bkt vm` commands sync, capture, and control libvirt VMs using the `virsh` CLI.

## Motivation

A Windows 11 VM was recently set up manually via `virsh`. This should be managed declaratively like other subsystems to support reproducibility, auditability, and consistent automation across machines.

## Goals

- Define a `vms.json` manifest for VM configuration.
- Provide `bkt vm` subcommands for sync, capture, status, and control.
- Integrate as a standard subsystem (`VmSubsystem`).
- Use `virsh` for all operations.
- Keep disk images as external artifacts, referenced by path.

## Non-Goals

- Live migration.
- Multi-host or clustered management.
- GPU passthrough.
- Snapshot management (defer to `virsh`).

## Guide-level Explanation

VMs are described in `manifests/vms.json` under a `vms` map keyed by VM name. The VM name is used as the libvirt domain name.

Example manifest:

```json
{
  "vms": {
    "win11": {
      "memory_gb": 8,
      "vcpus": 4,
      "disk_path": "/var/lib/libvirt/images/win11.qcow2",
      "disk_size_gb": 80,
      "boot": "uefi",
      "tpm": "2.0",
      "network": "default",
      "graphics": "spice",
      "iso_sources": [
        { "path": "~/Downloads/Win11_25H2_English_x64.iso" },
        {
          "url": "https://fedorapeople.org/groups/virt/virtio-win/direct-downloads/stable-virtio/virtio-win.iso"
        }
      ]
    }
  }
}
```

### Commands

- `bkt vm sync` — create/update VMs to match the manifest
- `bkt vm capture` — snapshot VM configuration into the manifest
- `bkt vm status` — show running/stopped/missing VMs
- `bkt vm start <name>` / `bkt vm stop <name>` — control VM state

## Reference-level Explanation

### Manifest Schema (JSON Schema)

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "VmManifest",
  "description": "The vms.json manifest.",
  "type": "object",
  "properties": {
    "$schema": {
      "type": ["string", "null"]
    },
    "vms": {
      "type": "object",
      "additionalProperties": {
        "$ref": "#/$defs/VmDefinition"
      }
    }
  },
  "required": ["vms"],
  "$defs": {
    "VmDefinition": {
      "type": "object",
      "description": "A single VM definition.",
      "properties": {
        "memory_gb": {
          "type": "integer",
          "minimum": 1
        },
        "vcpus": {
          "type": "integer",
          "minimum": 1
        },
        "disk_path": {
          "type": "string"
        },
        "disk_size_gb": {
          "type": "integer",
          "minimum": 1
        },
        "create_if_missing": {
          "type": "boolean",
          "default": false,
          "description": "Provision disk_path if missing."
        },
        "boot": {
          "type": "string",
          "enum": ["uefi", "bios"]
        },
        "tpm": {
          "type": "string",
          "enum": ["2.0", "none"],
          "default": "none"
        },
        "network": {
          "type": "string"
        },
        "graphics": {
          "type": "string",
          "enum": ["spice", "vnc", "none"]
        },
        "iso_sources": {
          "type": "array",
          "items": {
            "$ref": "#/$defs/IsoSource"
          }
        }
      },
      "required": [
        "memory_gb",
        "vcpus",
        "disk_path",
        "disk_size_gb",
        "boot",
        "network",
        "graphics"
      ]
    },
    "IsoSource": {
      "type": "object",
      "description": "An installer ISO source.",
      "oneOf": [
        {
          "properties": {
            "path": { "type": "string" }
          },
          "required": ["path"],
          "additionalProperties": false
        },
        {
          "properties": {
            "url": { "type": "string", "format": "uri" }
          },
          "required": ["url"],
          "additionalProperties": false
        }
      ]
    }
  }
}
```

### Subsystem Behavior

- Implement a `VmSubsystem` that participates in the standard capture/sync workflow.
- Sync uses `virsh` for all operations: `define`, `dumpxml`, `start`, and `destroy`.
- Disk images are referenced by absolute `disk_path`; images are not stored in git.
- Optional provisioning: `create_if_missing: true` creates the disk if absent.

### Capture Strategy

`bkt vm capture` uses `virsh dumpxml` for each known domain and converts to the manifest format, preserving fields that can round-trip cleanly.

### Status Strategy

`bkt vm status` reports:

- **running**: domain exists and is active
- **stopped**: domain exists but is inactive
- **missing**: in manifest but not defined in libvirt

## Implementation Plan

### Phase 1: Manifest Types and Schema

- Define `VmManifest` and `VmDefinition` structures.
- Add `vms.schema.json`.
- Wire manifest loading into the registry.

### Phase 2: VmSubsystem and Sync

- Implement `VmSubsystem` with `sync()` and `capture()` entry points.
- `sync` renders a domain XML template and uses `virsh define`.
- Ensure idempotent updates by comparing current `dumpxml` with desired config.

### Phase 3: Commands

- Add `bkt vm sync`, `bkt vm capture`, and `bkt vm status`.
- Add `bkt vm start <name>` and `bkt vm stop <name>` wrappers.

### Phase 4: Disk Provisioning

- Implement optional disk creation when `create_if_missing` is set.
- Validate `disk_path` and `disk_size_gb` before provisioning.

## Security Considerations

- VM definitions execute only via `virsh` and the current user’s libvirt permissions.
- ISO URLs are not fetched automatically unless explicitly allowed by a future phase.
- Disk paths are trusted input; tooling should validate path safety before provisioning.

## Prior Art

- Vagrant (Vagrantfile)
- Terraform libvirt provider
- quickemu
- GNOME Boxes

## Alternatives Considered

- Using libvirt bindings instead of `virsh`: heavier dependency and less transparent.
- Storing disk images in git: impractical due to size and update frequency.

## Open Questions

- Should `iso_sources` be optional or required for initial provisioning only?
- Do we want a default network if `network` is omitted?
- Should we support metadata like `autostart` or `cpu_model` later?
````
