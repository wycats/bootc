# RFC 0002: Package Management (`bkt dnf`)

- Feature Name: `bkt_dnf`
- Start Date: 2025-12-31
- RFC PR: (leave this empty until PR is opened)
- Tracking Issue: (leave this empty)

## Summary

Implement `bkt dnf` commands for managing RPM packages across both the immutable host system and mutable toolbox containers, with automatic manifest updates and PR propagation.

## Motivation

RPM package management in an immutable OS context is uniquely challenging:

1. **Host packages** require `rpm-ostree` with mandatory reboots
2. **Toolbox packages** use standard `dnf` but aren't persistent
3. **Build-time packages** go in the Containerfile but require manual editing

Users shouldn't need to remember which tool to use or manually maintain package lists. `bkt dnf` abstracts this complexity while maintaining full control.

### Current Pain Points

```bash
# Want htop on the host?
sudo rpm-ostree install htop
# Now edit Containerfile to make it permanent...

# Want gcc in toolbox?
sudo dnf install gcc
# Now edit toolbox/Containerfile to make it permanent...
# But wait, also need to add it to the package list for CI...
```

### The Solution

```bash
# Host system (uses rpm-ostree, updates Containerfile, opens PR)
bkt dnf install htop

# Toolbox (uses dnf, updates toolbox manifest)
bkt dev dnf install gcc
```

## Guide-level Explanation

### Basic Commands

#### Installing Packages

```bash
# Install on host system (layers via rpm-ostree)
bkt dnf install htop neovim

# Install in toolbox (uses dnf directly)
bkt dev dnf install gcc cmake ninja-build
```

Both commands:
1. Install the package immediately in the appropriate context
2. Update the relevant manifest/Containerfile
3. Open a PR to propagate the change

#### Removing Packages

```bash
bkt dnf remove htop
bkt dev dnf remove gcc
```

#### Listing Packages

```bash
# List packages managed by bkt on host
bkt dnf list

# List packages managed by bkt in toolbox
bkt dev dnf list

# Compare installed vs manifested
bkt dnf diff
```

### Understanding `--now` Behavior

For host packages, `rpm-ostree install` can apply immediately with `--now`:

```bash
# Default: install requires reboot
bkt dnf install htop
# Package will be available after reboot

# Opt-in: install and apply immediately
bkt dnf install htop --now
# Package available now (live deployment)
```

The `--now` flag:
- Applies changes to the running system without reboot
- Useful for quick iteration
- Can be slower for multiple packages
- May have edge cases with complex dependencies

**Default behavior**: No `--now`, require reboot for consistency.

### Package Groups and Patterns

```bash
# Install a group
bkt dnf group install "Development Tools"

# Install with glob pattern
bkt dnf install 'python3-*-devel'

# Install from specific repo
bkt dnf install htop --repo updates-testing
```

### COPR Repository Management

**COPR (Cool Other Package Repositories)** provides community packages that aren't in the standard Fedora repos. `bkt` tracks COPR repos explicitly to ensure reproducible builds.

**The "Works on My Machine" Problem**: Without explicit COPR tracking, a user might install a package from an enabled COPR on their machine, commit the package to the manifest, and the CI build fails because CI doesn't have that COPR enabled.

**Solution**: `bkt` requires COPRs to be tracked before packages from them can be installed.

#### Enabling a COPR

```bash
bkt dnf copr enable atim/starship
# 1. Runs: sudo dnf copr enable atim/starship
# 2. Adds to manifests/copr-repos.json
# 3. Opens PR
```

#### COPR Manifest Structure

```json
// manifests/copr-repos.json
[
  {
    "name": "atim/starship",
    "enabled": true,
    "gpg_check": true
  }
]
```

#### Safety Check

When installing a package, `bkt` verifies the package source:

```bash
bkt dnf install starship
# Error: Package 'starship' is only available from COPR 'atim/starship'
# which is not tracked in manifests/copr-repos.json.
#
# Next steps:
#   - First: bkt dnf copr enable atim/starship
#   - Then: bkt dnf install starship
```

This ensures every COPR dependency is tracked and reproducible.

#### Containerfile Integration

COPRs are added to the Containerfile:

```dockerfile
# === COPR REPOSITORIES (managed by bkt) ===
RUN dnf -y copr enable atim/starship
# === END COPR REPOSITORIES ===

# === SYSTEM PACKAGES (managed by bkt) ===
RUN dnf install -y \
    htop \
    neovim \
    starship
# === END SYSTEM PACKAGES ===
```

### Build Optimization: Caching Strategy

For optimal Docker layer caching, `bkt` generates a **single `RUN dnf install` command** with all packages:

```dockerfile
# GOOD: Single layer, cache-efficient
RUN dnf install -y \
    gcc \
    htop \
    neovim \
    starship
```

Not:

```dockerfile
# BAD: Multiple layers, poor cache utilization
RUN dnf install -y gcc
RUN dnf install -y htop
RUN dnf install -y neovim
```

**Rationale**: A single `RUN` instruction means changing one package invalidates one layer. Multiple `RUN` instructions provide no benefit (since Dockerfile layers are ordered) and waste space.

#### Advanced: BuildKit Cache Mount

For development builds that iterate frequently, consider BuildKit's cache mount:

```dockerfile
RUN --mount=type=cache,target=/var/cache/dnf \
    dnf install -y \
        gcc \
        htop \
        neovim
```

This caches the DNF package cache across builds, speeding up rebuilds. The generated Containerfile optionally includes this when `bkt` is configured with `cache_strategy: buildkit`.

## Reference-level Explanation

### Manifest Location and Format

Host packages are tracked in the Containerfile with section markers:

```dockerfile
# === SYSTEM PACKAGES (managed by bkt) ===
RUN dnf install -y \
    htop \
    neovim \
    starship
# === END SYSTEM PACKAGES ===
```

Toolbox packages are tracked similarly in `toolbox/Containerfile`.

The source of truth for package management is `manifests/system-packages.json`:

```json
{
  "packages": [
    "htop",
    "neovim",
    "starship"
  ],
  "groups": [
    "@development-tools"
  ],
  "excluded": [
    "nano"
  ]
}
```

### Command Mapping

| User Command | Host Context | Toolbox Context |
|--------------|--------------|-----------------|
| `bkt dnf install X` | `rpm-ostree install X` | N/A |
| `bkt dev dnf install X` | N/A | `dnf install X` |
| `bkt dnf remove X` | `rpm-ostree uninstall X` | N/A |
| `bkt dev dnf remove X` | N/A | `dnf remove X` |

### Dependency Resolution

`bkt` does not track transitive dependencies explicitly. The manifest contains only directly-requested packages.

```bash
bkt dnf install python3-requests
# Manifest: ["python3-requests"]
# Installed: python3-requests + all dependencies
```

### Package Rename Handling

When a package is renamed upstream, `bkt` treats it as remove + add:

```bash
# If 'oldpkg' is renamed to 'newpkg':
bkt dnf remove oldpkg
bkt dnf install newpkg
```

### Error Handling

```bash
bkt dnf install nonexistent-package
# Error: Package 'nonexistent-package' not found in enabled repositories.
#
# Did you mean one of these?
#   - nonexistent-pkg
#   - existent-package
#
# Next steps:
#   - Search: dnf search nonexistent
#   - Check COPR: bkt dnf copr search nonexistent
```

### Toolbox Integration

For `bkt dev dnf`, the command:
1. Enters the default toolbox (or creates it)
2. Runs `dnf install` inside
3. Updates `toolbox/packages.json` (or `toolbox/Containerfile`)
4. Does NOT open a host PR (toolbox changes are separate)

## Drawbacks

### Two Mental Models

Users must understand when `rpm-ostree` vs `dnf` is used. Mitigation: clear error messages.

### COPR Complexity

Requiring explicit COPR tracking adds friction. Mitigation: helpful error messages guide users.

### Reboot Requirement

Host package changes may require reboot. Mitigation: `--now` flag for immediate apply.

## Rationale and Alternatives

### Why Not Just Use `dnf`?

On immutable systems, `dnf` doesn't work directly. `rpm-ostree` is required.

### Why Not Auto-detect Context?

Explicit is better than implicit. `bkt dev` makes the target clear.

## Prior Art

- **rpm-ostree**: The underlying mechanism
- **toolbox/distrobox**: Container-based development environments
- **Nix**: Declarative package management

## Unresolved Questions

### Q1: `--now` Flag Default

**Resolution**: Default to no `--now`, require opt-in. Document the trade-offs.

### Q2: Package Renames

**Resolution**: Treat as remove + add. No automatic rename detection.

### Q3: COPR Phase

**Resolution**: COPR management is part of Phase 1 (critical path for reproducibility).

### Q4: Weak Dependencies

**Resolution**: Defer to future work. Track only explicit dependencies.

### Q5: Delta RPMs

**Resolution**: Defer. Use rpm-ostree defaults.

## Future Possibilities

- **Version Pinning**: `bkt dnf install htop@3.2.1`
- **Profile Packages**: Different package sets per profile
- **Dependency Graph**: Visualize what depends on what
