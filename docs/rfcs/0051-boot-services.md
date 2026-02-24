# RFC 0051: Boot Services and System Initialization

- **Status**: Draft
- **Created**: 2026-02-23
- **Related**: [RFC-0019](0019-cron-able-sync.md) (Sync), [RFC-0035](0035-admin-update.md) (Admin Update)

> **⚠️ Absorbed by [RFC-0053](0053-bootstrap-and-repo-discovery.md).**
> Bootstrap and apply-boot are consolidated into RFC-0053, which adds repo
> discovery and the repo clone step to the bootstrap sequence.

## Summary

Define how bkt integrates with systemd for boot-time and first-boot operations. This RFC covers `bkt admin apply-boot` (post-reboot package capture) and `bkt bootstrap` (first-boot setup), replacing the shell scripts `bootc-apply` and `bootc-bootstrap`.

## Motivation

### The Problem

Two shell scripts handle boot-time operations:

1. **`bootc-apply`** — Runs at boot to capture any rpm-ostree layered packages into the manifest
2. **`bootc-bootstrap`** — Runs on first boot to apply flatpak remotes, apps, gsettings, and other user-scoped state

These scripts work, but they're bash when they should be Rust:

- They duplicate logic that exists in bkt commands
- They can't leverage bkt's manifest parsing, validation, or error handling
- They're harder to test than Rust code
- They create a false distinction between "bkt commands" and "system scripts"

### The Principle

**bkt binaries are system binaries.** They're built into the image during `podman build`, installed to `/usr/bin/`, and available at boot like any other system tool. There's no reason boot-time operations can't be bkt commands.

## Guide-level Explanation

### `bkt admin apply-boot`

Runs at boot (via systemd) to capture any layered packages that were installed since the last image deployment.

```bash
# Systemd unit calls this
bkt admin apply-boot
```

**What it does:**

1. Check if the current deployment differs from the last-applied deployment
2. If different, run `bkt capture` to update manifests
3. Record the current deployment checksum to avoid redundant work on subsequent boots

**Marker file:** `/var/lib/bkt/last-applied-deployment`

### `bkt bootstrap`

Runs on first boot (via systemd) to apply user-scoped state that can't be baked into the image.

```bash
# Systemd unit calls this
bkt bootstrap [--force]
```

**What it does:**

1. Check if bootstrap has already run (marker file)
2. Apply flatpak remotes and apps
3. Apply gsettings
4. Apply GNOME extensions
5. Record completion to avoid redundant work

**Marker file:** `$XDG_STATE_HOME/bootc-bootstrap/last-applied.sha256`

### Systemd Integration

Both commands are invoked by systemd units that ship with the image:

```ini
# /etc/systemd/system/bkt-apply-boot.service
[Unit]
Description=Capture layered packages after boot
After=rpm-ostreed.service

[Service]
Type=oneshot
ExecStart=/usr/bin/bkt admin apply-boot
RemainAfterExit=yes

[Install]
WantedBy=multi-user.target
```

```ini
# /etc/systemd/user/bkt-bootstrap.service
[Unit]
Description=First-boot user setup
After=graphical-session.target

[Service]
Type=oneshot
ExecStart=/usr/bin/bkt bootstrap
RemainAfterExit=yes

[Install]
WantedBy=default.target
```

## Reference-level Explanation

### `bkt admin apply-boot`

**Implementation:**

```rust
pub fn run(args: ApplyBootArgs) -> Result<()> {
    let marker_path = Path::new("/var/lib/bkt/last-applied-deployment");

    // Get current deployment checksum
    let current = get_current_deployment()?;

    // Check marker
    if let Ok(last) = fs::read_to_string(&marker_path) {
        if last.trim() == current {
            tracing::info!("Deployment unchanged, skipping capture");
            return Ok(());
        }
    }

    // Run capture
    tracing::info!("New deployment detected, capturing state");
    commands::capture::run(CaptureArgs::default())?;

    // Update marker
    fs::create_dir_all(marker_path.parent().unwrap())?;
    fs::write(&marker_path, &current)?;

    Ok(())
}
```

**CommandTarget:** `Host` — always runs on host, no delegation.

### `bkt bootstrap`

**Implementation:**

```rust
pub fn run(args: BootstrapArgs) -> Result<()> {
    let state_dir = dirs::state_dir()
        .unwrap_or_else(|| PathBuf::from("~/.local/state"))
        .join("bootc-bootstrap");
    let marker_path = state_dir.join("last-applied.sha256");

    // Compute manifest hash
    let manifest_hash = compute_manifest_hash()?;

    // Check marker (unless --force)
    if !args.force {
        if let Ok(last) = fs::read_to_string(&marker_path) {
            if last.trim() == manifest_hash {
                tracing::info!("Bootstrap already applied, skipping");
                return Ok(());
            }
        }
    }

    // Apply subsystems
    apply_flatpak_remotes()?;
    apply_flatpak_apps()?;
    apply_gsettings()?;
    apply_gnome_extensions()?;

    // Update marker
    fs::create_dir_all(&state_dir)?;
    fs::write(&marker_path, &manifest_hash)?;

    Ok(())
}
```

**CommandTarget:** `Host` — always runs on host, no delegation.

### Build Integration

The bkt binary is built during `podman build` and installed to `/usr/bin/`:

```dockerfile
FROM base AS build-bkt
COPY bkt/ /build/bkt/
COPY bkt-common/ /build/bkt-common/
RUN cargo build --release --manifest-path /build/bkt/Cargo.toml
RUN install -m 0755 /build/bkt/target/release/bkt /usr/bin/bkt
```

This makes bkt available at boot like any other system binary.

## Migration

### Scripts to Remove

Once implemented, these scripts are superseded:

| Script                    | Replacement            |
| ------------------------- | ---------------------- |
| `scripts/bootc-apply`     | `bkt admin apply-boot` |
| `scripts/bootc-bootstrap` | `bkt bootstrap`        |

### Systemd Units to Update

The existing systemd units should be updated to call bkt instead of the scripts:

```diff
-ExecStart=/usr/share/bootc-bootstrap/bootc-apply
+ExecStart=/usr/bin/bkt admin apply-boot
```

## Drawbacks

- Requires bkt to be built and installed during image build
- Adds complexity to the Containerfile

## Rationale and Alternatives

**Why not keep the scripts?**

The scripts duplicate logic that exists in bkt. Maintaining two implementations (bash and Rust) for the same operations is error-prone and wasteful.

**Why not use a separate binary?**

A separate `bkt-boot` binary would fragment the codebase. The bkt binary is already designed to handle multiple command contexts; boot-time commands are just another context.

## Unresolved Questions

1. Should `bkt bootstrap` be a top-level command or `bkt admin bootstrap`?
2. Should the systemd units be generated by bkt or hand-maintained?

## Future Possibilities

- `bkt admin apply-boot --commit` to automatically commit and push manifest changes
- Integration with RFC-0019 (sync) for periodic re-application
