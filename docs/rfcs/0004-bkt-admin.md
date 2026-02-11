# RFC 0004: bkt admin

Privileged host operations and image-time system configuration.

## Motivation

Some actions require host privileges (bootc, systemd control), while other
actions need to be recorded for image builds (kernel args and persistent
systemd configuration). `bkt admin` provides a clear split between immediate
host operations and manifest-backed image settings.

## Design

### Command Surface

Host operations (immediate):

- `bkt admin bootc status`
- `bkt admin bootc upgrade --confirm|--yes`
- `bkt admin bootc switch <image> --confirm|--yes`
- `bkt admin bootc rollback --confirm|--yes`

- `bkt admin systemctl status <unit>`
- `bkt admin systemctl start|stop|restart <unit> --confirm`
- `bkt admin systemctl enable|disable <unit> --confirm`
- `bkt admin systemctl daemon-reload --confirm`

Image-time configuration (manifest backed):

- `bkt admin kargs append <arg...>`
- `bkt admin kargs remove <arg...>`
- `bkt admin kargs list`

- `bkt admin systemd enable <unit...>`
- `bkt admin systemd disable <unit...>`
- `bkt admin systemd mask <unit...>`
- `bkt admin systemd list`

### Manifest Format

Image-time settings are stored in `manifests/system-config.json`:

```json
{
  "kargs": {
    "append": ["quiet"],
    "remove": ["rhgb"]
  },
  "systemd": {
    "enable": ["docker.socket"],
    "disable": ["cups.service"],
    "mask": []
  },
  "udev": { "rules": [] },
  "selinux": { "booleans": {} },
  "firmware_notes": []
}
```

### Behavior

- `bootc` actions run via `pkexec bootc` and require explicit confirmation for
  mutating operations. Read-only status is passwordless for wheel users.
- `systemctl` actions use D-Bus (zbus) instead of shelling out to `systemctl`.
  Mutating operations require confirmation and prompt in interactive sessions.
- `kargs` and `systemd` mutate `manifests/system-config.json` only; they do not
  apply changes to the running system.
- Containerfile generation consumes `system-config.json` to emit:
  - `rpm-ostree kargs` in the KERNEL_ARGUMENTS section.
  - `systemctl enable/disable/mask` in the SYSTEMD_UNITS section.

## Implementation Notes

- Admin commands are designed to run on the host; when invoked from a toolbox,
  D-Bus routing still reaches the host system.
- Mutating operations require `--confirm` (or `--yes` for bootc) to prevent
  accidental host changes.
- `bkt admin systemd` and `bkt admin kargs` only write manifests today; they
  do not create PRs or trigger builds.

## Known Gaps

- No `bkt admin` support for udev rules, SELinux policies, or firmware notes.
- No tooling for adding custom systemd unit files to the manifest.# RFC 0004: System Administration (`bkt admin`)

- **Status**: Partially Implemented
- Feature Name: `bkt_admin`
- Start Date: 2025-12-31
- RFC PR: (leave this empty until PR is opened)
- Tracking Issue: (leave this empty)

> **ℹ️ Implementation Status**
>
> | Feature               | Status         | Notes                                       |
> | --------------------- | -------------- | ------------------------------------------- |
> | `bkt admin kargs`     | ✅ Implemented | Manifest-only; see RFC-0036 for enhancement |
> | `bkt admin systemd`   | ✅ Implemented | Enable/disable/mask units                   |
> | `bkt admin systemctl` | ✅ Implemented | Direct systemctl wrapper                    |
> | `bkt admin bootc`     | ✅ Implemented | bootc operations                            |
> | udev rules            | ❌ Not started |                                             |
> | SELinux policies      | ❌ Not started |                                             |
> | Firmware settings     | ❌ Not started |                                             |
>
> **Note:** The kernel arguments section describes `rpm-ostree kargs` approach.
> [RFC-0036](0036-system-kargs.md) proposes enhancing `bkt admin kargs` to use
> bootc-native `kargs.d/` TOML files instead.

## Summary

Implement `bkt admin` commands for managing system-level configuration that must be baked into the bootc image, including systemd units, kernel arguments, SELinux policies, and firmware settings.

## Motivation

Some system configuration cannot be applied at runtime - it must be part of the image:

1. **Systemd units**: Services that start at boot
2. **Kernel arguments**: Parameters passed to the kernel
3. **udev rules**: Hardware configuration
4. **SELinux policies**: Security contexts
5. **Firmware settings**: UEFI variables

These configurations are critical for a functional system but are easy to forget when rebuilding.

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

# Both update the Containerfile and create a PR
```

## Guide-level Explanation

### Kernel Arguments

```bash
# Append argument
bkt admin kargs append quiet

# Append multiple
bkt admin kargs append quiet splash

# Remove argument
bkt admin kargs remove rhgb

# List current
bkt admin kargs list
```

Generated in Containerfile:

```dockerfile
# === KERNEL ARGUMENTS (managed by bkt) ===
RUN rpm-ostree kargs --append=quiet --append=splash
# === END KERNEL ARGUMENTS ===
```

### Systemd Units

```bash
# Enable a service
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

```json
// manifests/admin.json
{
  "kargs": {
    "append": ["quiet", "splash"],
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

### Local Application

Some admin changes can be applied locally without a reboot:

```bash
bkt admin systemd enable docker.socket
# Runs: systemctl enable --now docker.socket
# Updates manifest and opens PR
```

Others require a reboot or new image:

```bash
bkt admin kargs append quiet
# Updates manifest and opens PR
# Note: Kernel args take effect on next boot
```

## Drawbacks

### Limited Local Effect

Kernel args and some systemd changes require reboot. Mitigation: clear messaging.

### SELinux Complexity

SELinux is complex. Mitigation: support only simple boolean cases.

## Rationale and Alternatives

### Why Not Just Edit Containerfile?

Programmatic tracking enables drift detection and easier updates.

### Alternative: Ansible

More powerful but overkill for personal distribution.

## Prior Art

- **rpm-ostree kargs**: Direct kernel argument management
- **systemd presets**: Distribution-level service configuration

## Unresolved Questions

_None currently - this RFC focuses on well-understood patterns._

## Future Possibilities

- **Boot Loader Configuration**: GRUB themes, timeout
- **Secure Boot**: Key enrollment automation
- **Hardware Profiles**: Different configs for different machines
