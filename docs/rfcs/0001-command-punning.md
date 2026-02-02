# RFC 0001: Command Punning Philosophy

- **Status**: Foundational (Terminology Evolved)
- Feature Name: `command_punning`
- Start Date: 2025-12-31
- RFC PR: (leave this empty until PR is opened)
- Tracking Issue: (leave this empty)

> **ℹ️ Terminology Note**
>
> This RFC establishes the foundational **philosophy** of command punning, which
> remains the core design principle of `bkt`. However, the specific command
> examples have evolved:
>
> | RFC 0001 Example | Current Implementation | Notes |
> |------------------|------------------------|-------|
> | `bkt dnf install` | `bkt system add` | See RFC-0020 |
> | `bkt dev dnf install` | `bkt dev install` | See RFC-0020 |
> | `bkt image` prefix | Not implemented | PR-only mode uses `--pr-only` flag |
>
> The **philosophy** (immediate effect + persistence + PR propagation) is fully
> implemented. See [RFC-0020](0020-dev-and-system-commands.md) for current command structure.

## Summary

Establish a foundational design philosophy for `bkt` commands based on **command punning**: familiar CLI patterns that execute immediately while simultaneously propagating changes to the distribution via Git.

The core equation:

```
Command Punning = Muscle Memory + Distribution Propagation + Context-Aware Execution
```

## Motivation

Managing a personal Linux distribution requires two parallel concerns:

1. **Immediate effect**: Installing a package, changing a setting, adding an extension
2. **Persistence**: Ensuring that change survives the next image build

Traditional approaches force users to choose: either make ad-hoc changes that get lost, or maintain configuration files that require manual synchronization. `bkt` eliminates this false dichotomy.

### The Problem with Manual Synchronization

Consider installing a Flatpak:

```bash
# Step 1: Install locally
flatpak install flathub org.mozilla.firefox

# Step 2: Remember to update the manifest
vim manifests/flatpak-apps.json
# ... add the entry manually ...

# Step 3: Commit and push
git add manifests/flatpak-apps.json
git commit -m "Add Firefox"
git push
```

This workflow is error-prone:
- Users forget step 2 (drift accumulates)
- Manual JSON editing invites syntax errors
- The mental overhead discourages experimentation

### The Command Punning Solution

```bash
bkt flatpak add org.mozilla.firefox
```

One command:
1. Installs Firefox immediately (you can use it now)
2. Updates `manifests/flatpak-apps.json` with validated entry
3. Opens a PR to propagate to the distribution

The command "puns" on `flatpak install` — familiar enough to require no learning curve, but with superpowers.

## Guide-level Explanation

### Three Execution Contexts

`bkt` operates in three distinct contexts, each with different immediate effects and persistence targets:

| Context | Command Prefix | Immediate Effect | Persists To |
|---------|----------------|------------------|-------------|
| **Host system** | (none) | rpm-ostree, flatpak, gsettings | Image PR |
| **Toolbox** | `bkt dev` | dnf, direct install | Toolbox Containerfile |
| **Build-time only** | `bkt image` | None | Image PR |

#### Host Context (Default)

Commands without a prefix operate on the immutable host system:

```bash
# Immediately installs via flatpak, updates manifest, opens PR
bkt flatpak add org.gnome.Calculator

# Immediately applies gsetting, updates manifest, opens PR  
bkt gsetting set org.gnome.desktop.interface gtk-theme Adwaita-dark

# Layers package via rpm-ostree, updates Containerfile, opens PR
bkt dnf install htop
```

#### Toolbox Context (`bkt dev`)

The `dev` prefix targets the development toolbox:

```bash
# Installs in toolbox via dnf, updates toolbox/Containerfile
bkt dev dnf install gcc

# Installs Rust toolchain in toolbox
bkt dev rustup default stable
```

**Invalid combinations fail fast:**

```bash
bkt dev flatpak add org.mozilla.firefox
# Error: Flatpaks are host-level. Use `bkt flatpak add` instead.
```

#### Build-time Context (`bkt image`)

The `image` prefix skips immediate execution entirely:

```bash
# Only updates manifests and opens PR, no local effect
bkt image flatpak add org.gnome.Boxes

# Useful when preparing changes for a fresh image
bkt image dnf install podman
```

### Punnable Commands

#### Package Management

| Familiar Command | Punned Version | Behavior |
|------------------|----------------|----------|
| `flatpak install` | `bkt flatpak add` | Install + manifest + PR |
| `flatpak uninstall` | `bkt flatpak remove` | Uninstall + manifest + PR |
| `dnf install` | `bkt dnf install` | rpm-ostree overlay + Containerfile + PR |
| `dnf remove` | `bkt dnf remove` | rpm-ostree uninstall + Containerfile + PR |

#### Settings Management

| Familiar Command | Punned Version | Behavior |
|------------------|----------------|----------|
| `gsettings set` | `bkt gsetting set` | Apply + manifest + PR |
| `gsettings reset` | `bkt gsetting reset` | Reset + manifest + PR |
| `dconf write` | `bkt dconf write` | Write + manifest + PR |

#### GNOME Extensions

| Familiar Command | Punned Version | Behavior |
|------------------|----------------|----------|
| `gnome-extensions install` | `bkt extension add` | Install + manifest + PR |
| `gnome-extensions enable` | `bkt extension enable` | Enable + manifest + PR |
| `gnome-extensions disable` | `bkt extension disable` | Disable + manifest + PR |

### The `--pr` Flag

All mutating commands support `--pr` to skip immediate execution:

```bash
# Only creates PR, does not install locally
bkt flatpak add --pr org.gnome.Calculator

# Batch multiple changes into one PR
bkt flatpak add --pr org.gnome.Calculator
bkt flatpak add --pr org.gnome.TextEditor  
bkt gsetting set --pr org.gnome.desktop.interface gtk-theme Adwaita-dark
bkt pr submit  # Submits accumulated changes
```

This is useful for:
- Preparing changes on one machine for another
- Batching related changes into a single PR
- Testing manifest changes in CI before applying locally

### The `--local` Flag

Conversely, `--local` skips PR creation:

```bash
# Install locally, do not open PR
bkt flatpak add --local org.gnome.Calculator
```

This is useful for:
- Temporary installations
- Testing before committing
- Machines that shouldn't modify the distribution

### Ephemeral Manifest: Tracking Local Changes

When `--local` is used, the change is recorded in an **ephemeral manifest** at `~/.local/share/bkt/ephemeral.json`. This manifest:

1. **Tracks all local-only changes** since last reboot
2. **Invalidates on reboot** (using `/proc/sys/kernel/random/boot_id`)
3. **Enables later promotion** to a proper PR

```bash
# View local changes not yet in a PR
bkt local list
# Added (local-only):
#   flatpak: org.gnome.Calculator
#   dnf: htop
#
# These changes will be lost on reboot or image switch.

# Promote all local changes to a PR
bkt local commit
# Creates PR with all local changes

# Promote specific changes
bkt local commit --select
# Interactive selection of which changes to include

# Clear local tracking (changes remain installed but untracked)
bkt local clear
```

The ephemeral manifest includes sufficient metadata to construct the appropriate PR:

```json
{
  "boot_id": "80676aac-1390-4bd4-9ec9-e2d57ad677bb",
  "changes": [
    {
      "domain": "flatpak",
      "action": "add",
      "identifier": "org.gnome.Calculator",
      "timestamp": "2025-01-02T10:30:00Z",
      "metadata": {
        "remote": "flathub"
      }
    }
  ]
}
```

**Boot ID Validation**: On each `bkt` invocation, the current boot ID is compared against the cached `boot_id`. If they differ, the ephemeral manifest is cleared.

## Reference-level Explanation

### Command Structure

```
bkt [CONTEXT] <DOMAIN> <ACTION> [OPTIONS] [ARGS...]
```

Where:
- `CONTEXT` is optional: `dev` or `image`
- `DOMAIN` is required: `flatpak`, `dnf`, `gsetting`, `extension`, `shim`, etc.
- `ACTION` is required: `add`, `remove`, `set`, `list`, `sync`, etc.

### Execution Pipeline

Every mutating command follows this pipeline:

```
+-------------+     +-------------+     +-------------+     +-------------+
|   Validate  |---->|   Execute   |---->|   Update    |---->|  Propagate  |
|    Input    |     |   Locally   |     |  Manifests  |     |    (PR)     |
+-------------+     +-------------+     +-------------+     +-------------+
                          |                                        |
                          | --pr flag                              | --local flag
                          v                                        v
                       [SKIP]                                   [SKIP]
```

### Error Handling

Commands must fail fast with actionable errors. **Error messages should suggest appropriate next steps** to support both human users and AI agents:

```bash
bkt dev flatpak add org.mozilla.firefox
# Error: Invalid context for domain
# 
# Flatpaks are installed at the host level, not in toolboxes.
# 
# Did you mean:
#   bkt flatpak add org.mozilla.firefox
#
# Next steps:
#   - Run: bkt flatpak add org.mozilla.firefox
```

**AI Steering Principle**: Every error message should include a "Next steps" section with:
- Concrete commands to run
- Safe actions without side effects
- Simplest fix first

### Script and AI Usage Guidance

`bkt` commands are designed for **interactive human use**, not for scripting or automation.

**For scripts**: Use the underlying tools directly (`flatpak install`, `rpm-ostree install`, etc.).

**For AI agents**: Use `bkt` only when the user explicitly wants distribution management.

```bash
bkt dnf install htop --script
# Error: bkt commands are not designed for scripting
#
# For scripts, use the underlying tools directly:
#   rpm-ostree install htop    # Host system
#   dnf install htop           # Toolbox
```

**Detecting non-interactive usage**: Commands warn when stdin is not a TTY. The `--yes` flag is intentionally not provided.

### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | General error |
| 2 | Invalid arguments |
| 3 | Invalid context/domain combination |
| 4 | External command failed |
| 5 | Manifest validation failed |
| 6 | Git/PR operation failed |
| 7 | Network error |

### JSON Output

All commands support `--json` for machine-readable output:

```bash
bkt flatpak add --json org.gnome.Calculator
```

```json
{
  "success": true,
  "domain": "flatpak",
  "action": "add",
  "identifier": "org.gnome.Calculator",
  "local_effect": {
    "executed": true,
    "command": "flatpak install -y flathub org.gnome.Calculator"
  },
  "manifest_update": {
    "file": "manifests/flatpak-apps.json",
    "diff": "+ { \"id\": \"org.gnome.Calculator\", \"remote\": \"flathub\" }"
  },
  "pr": {
    "created": true,
    "url": "https://github.com/wycats/bootc/pull/42"
  }
}
```

## Drawbacks

### Learning Curve for Context Prefixes

Users must learn that `bkt dev` and `bkt image` exist. However:
- The default (no prefix) handles 90% of use cases
- Error messages guide users to correct commands

### Potential for Confusion with Native Commands

`bkt dnf install` is not `dnf install`. Mitigation: Clear documentation and `--dry-run`.

### Dependency on GitHub

PR creation requires GitHub access. Mitigation: `--local` flag for offline operation.

## Rationale and Alternatives

### Why "Punning"?

The term "punning" captures the dual nature: the command means one thing (execute locally) and another thing (propagate to distribution) simultaneously.

### Alternative: Separate Commands

Rejected because three commands instead of one is error-prone.

### Alternative: Git Hooks

Rejected because it inverts the natural workflow.

### Alternative: Declarative-Only

Rejected because it discourages experimentation with slow feedback loops.

## Prior Art

- **Nix Home Manager**: Declarative model, requires editing files first
- **Homebrew Bundle**: Generates Brewfile from installed packages
- **chezmoi**: Manages dotfiles with automatic git operations

## Unresolved Questions

### Q1: Branch Naming Strategy

**Resolution**: Individual branches for atomic changes, with `bkt pr batch` for grouping related changes.

### Q2: Conflict Resolution

**Resolution**: Fail and ask user to resolve. Suggest: `git pull origin main && bkt pr retry`.

### Q3: Rollback Semantics

**Resolution**: Defer to future work. Use `bootc rollback` for image-level rollback.

### Q4: Multi-Machine Coordination

**Resolution**: Each machine pulls and applies via `bkt sync`. Git is the coordination mechanism.

## Future Possibilities

- **Dependency Tracking**: `bkt flatpak add --with-gsettings`
- **Diff Against Upstream**: `bkt diff`
- **Profile Snapshots**: `bkt snapshot create/restore`
- **Interactive Mode**: TUI for browsing packages

## Implementation Checklist

### Phase 1: Core Infrastructure
- [ ] Manifest parsing and validation
- [ ] JSON schema validation
- [ ] Basic PR creation via `gh`

### Phase 2: Context Support
- [ ] Implement `bkt dev` prefix
- [ ] Implement `bkt image` prefix
- [ ] Add `--pr` and `--local` flags

### Phase 3: Ephemeral Tracking
- [ ] Boot ID detection
- [ ] Ephemeral manifest format
- [ ] `bkt local list/commit/clear`

### Phase 4: Polish
- [ ] `--json` output
- [ ] `--dry-run`
- [ ] Comprehensive error messages
