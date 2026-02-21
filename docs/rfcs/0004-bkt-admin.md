# RFC 0004: Tier 1 — Image-Bound State (`bkt admin`)

- **Status**: Partially Implemented
- Feature Name: `bkt_admin`
- Start Date: 2025-12-31
- RFC PR: (leave this empty until PR is opened)
- Tracking Issue: (leave this empty)

> **ℹ️ Implementation Status**
>
> | Feature               | Status         | Notes                     |
> | --------------------- | -------------- | ------------------------- |
> | `bkt admin kargs`     | ✅ Implemented | Manifest-only             |
> | `bkt admin systemd`   | ✅ Implemented | Enable/disable/mask units |
> | `bkt admin systemctl` | ✅ Implemented | Direct systemctl wrapper  |
> | `bkt admin bootc`     | ✅ Implemented | bootc operations          |
> | udev rules            | ❌ Not started |                           |
> | SELinux policies      | ❌ Not started |                           |
> | Firmware settings     | ❌ Not started |                           |

## Summary

This RFC defines **Tier 1 state management** — configuration that must be baked
into the bootc image. Changes to Tier 1 state require the full pipeline:
manifest edit → PR → CI build → `bootc upgrade` → reboot.

Tier 1 is the complement to [RFC-0007](0007-drift-detection.md) (Tier 2), which
covers runtime state that can be applied immediately and may drift.

## The Tier Model

The system has two fundamentally different state lifecycles:

| Tier                                             | State Location                        | Change Mechanism      | Drift Possible?                 |
| ------------------------------------------------ | ------------------------------------- | --------------------- | ------------------------------- |
| **Tier 1** (this RFC)                            | Baked into image                      | PR → build → reboot   | No — image is deterministic     |
| **Tier 2** ([RFC-0007](0007-drift-detection.md)) | Runtime (flatpak DB, gsettings, /etc) | Immediate or deferred | Yes — runtime state can diverge |

### What Belongs in Tier 1

- **System packages** (`dnf install`) — RPMs in the base image
- **Kernel arguments** — Parameters passed to the kernel at boot
- **Systemd unit state** (image-time) — Services enabled/disabled/masked in the image
- **Custom systemd units** — Unit files shipped in the image
- **udev rules** — Hardware configuration
- **SELinux policies** — Security contexts
- **Upstream binaries** — Binaries fetched from GitHub releases during build

### Why Tier 1 Has No Drift

Tier 1 state is **deterministic** — the image defines exactly what's present.
When you boot, you get exactly what the Containerfile produced. There's no
runtime process that can modify `/usr` or change what packages are installed.

This is why drift detection ([RFC-0007](0007-drift-detection.md)) focuses on
Tier 2. Tier 1 "drift" is handled by the image build itself — if the manifest
says "install htop" and the image doesn't have htop, the build fails.

## Motivation

Some system configuration cannot be applied at runtime — it must be part of the
image:

1. **Systemd units**: Services that start at boot
2. **Kernel arguments**: Parameters passed to the kernel
3. **udev rules**: Hardware configuration
4. **SELinux policies**: Security contexts
5. **Firmware settings**: UEFI variables (documentation only)

These configurations are critical for a functional system but are easy to forget
when rebuilding.

### Current Pain Points

```bash
# Add a kernel argument
sudo rpm-ostree kargs --append=quiet

# Now I need to add this to the Containerfile... how?
# And what about the systemd unit I enabled last week?
```

### The Solution

```bash
# Kernel arguments
bkt admin kargs append quiet splash

# Systemd units
bkt admin systemd enable docker.socket

# Both update the manifest and regenerate the Containerfile
```

## Design

### Command Surface

**Host operations (immediate):**

These commands affect the running system directly via D-Bus or pkexec:

- `bkt admin bootc status`
- `bkt admin bootc upgrade --confirm|--yes`
- `bkt admin bootc switch <image> --confirm|--yes`
- `bkt admin bootc rollback --confirm|--yes`

- `bkt admin systemctl status <unit>`
- `bkt admin systemctl start|stop|restart <unit> --confirm`
- `bkt admin systemctl enable|disable <unit> --confirm`
- `bkt admin systemctl daemon-reload --confirm`

**Image-time configuration (manifest-backed):**

These commands update `manifests/system-config.json` only — they do not affect
the running system:

- `bkt admin kargs append <arg...>`
- `bkt admin kargs remove <arg...>`
- `bkt admin kargs list`

- `bkt admin systemd enable <unit...>`
- `bkt admin systemd disable <unit...>`
- `bkt admin systemd mask <unit...>`
- `bkt admin systemd list`

### The `systemctl` vs `systemd` Distinction

Note the two different command groups:

| Command                          | Scope          | Effect                             |
| -------------------------------- | -------------- | ---------------------------------- |
| `bkt admin systemctl enable foo` | **Runtime**    | Enables unit now via D-Bus         |
| `bkt admin systemd enable foo`   | **Image-time** | Records in manifest for next build |

This mirrors the Tier 1/Tier 2 split:

- `systemctl` = immediate host operation (like Tier 2)
- `systemd` = image-time configuration (Tier 1)

For services that need both, run both commands — or use `bkt admin systemctl`
with `--persist` (future) to do both at once.

## Guide-level Explanation

### Kernel Arguments

```bash
# Append argument
bkt admin kargs append quiet

# Append multiple
bkt admin kargs append quiet splash

# Remove argument
bkt admin kargs remove rhgb

# List current manifest entries
bkt admin kargs list
```

Generated in Containerfile:

```dockerfile
# === KERNEL ARGUMENTS (managed by bkt) ===
RUN rpm-ostree kargs --append=quiet --append=splash
# === END KERNEL ARGUMENTS ===
```

#### Current Gaps

- No `/usr/lib/bootc/kargs.d` integration (bootc-native approach)
- No immediate application or `/proc/cmdline` visibility
- `list` only shows manifest entries, not active kernel command line

### Systemd Units

```bash
# Enable a service (image-time)
bkt admin systemd enable docker.socket

# Enable multiple
bkt admin systemd enable docker.socket podman.socket

# Disable a service
bkt admin systemd disable cups.service

# Mask a service (prevent starting)
bkt admin systemd mask NetworkManager-wait-online.service

# List managed units
bkt admin systemd list
```

Generated in Containerfile:

```dockerfile
# === SYSTEMD UNITS (managed by bkt) ===
RUN systemctl enable docker.socket podman.socket
RUN systemctl disable cups.service
RUN systemctl mask NetworkManager-wait-online.service
# === END SYSTEMD UNITS ===
```

### Custom Systemd Units

For units that don't exist in packages:

```bash
# Add a custom unit file
bkt admin systemd add my-backup.timer
# Opens $EDITOR with template, saves to system/my-backup.timer
```

```dockerfile
# === CUSTOM UNITS (managed by bkt) ===
COPY system/my-backup.timer /etc/systemd/system/
COPY system/my-backup.service /etc/systemd/system/
RUN systemctl enable my-backup.timer
# === END CUSTOM UNITS ===
```

### udev Rules

```bash
# Add a udev rule
bkt admin udev add 99-my-device.rules
# Opens $EDITOR, saves to system/udev/99-my-device.rules
```

### SELinux

```bash
# Set a boolean
bkt admin selinux bool set httpd_can_network_connect on

# List managed booleans
bkt admin selinux bool list
```

### Firmware/UEFI

```bash
# Document UEFI settings (for reference, not automation)
bkt admin firmware note "Secure Boot: Enabled"
bkt admin firmware note "TPM: Enabled"
```

These are stored in documentation, not applied automatically.

## Reference-level Explanation

### Manifest Structure

Image-time settings are stored in `manifests/system-config.json`:

```json
{
  "$schema": "../schemas/system-config.schema.json",
  "kargs": {
    "append": ["quiet", "splash", "zswap.enabled=1"],
    "remove": ["rhgb"]
  },
  "systemd": {
    "enable": ["docker.socket", "podman.socket"],
    "disable": ["cups.service"],
    "mask": ["NetworkManager-wait-online.service"],
    "custom": ["my-backup.timer", "my-backup.service"]
  },
  "udev": {
    "rules": ["99-my-device.rules"]
  },
  "selinux": {
    "booleans": {
      "httpd_can_network_connect": true
    }
  },
  "firmware_notes": ["Secure Boot: Enabled", "TPM: Enabled"]
}
```

### Containerfile Generation

Admin settings are generated in the appropriate sections:

```dockerfile
FROM ghcr.io/ublue-os/bazzite-gnome:stable

# === KERNEL ARGUMENTS (managed by bkt) ===
RUN rpm-ostree kargs --append=quiet --append=splash
# === END KERNEL ARGUMENTS ===

# ... packages ...

# === SYSTEMD UNITS (managed by bkt) ===
RUN systemctl enable docker.socket podman.socket
RUN systemctl disable cups.service
RUN systemctl mask NetworkManager-wait-online.service
# === END SYSTEMD UNITS ===

# === CUSTOM UNITS (managed by bkt) ===
COPY system/my-backup.timer /etc/systemd/system/
COPY system/my-backup.service /etc/systemd/system/
RUN systemctl enable my-backup.timer
# === END CUSTOM UNITS ===

# === UDEV RULES (managed by bkt) ===
COPY system/udev/* /etc/udev/rules.d/
# === END UDEV RULES ===

# === SELINUX (managed by bkt) ===
RUN setsebool -P httpd_can_network_connect on
# === END SELINUX ===
```

### Behavior

- `bootc` actions run via `pkexec bootc` and require explicit confirmation for
  mutating operations. Read-only status is passwordless for wheel users.
- `systemctl` actions use D-Bus (zbus) instead of shelling out to `systemctl`.
  Mutating operations require confirmation and prompt in interactive sessions.
- `kargs` and `systemd` mutate `manifests/system-config.json` only; they do not
  apply changes to the running system.
- Containerfile generation consumes `system-config.json` to emit the appropriate
  RUN commands.

## Implementation Notes

- Admin commands are designed to run on the host; when invoked from a toolbox,
  D-Bus routing still reaches the host system.
- Mutating operations require `--confirm` (or `--yes` for bootc) to prevent
  accidental host changes.
- `bkt admin systemd` and `bkt admin kargs` only write manifests today; they
  do not create PRs or trigger builds.

## Relationship to Other RFCs

| RFC                                                         | Relationship                                           |
| ----------------------------------------------------------- | ------------------------------------------------------ |
| [RFC-0007](0007-drift-detection.md)                         | Tier 2 complement — runtime state and drift detection  |
| [RFC-0035](0035-admin-update.md)                            | Uses `bkt admin` as part of the update workflow        |
| [RFC-0037](0037-bkt-upgrade.md)                             | `bkt upgrade` wraps `bkt admin bootc upgrade`          |
| [RFC-0044](0044-bkt-try-transient-overlay.md)               | `bkt try` provides transient preview of Tier 1 changes |
| [RFC-0048](0048-subsystem-and-containerfile-unification.md) | Defines `SubsystemTier::Atomic` for Tier 1 subsystems  |

## Drawbacks

### Limited Local Effect

Kernel args and systemd changes require a new image build and reboot.
Mitigation: clear messaging about what takes effect when.

### SELinux Complexity

SELinux is complex. Mitigation: support only simple boolean cases initially.

## Rationale and Alternatives

### Why Not Just Edit Containerfile?

Programmatic tracking enables:

- Consistent manifest format across all configuration
- Containerfile regeneration from manifests
- Future drift detection for Tier 1 (comparing manifest to staged image)

### Alternative: Ansible

More powerful but overkill for personal distribution.

## Prior Art

- **rpm-ostree kargs**: Direct kernel argument management
- **systemd presets**: Distribution-level service configuration
- **bootc kargs.d**: Drop-in TOML files for kernel arguments

## Future Possibilities

- **Boot Loader Configuration**: GRUB themes, timeout
- **Secure Boot**: Key enrollment automation
- **Hardware Profiles**: Different configs for different machines
- **`--persist` flag**: Apply change immediately AND record in manifest
