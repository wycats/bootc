# RFC 0009: Runtime Privileged Operations

- Feature Name: `privileged_operations`
- Start Date: 2026-01-03
- RFC PR: (leave this empty until PR is opened)
- Tracking Issue: (leave this empty)
- Depends on: [RFC-0010](0010-transparent-delegation.md) (Transparent Command Delegation)

## Summary

Implement passwordless privileged operations for `bkt` commands using polkit authorization, enabling seamless management of bootc images and systemd services from both host and toolbox contexts.

> **Note**: This RFC assumes that `bkt admin` commands automatically delegate to the host via RFC-0010's transparent delegation infrastructure. The delegation layer handles toolbox→host routing; this RFC focuses on privilege elevation via polkit/pkexec.

## Motivation

### Current Pain Points

**Problem 1: Interactive Password Prompts Break Automation**

```bash
# From toolbox, trying to check system status
bootc status
# ❌ Requires sudo
# ❌ Password prompt breaks scripts
# ❌ Annoying for frequent operations
```

**Problem 2: Toolbox Can't Manage Host System Smoothly**

```bash
# From toolbox:
flatpak-spawn --host bootc upgrade
# ❌ Still needs privilege escalation
# ❌ User must know the flatpak-spawn incantation
# ❌ No confirmation for dangerous operations
```

**Problem 3: All Operations Treated Equally**

```bash
bootc status          # Read-only, should be easy
bootc upgrade         # Mutating, should require confirmation
# Currently both require the same sudo dance
```

### The Solution

```bash
# Read-only operations: passwordless for wheel group
bkt admin bootc status
bkt admin systemctl status docker.service

# Mutating operations: require explicit confirmation
bkt admin bootc upgrade --confirm
bkt admin systemctl restart docker.service --confirm

# From toolbox or host: same experience
# bkt handles the context detection automatically
```

## Guide-level Explanation

### Bootc Management

```bash
# Check system status (passwordless, read-only)
bkt admin bootc status

# Upgrade to latest image (requires --confirm)
bkt admin bootc upgrade --confirm

# Switch to different image
bkt admin bootc switch ghcr.io/user/custom:latest --confirm

# Rollback to previous deployment
bkt admin bootc rollback --confirm
```

### Systemd Management

```bash
# Query status (passwordless, read-only)
bkt admin systemctl status docker.service

# Start/stop/restart (requires --confirm)
bkt admin systemctl restart docker.service --confirm
bkt admin systemctl stop cups.service --confirm

# Enable/disable units at boot (requires --confirm)
bkt admin systemctl enable docker.socket --confirm
bkt admin systemctl disable cups.service --confirm
```

### Security Model

**Principle**: Separate read operations (passwordless) from mutations (confirmation required).

| Operation Type      | Privilege Level | Requires --confirm | Example                            |
| ------------------- | --------------- | ------------------ | ---------------------------------- |
| **Read-only**       | Polkit (wheel)  | No                 | `bootc status`, `systemctl status` |
| **Service control** | Polkit (wheel)  | Yes                | `systemctl restart`                |
| **Image updates**   | Polkit (wheel)  | Yes                | `bootc upgrade`                    |
| **Rollback**        | Polkit (wheel)  | Yes                | `bootc rollback`                   |

**The `--confirm` Flag**:

- Required for all mutating operations
- CLI prompts: "This will restart docker.service. Continue? [y/N]"
- Scriptable: `--yes` flag bypasses prompt (use with caution)

**Why Passwordless for Wheel Group?**

- Wheel group already has sudo access — this adds no new privilege
- Polkit provides audit logging
- Explicit `--confirm` prevents accidental mutations
- Matches the "you're maintaining your own distribution" philosophy

### Toolbox Integration

All commands work identically from toolbox:

```bash
# Automatic detection + delegation
bkt admin bootc status
# Internally: flatpak-spawn --host pkexec bootc status
```

No special flags needed — `bkt` detects toolbox context automatically.

### Dry-Run Support

Following the Plan-Execute pattern from RFC-0008:

```bash
# Preview what would happen
bkt admin bootc upgrade --dry-run
# → Would execute: pkexec bootc upgrade
# → Context: toolbox (will use flatpak-spawn --host)
```

## Reference-level Explanation

### Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                        bkt admin                            │
├─────────────────────────┬───────────────────────────────────┤
│    bootc subcommand     │       systemctl subcommand        │
│    (pkexec-based)       │       (D-Bus-based)               │
├─────────────────────────┼───────────────────────────────────┤
│                         │                                   │
│  flatpak-spawn --host   │   zbus → org.freedesktop.systemd1 │
│         ↓               │              ↓                    │
│      pkexec             │        Polkit auth                │
│         ↓               │              ↓                    │
│      bootc              │      systemd Manager              │
│                         │                                   │
└─────────────────────────┴───────────────────────────────────┘
                          ↑
                    Polkit Rules
              (50-bkt-admin.rules)
```

### Component 1: Polkit Rules

**File**: `system/polkit-1/rules.d/50-bkt-admin.rules`

```javascript
// Grant passwordless access to wheel group for bkt admin operations
polkit.addRule(function (action, subject) {
  // pkexec-based commands (bootc, rpm-ostree)
  if (
    action.id == "org.freedesktop.policykit.exec" &&
    subject.isInGroup("wheel")
  ) {
    var program = action.lookup("program");
    if (program == "/usr/bin/bootc" || program == "/usr/bin/rpm-ostree") {
      return polkit.Result.YES;
    }
  }

  // systemd D-Bus operations are handled by systemd's own polkit rules
  // We don't need custom rules for those — wheel group already has access

  return polkit.Result.NOT_HANDLED;
});
```

### Component 2: Context Detection

```rust
/// Detect if we're running inside a toolbox container
pub fn in_toolbox() -> bool {
    // Method 1: Toolbox environment variable
    if std::env::var("TOOLBOX_PATH").is_ok() {
        return true;
    }

    // Method 2: Container type marker
    if let Ok(container_type) = std::env::var("container") {
        if container_type == "toolbox" {
            return true;
        }
    }

    // Method 3: Filesystem marker
    std::path::Path::new("/run/.toolboxenv").exists()
}
```

### Component 3: pkexec Wrapper (bootc, rpm-ostree)

**File**: `bkt/src/commands/admin/bootc.rs`

```rust
use std::process::Command;
use anyhow::{Result, bail};

pub fn exec_bootc(subcommand: &str, args: &[String]) -> Result<()> {
    let status = if in_toolbox() {
        // Delegate to host via flatpak-spawn
        Command::new("flatpak-spawn")
            .arg("--host")
            .arg("pkexec")
            .arg("bootc")
            .arg(subcommand)
            .args(args)
            .status()?
    } else {
        // Direct execution on host
        Command::new("pkexec")
            .arg("bootc")
            .arg(subcommand)
            .args(args)
            .status()?
    };

    if !status.success() {
        bail!("bootc {} failed with exit code {:?}",
              subcommand, status.code());
    }
    Ok(())
}
```

### Component 4: D-Bus Integration (systemd)

**File**: `bkt/src/dbus/systemd.rs`

```rust
use zbus::{blocking::Connection, dbus_proxy, Result as ZbusResult};

#[dbus_proxy(
    interface = "org.freedesktop.systemd1.Manager",
    default_service = "org.freedesktop.systemd1",
    default_path = "/org/freedesktop/systemd1"
)]
trait SystemdManager {
    fn start_unit(&self, name: &str, mode: &str)
        -> ZbusResult<zbus::zvariant::OwnedObjectPath>;

    fn stop_unit(&self, name: &str, mode: &str)
        -> ZbusResult<zbus::zvariant::OwnedObjectPath>;

    fn restart_unit(&self, name: &str, mode: &str)
        -> ZbusResult<zbus::zvariant::OwnedObjectPath>;

    fn reload_unit(&self, name: &str, mode: &str)
        -> ZbusResult<zbus::zvariant::OwnedObjectPath>;

    fn enable_unit_files(&self, files: &[&str], runtime: bool, force: bool)
        -> ZbusResult<(bool, Vec<(String, String, String)>)>;

    fn disable_unit_files(&self, files: &[&str], runtime: bool)
        -> ZbusResult<Vec<(String, String, String)>>;

    fn get_unit(&self, name: &str)
        -> ZbusResult<zbus::zvariant::OwnedObjectPath>;
}

pub struct SystemdClient {
    proxy: SystemdManagerProxyBlocking<'static>,
}

impl SystemdClient {
    pub fn new() -> Result<Self> {
        let connection = Connection::system()?;
        let proxy = SystemdManagerProxyBlocking::new(&connection)?;
        Ok(Self { proxy })
    }

    pub fn restart_unit(&self, unit: &str) -> Result<()> {
        self.proxy.restart_unit(unit, "replace")?;
        Ok(())
    }

    // ... other methods
}
```

**Note on Toolbox Context**: D-Bus from toolbox automatically routes to the host's system bus, so no special handling is needed for `zbus` calls.

### Component 5: Command Structure

Following the Plan-Execute pattern from RFC-0008:

```rust
// bkt/src/commands/admin/mod.rs
use clap::{Args, Subcommand};

#[derive(Debug, Args)]
pub struct AdminArgs {
    #[command(subcommand)]
    pub action: AdminAction,
}

#[derive(Debug, Subcommand)]
pub enum AdminAction {
    /// Manage bootc images
    Bootc {
        #[command(subcommand)]
        action: BootcAction,
    },
    /// Manage systemd services
    Systemctl {
        #[command(subcommand)]
        action: SystemctlAction,
    },
}

#[derive(Debug, Subcommand)]
pub enum BootcAction {
    /// Show current deployment status
    Status,
    /// Upgrade to latest image
    Upgrade {
        /// Confirm the upgrade operation
        #[arg(long)]
        confirm: bool,
    },
    /// Switch to a different image
    Switch {
        /// Image reference (e.g., ghcr.io/user/image:tag)
        image: String,
        /// Confirm the switch operation
        #[arg(long)]
        confirm: bool,
    },
    /// Rollback to previous deployment
    Rollback {
        /// Confirm the rollback operation
        #[arg(long)]
        confirm: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum SystemctlAction {
    /// Show unit status
    Status { unit: String },
    /// Start a unit
    Start {
        unit: String,
        #[arg(long)]
        confirm: bool,
    },
    /// Stop a unit
    Stop {
        unit: String,
        #[arg(long)]
        confirm: bool,
    },
    /// Restart a unit
    Restart {
        unit: String,
        #[arg(long)]
        confirm: bool,
    },
    /// Enable a unit at boot
    Enable {
        unit: String,
        #[arg(long)]
        confirm: bool,
    },
    /// Disable a unit at boot
    Disable {
        unit: String,
        #[arg(long)]
        confirm: bool,
    },
}
```

### Cargo Dependencies

Add to `bkt/Cargo.toml`:

```toml
[dependencies]
zbus = { version = "4", default-features = false, features = ["blocking"] }
```

### Containerfile Integration

```dockerfile
# Install polkit rules for passwordless bkt admin
COPY system/polkit-1/rules.d/50-bkt-admin.rules /etc/polkit-1/rules.d/
```

### Error Handling

**Polkit denial (non-wheel user)**:

```
Error: Permission denied by system policy

You must be in the 'wheel' group to perform privileged operations.
To add yourself to the wheel group:
  sudo usermod -aG wheel $USER
Then log out and back in.
```

**Missing --confirm flag**:

```
Error: This operation requires explicit confirmation

'bootc upgrade' will update your system image. This operation:
  • Stages a new deployment for next boot
  • Does not affect the running system
  • Can be rolled back with 'bkt admin bootc rollback'

To proceed, run:
  bkt admin bootc upgrade --confirm
```

**Toolbox without host access**:

```
Error: Cannot access host system from this container

This container lacks flatpak-spawn integration.
Ensure you're using a toolbox created with 'toolbox create'.
```

## Drawbacks

1. **Polkit dependency**: Requires polkit rules to be installed in the image
2. **Wheel group requirement**: Users must be in wheel group (standard for Fedora/RHEL)
3. **Security trade-off**: Passwordless for wheel users — mitigated by `--confirm` for mutations

## Alternatives

### Alternative 1: setuid Binary (Rejected)

Create a setuid `bkt-admin` binary that handles privileged operations.

**Pros**:

- No polkit dependency
- Works on systems without polkit

**Cons**:

- Massive security surface area
- Every input must be carefully sanitized
- SELinux complications
- Ongoing maintenance burden
- setuid Rust binaries are unusual and poorly supported

**Verdict**: The security risk is not worth the benefit.

### Alternative 2: Always Require sudo (Rejected)

Keep the current behavior where users must use sudo.

**Pros**:

- Simple, no new code
- Familiar to users

**Cons**:

- Breaks automation and scripting
- Poor UX for frequent operations
- No distinction between read and write operations
- Toolbox experience is clunky

**Verdict**: Doesn't solve the problem we set out to solve.

### Alternative 3: Custom D-Bus Service (Rejected)

Create a dedicated `org.bootc.Admin1` D-Bus service with its own polkit policies.

**Pros**:

- Maximum control over authorization
- Clean API design
- Could expose additional metadata

**Cons**:

- Requires a daemon to be running
- More complex packaging (systemd unit, D-Bus service file, polkit policy)
- Overkill for wrapping a few commands

**Verdict**: Too much complexity for the use case.

## Unresolved Questions

1. **Should `bootc status` require polkit at all?**

   - Current: Yes, but passwordless for wheel
   - Alternative: Could use `bootc status --json` which may not need privileges
   - Decision: Start with polkit, revisit if `bootc` adds unprivileged status

2. **What systemd operations should be exposed?**

   - Current: start, stop, restart, enable, disable, status
   - Future: reload, mask, unmask, daemon-reload
   - Decision: Start minimal, expand based on use

3. **Should we support unit files from manifests?**
   - Integration with RFC-0004's image-time systemd management
   - Could have `bkt admin systemctl sync` that applies manifest-defined units
   - Decision: Out of scope for this RFC, revisit after RFC-0004

## Implementation Checklist

### Phase 1: Foundation (~2-3 hours)

- [ ] Create `bkt/src/commands/admin/mod.rs`
- [ ] Create CLI structure with clap
- [ ] Implement `in_toolbox()` context detection
- [ ] Add placeholder subcommands

### Phase 2: Polkit + pkexec (~6-8 hours)

- [ ] Create `system/polkit-1/rules.d/50-bkt-admin.rules`
- [ ] Implement `exec_bootc()` helper
- [ ] Implement `bkt admin bootc status`
- [ ] Implement `bkt admin bootc upgrade --confirm`
- [ ] Implement `bkt admin bootc switch --confirm`
- [ ] Implement `bkt admin bootc rollback --confirm`
- [ ] Update Containerfile to copy polkit rules

### Phase 3: D-Bus Integration (~8-12 hours)

- [ ] Add `zbus` dependency
- [ ] Create `bkt/src/dbus/mod.rs`
- [ ] Create `bkt/src/dbus/systemd.rs`
- [ ] Implement SystemdManager proxy
- [ ] Test D-Bus connectivity from toolbox

### Phase 4: systemctl Commands (~4-6 hours)

- [ ] Implement `bkt admin systemctl status <unit>`
- [ ] Implement `bkt admin systemctl start/stop/restart <unit> --confirm`
- [ ] Implement `bkt admin systemctl enable/disable <unit> --confirm`

### Phase 5: Testing & Documentation (~4 hours)

- [ ] Unit tests for context detection
- [ ] Integration tests (manual, requires image rebuild)
- [ ] Update CURRENT.md with implementation summary
- [ ] Add help text and examples

## Success Criteria

- [ ] `bkt admin bootc status` works passwordless from both host and toolbox
- [ ] `bkt admin bootc upgrade --confirm` works without password for wheel users
- [ ] `bkt admin systemctl restart docker.service --confirm` works via D-Bus
- [ ] Non-wheel users receive clear, actionable error messages
- [ ] All commands follow the Plan-Execute pattern
- [ ] `--dry-run` shows what would be executed
