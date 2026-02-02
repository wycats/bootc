# RFC 0034: Ephemeral System Modifications via usroverlay

- Feature Name: `usroverlay_integration`
- Start Date: 2026-02-02
- RFC PR: (leave this empty until PR is opened)
- Tracking Issue: (leave this empty)

## Summary

Integrate `rpm-ostree usroverlay` into the bkt workflow to enable safe, idempotent, ephemeral modifications to `/usr` for development and hotfix scenarios, while maintaining alignment with the capture-first, manifest-driven design philosophy.

## Motivation

### The Problem

On immutable (bootc/ostree) systems, `/usr` is read-only. This creates friction when:

1. **Development iteration**: Testing new versions of `bkt` itself before image rebuild
2. **Hotfixes**: Applying urgent fixes that can't wait for a full image deploy
3. **Debugging**: Temporarily replacing binaries to diagnose issues
4. **Bootstrap chicken-and-egg**: System services depend on `/usr/bin/bkt`, but the image contains an older version

### Current State

Today, users must either:

- Wait for a full image rebuild and reboot (slow, disruptive)
- Use `rpm-ostree override replace` (persistent, requires RPM packaging)
- Manually discover and use `rpm-ostree usroverlay` (undocumented in our workflow)

### The Opportunity

`rpm-ostree usroverlay` creates a **transient writable overlay** on `/usr`:

- Changes are **ephemeral** (discarded on reboot)
- No RPM packaging required
- Immediate effect (no reboot needed)
- Perfect for development and testing

This aligns with our ephemeral manifest concept (RFC 0021) but for system binaries.

## Guide-level Explanation

### Core Concepts

#### Ephemeral vs Persistent Modifications

| Modification Type        | Mechanism            | Survives Reboot | Use Case                       |
| ------------------------ | -------------------- | --------------- | ------------------------------ |
| **Ephemeral**            | `usroverlay`         | No              | Development, testing, hotfixes |
| **Persistent (layered)** | `rpm-ostree install` | Yes             | Permanent additions            |
| **Persistent (image)**   | Containerfile        | Yes             | Base system                    |

#### The Idempotency Contract

**Critical**: Any ephemeral modification via usroverlay must be:

1. **Reproducible**: Can be re-applied after reboot via manifest/script
2. **Declarative**: Described in a manifest, not ad-hoc
3. **Convergent**: Running the command twice produces the same result
4. **Recoverable**: System boots cleanly without the overlay

This means we **never** use usroverlay for one-off manual changes. Instead:

- Changes are captured in a manifest or script
- The overlay is applied programmatically
- The manifest is the source of truth for what _should_ be overlaid

### New Commands

#### `bkt admin usroverlay`

```bash
# Enable the writable overlay on /usr
bkt admin usroverlay enable
# Development mode enabled. A writable overlayfs is now mounted on /usr.
# All changes will be discarded on reboot.

# Check overlay status
bkt admin usroverlay status
# Overlay: active
# Changes will be discarded on reboot.
# Modified files:
#   /usr/bin/bkt (replaced)
#   /usr/bin/bootc-apply (replaced)

# Apply development binaries from local build
bkt admin usroverlay apply-dev
# Copying ~/.cargo/bin/bkt -> /usr/bin/bkt
# Copying scripts/bootc-apply -> /usr/bin/bootc-apply
# Copying scripts/bootc-bootstrap -> /usr/bin/bootc-bootstrap
# ✓ Development binaries applied (ephemeral until reboot)

# Disable overlay (requires reboot)
bkt admin usroverlay disable
# Note: Overlay changes persist until reboot.
# Run 'systemctl reboot' to restore original /usr.
```

#### `bkt dev install-local`

For the common case of testing local bkt changes:

```bash
# Build and install bkt to usroverlay (one command)
bkt dev install-local
# Building bkt in release mode...
# Enabling usroverlay...
# Installing to /usr/bin/bkt...
# ✓ Local bkt installed (ephemeral until reboot)

# With scripts
bkt dev install-local --with-scripts
# Also copies bootc-apply, bootc-bootstrap to /usr/bin/
```

### Workflow Integration

#### Development Workflow

```bash
# 1. Make changes to bkt source
vim bkt/src/commands/new_feature.rs

# 2. Build and test locally
cargo build --release

# 3. Install to system for integration testing
bkt dev install-local

# 4. Test the changes
bkt new-feature --test

# 5. If satisfied, commit and push
git add . && git commit -m "feat: add new feature"

# 6. Reboot restores original system
# (or rebuild image with changes)
```

#### Hotfix Workflow

```bash
# 1. Identify the fix needed
# (e.g., bootc-apply calls non-existent command)

# 2. Apply hotfix via overlay
bkt admin usroverlay enable
sudo cp scripts/bootc-apply /usr/bin/bootc-apply

# 3. Document the hotfix
echo "Applied bootc-apply hotfix for bkt system capture" >> HOTFIXES.md

# 4. Create tracking issue/PR for permanent fix
# (the overlay is temporary; real fix goes in image)
```

### Service Compatibility

When usroverlay is active, systemd services see the modified `/usr`. However, services that run **at boot** (before user login) won't see overlay changes because:

1. `usroverlay` is session-scoped (enabled by user)
2. Boot services run before any user session

**Solution**: For boot-time services, use the service override pattern:

```bash
# Disable service until next image (if overlay can't help)
bkt admin systemctl override bootc-apply.service \
  --condition "ConditionPathExists=!/var/lib/bkt/disable-bootc-apply"

# Create the disable marker
sudo touch /var/lib/bkt/disable-bootc-apply

# Remove marker after deploying fixed image
sudo rm /var/lib/bkt/disable-bootc-apply
```

## Reference-level Explanation

### Implementation

#### `bkt admin usroverlay` Subcommand

```rust
#[derive(Debug, Subcommand)]
pub enum UsroverlayAction {
    /// Enable writable overlay on /usr
    Enable,

    /// Show overlay status and modified files
    Status,

    /// Apply development binaries from local build
    ApplyDev {
        /// Also copy bootstrap scripts
        #[arg(long)]
        with_scripts: bool,
    },

    /// Show instructions to disable (requires reboot)
    Disable,
}

impl UsroverlayAction {
    pub fn run(&self) -> Result<()> {
        match self {
            Self::Enable => {
                // Check if already enabled
                if is_usroverlay_active()? {
                    println!("Overlay already active.");
                    return Ok(());
                }

                // Enable via polkit (requires wheel group)
                run_privileged(&["rpm-ostree", "usroverlay"])?;

                println!("Development mode enabled.");
                println!("All changes to /usr will be discarded on reboot.");
                Ok(())
            }

            Self::Status => {
                if is_usroverlay_active()? {
                    println!("Overlay: active");
                    println!("Changes will be discarded on reboot.");
                    // List modified files via overlay inspection
                    list_overlay_changes()?;
                } else {
                    println!("Overlay: inactive");
                    println!("Run 'bkt admin usroverlay enable' to enable.");
                }
                Ok(())
            }

            Self::ApplyDev { with_scripts } => {
                ensure_usroverlay_active()?;

                // Copy bkt binary
                let cargo_bin = dirs::home_dir()
                    .unwrap()
                    .join(".cargo/bin/bkt");

                if cargo_bin.exists() {
                    copy_privileged(&cargo_bin, "/usr/bin/bkt")?;
                    println!("Copied {} -> /usr/bin/bkt", cargo_bin.display());
                }

                if *with_scripts {
                    let repo_root = find_repo_root()?;
                    for script in ["bootc-apply", "bootc-bootstrap"] {
                        let src = repo_root.join("scripts").join(script);
                        let dst = format!("/usr/bin/{}", script);
                        if src.exists() {
                            copy_privileged(&src, &dst)?;
                            println!("Copied {} -> {}", src.display(), dst);
                        }
                    }
                }

                println!("✓ Development binaries applied (ephemeral until reboot)");
                Ok(())
            }

            Self::Disable => {
                println!("Note: Overlay changes persist until reboot.");
                println!("Run 'systemctl reboot' to restore original /usr.");
                Ok(())
            }
        }
    }
}

fn is_usroverlay_active() -> Result<bool> {
    // Check if /usr is mounted as overlay
    let mounts = std::fs::read_to_string("/proc/mounts")?;
    Ok(mounts.lines().any(|line| {
        line.contains("overlay") && line.contains(" /usr ")
    }))
}
```

#### `bkt dev install-local` Subcommand

```rust
#[derive(Debug, Args)]
pub struct InstallLocalArgs {
    /// Also install bootstrap scripts
    #[arg(long)]
    with_scripts: bool,

    /// Skip cargo build (use existing binary)
    #[arg(long)]
    no_build: bool,
}

impl InstallLocalArgs {
    pub fn run(&self) -> Result<()> {
        let repo_root = find_repo_root()?;

        // Build if needed
        if !self.no_build {
            println!("Building bkt in release mode...");
            let status = Command::new("cargo")
                .args(["build", "--release", "--manifest-path"])
                .arg(repo_root.join("bkt/Cargo.toml"))
                .status()?;

            if !status.success() {
                anyhow::bail!("Build failed");
            }
        }

        // Enable overlay
        if !is_usroverlay_active()? {
            println!("Enabling usroverlay...");
            run_privileged(&["rpm-ostree", "usroverlay"])?;
        }

        // Install binary
        let built_binary = repo_root.join("bkt/target/release/bkt");
        copy_privileged(&built_binary, "/usr/bin/bkt")?;
        println!("Installed /usr/bin/bkt");

        if self.with_scripts {
            for script in ["bootc-apply", "bootc-bootstrap"] {
                let src = repo_root.join("scripts").join(script);
                copy_privileged(&src, &format!("/usr/bin/{}", script))?;
                println!("Installed /usr/bin/{}", script);
            }
        }

        println!("✓ Local bkt installed (ephemeral until reboot)");
        Ok(())
    }
}
```

### Idempotency Guarantees

The implementation ensures idempotency:

1. **`usroverlay enable`**: No-op if already active
2. **`apply-dev`**: Overwrites existing files (convergent)
3. **`install-local`**: Builds fresh, overwrites (convergent)
4. **Reboot**: Always restores to known-good state (recovery)

### Integration with Existing Commands

| Existing Command          | Usroverlay Interaction                         |
| ------------------------- | ---------------------------------------------- |
| `bkt apply`               | Works normally (applies to running system)     |
| `bkt capture`             | Works normally (captures from running system)  |
| `bkt admin bootc upgrade` | Warns if overlay active (changes will be lost) |
| `bkt drift check`         | Should detect overlay modifications            |

### Drift Detection Integration

Extend drift detection (RFC 0007) to detect usroverlay state:

```bash
bkt drift check
# +-------------------------------------------------------------+
# | Drift Report                                                |
# +-------------------------------------------------------------+
# | System State:                                               |
# |   ⚠ usroverlay active (changes ephemeral)                   |
# |     /usr/bin/bkt (modified)                                 |
# |     /usr/bin/bootc-apply (modified)                         |
# +-------------------------------------------------------------+
```

## Drawbacks

1. **Complexity**: Another mechanism to understand
2. **Footgun potential**: Users might forget changes are ephemeral
3. **Boot-time limitation**: Can't help services that run before user login
4. **Polkit dependency**: Requires wheel group membership

## Rationale and Alternatives

### Why usroverlay over alternatives?

| Alternative                   | Drawback                                      |
| ----------------------------- | --------------------------------------------- |
| `rpm-ostree override replace` | Requires RPM, persistent (survives reboot)    |
| `rpm-ostree install`          | Requires RPM, persistent                      |
| Symlinks from /var            | Doesn't work for all binaries, SELinux issues |
| Container-based testing       | Can't test system service integration         |

### Why not make it automatic?

We explicitly **don't** auto-enable usroverlay because:

1. It requires explicit user intent (security)
2. Users should know their changes are ephemeral
3. Aligns with plan→confirm→execute philosophy

## Prior Art

- **NixOS**: `nix-shell` provides ephemeral environments, but for user-space only
- **Fedora Silverblue**: Documents usroverlay for development
- **systemd-sysext**: Similar concept for system extensions

## Unresolved Questions

1. Should `bkt dev install-local` auto-enable usroverlay or require explicit enable first?
2. How to handle fetchbin binaries that install to `/usr/local/bin`?
3. Should we track overlay modifications in a manifest for reproducibility?

## Future Possibilities

1. **Overlay manifest**: Track what's been overlaid for documentation
2. **Auto-apply on login**: Option to re-apply dev binaries after reboot
3. **CI integration**: Test PRs with usroverlay before merge
4. **Rollback within session**: `bkt admin usroverlay reset` to restore original without reboot

---

## Appendix: Blue/Immutable OS Concepts

For users unfamiliar with immutable OS patterns, here's a glossary of related concepts:

### Core Concepts

| Concept          | Description                                                           |
| ---------------- | --------------------------------------------------------------------- |
| **Immutable OS** | Operating system where `/usr` is read-only; changes require new image |
| **bootc**        | Container-native approach to immutable OS (OCI images as OS)          |
| **ostree**       | Content-addressed filesystem for OS images (like git for filesystems) |
| **rpm-ostree**   | Hybrid package/image system combining ostree with RPM                 |

### Modification Mechanisms

| Mechanism            | Persistence  | Use Case                                      |
| -------------------- | ------------ | --------------------------------------------- |
| **Layered packages** | Persistent   | Add packages not in base image                |
| **Overrides**        | Persistent   | Replace base packages with different versions |
| **usroverlay**       | Ephemeral    | Development, testing, hotfixes                |
| **apply-live**       | Until reboot | Apply staged changes without reboot           |

### Related Commands

```bash
# Check current deployment state
rpm-ostree status

# Stage an upgrade (applies on reboot)
rpm-ostree upgrade

# Apply staged changes live (no reboot)
rpm-ostree apply-live

# Create ephemeral writable /usr
rpm-ostree usroverlay

# Replace a package temporarily
rpm-ostree override replace ./package.rpm

# Rollback to previous deployment
rpm-ostree rollback
```

### Filesystem Layout

```
/                     # Root (read-only, from image)
├── usr/              # System binaries (read-only, from image)
│   ├── bin/          # Executables
│   ├── lib/          # Libraries
│   └── share/        # Shared data
├── etc/              # Configuration (writable, merged on upgrade)
├── var/              # Variable data (writable, persistent)
│   ├── home/         # User homes (symlinked from /home)
│   └── lib/          # Application state
└── ostree/           # OSTree repository (deployments, refs)
```

### Key Principles

1. **Atomic updates**: Upgrades are all-or-nothing; no partial states
2. **Rollback**: Previous deployment always available
3. **Reproducibility**: Same image = same system
4. **Separation**: System (`/usr`) vs configuration (`/etc`) vs data (`/var`)
