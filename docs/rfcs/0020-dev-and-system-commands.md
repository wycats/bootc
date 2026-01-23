# RFC 0020: Dev and System Package Commands

- Feature Name: `dev_and_system_commands`
- Start Date: 2026-01-23
- RFC PR: #86
- Tracking Issue: (leave this empty)

## Summary

Replace the unified `bkt dnf` command with two distinct commands: `bkt dev` for toolbox package management and `bkt system` for image package management. This separation reflects the fundamentally different execution models of these two domains.

> **Supersedes:** This RFC supersedes the `bkt dnf` commands defined in RFC 0002 and the `bkt dev dnf` delegation defined in RFC 0003.

## Motivation

### The Current Problem

The existing `bkt dnf install` command auto-detects context and behaves differently:

```bash
# On host: uses rpm-ostree, updates system-packages.json
bkt dnf install gcc

# In toolbox: uses dnf, updates toolbox-packages.json
bkt dnf install gcc
```

This conflation causes confusion:

1. **Same command, different meanings** — Users can't tell from the command what will happen
2. **`--pr-only` was confusing on host** — What does "skip local execution" mean when the package won't exist until the image rebuilds anyway? With `bkt system`, there IS no local execution to skip, so the flag becomes meaningless.
3. **Different mental models** — Toolbox packages are immediate; system packages are deferred

### Two Fundamentally Different Operations

#### Dev: "Install and Record"

```bash
bkt dev install gcc
```

- **Primary action:** Install the package NOW (via dnf in toolbox)
- **Secondary action:** Record in manifest (bookkeeping for future containers)
- **PR workflow:** Not typical — toolbox state is local/ephemeral
- **Mental model:** Like `npm install --save` — you want it now AND tracked

#### System: "Add to Image Recipe"

```bash
bkt system add virt-manager
```

- **Primary action:** Update manifest + Containerfile → Create PR
- **Local execution:** None (the package doesn't exist until image rebuilds)
- **PR workflow:** Fundamental — the PR IS the installation mechanism
- **Mental model:** Like editing a Dockerfile — you're changing the build recipe

### The Key Insight

**"Install" implies immediacy.** But system packages are deferred until the next image build. This mismatch creates cognitive dissonance.

The solution: Use **"install"** for immediate actions, **"add"** for deferred actions.

## Guide-level Explanation

### New Command Structure

#### `bkt dev` — Toolbox Package Management

For packages in your development toolbox (mutable, immediate):

```bash
bkt dev install <package>     # dnf install in toolbox + update manifest
bkt dev remove <package>      # dnf remove + update manifest
bkt dev list                  # show toolbox-packages.json contents
bkt dev sync                  # install all packages from manifest
bkt dev capture               # capture installed packages to manifest
```

Flags:
- `--manifest-only` — Update manifest without executing dnf (for batch operations)

#### `bkt system` — Image Package Management

For packages baked into your bootc image (immutable, deferred):

```bash
bkt system add <package>      # update manifest + Containerfile + create PR
bkt system remove <package>   # remove from manifest + Containerfile + PR
bkt system list               # show system-packages.json contents
bkt system capture            # capture rpm-ostree layered packages to manifest
```

Flags:
- `--local` — Update manifest without creating PR (for batch changes)

Note: There is no `bkt system sync` — syncing happens at image build time via the Containerfile.

### Verb Consistency Across bkt

This change clarifies an existing pattern:

| Verb | Meaning | Used By |
|------|---------|---------|
| **install** | Execute NOW in toolbox + record | `bkt dev install` |
| **add** | Install/enable + record in manifest | `bkt flatpak add`, `bkt extension add`, `bkt system add`, `bkt appimage add`, `bkt upstream add`, `bkt shim add` |
| **remove** | Remove (immediate or deferred, matching the domain) | All domains |
| **sync** | Make system match manifest | `bkt dev sync`, `bkt flatpak sync`, `bkt extension sync` |
| **capture** | Make manifest match system | All domains |

**Why does `bkt dev` use "install" while `bkt flatpak` uses "add"?**

The distinction is subtle but intentional:

- **`bkt dev install`** — Emphasizes the immediate execution. Toolbox packages are ephemeral; the manifest is bookkeeping for recreating the environment.
- **`bkt flatpak add`** — Emphasizes adding to the tracked configuration. Flatpaks are host-level and persistent; the installation happens as a side effect of adding to the manifest.

Both execute immediately and both update manifests, but the emphasis differs based on the domain's mental model.

### What Happens to `bkt dnf`?

**Removed entirely.** For package discovery, use dnf directly:

```bash
dnf search ripgrep    # works via distrobox shim on host
dnf info ripgrep      # pure passthrough, no bkt value-add
dnf provides /usr/bin/rg
```

The `bkt dnf` command provided no value for queries — it was just a passthrough. Removing it eliminates a confusing command that hid context-dependent behavior.

## Reference-level Explanation

### Command Implementation

#### `bkt dev install`

```rust
pub enum DevAction {
    Install {
        packages: Vec<String>,
        #[arg(long)]
        manifest_only: bool,
        #[arg(long)]
        force: bool,
    },
    Remove { packages: Vec<String> },
    List { format: String },
    Sync,
    Capture { apply: bool },
}
```

Execution flow:
1. If not `--manifest-only`: Run `dnf install -y <packages>` in current toolbox
2. On success: Add packages to `manifests/toolbox-packages.json`
3. No PR creation (toolbox is local state)

#### `bkt system add`

```rust
pub enum SystemAction {
    Add {
        packages: Vec<String>,
        #[arg(long)]
        force: bool,
    },
    Remove { packages: Vec<String> },
    List { format: String },
    Capture { apply: bool },
}
```

Execution flow:
1. Validate packages exist in repos (optional, can skip with `--force`)
2. Add packages to `manifests/system-packages.json`
3. Regenerate Containerfile SYSTEM_PACKAGES section
4. If not `--local`: Create PR with both changes
5. **No local rpm-ostree execution** — package appears after image rebuild

> **Behavior Change:** The previous `bkt dnf install` on host would run `rpm-ostree install` locally, staging the package for next boot. This RFC eliminates that local execution. System packages are now purely image-based: update the recipe, rebuild, reboot. This aligns with the immutable image philosophy and avoids drift between local state and image definition.

### Manifest Locations

| Command | Manifest |
|---------|----------|
| `bkt dev` | `manifests/toolbox-packages.json` |
| `bkt system` | `manifests/system-packages.json` |

### COPR Repository Management

COPR follows the same split:

```bash
bkt dev copr enable atim/starship      # enable in toolbox + manifest
bkt system copr enable atim/starship   # add to system manifest + Containerfile + PR
```

> **Transition:** These commands replace `bkt dnf copr enable` from RFC 0002. There is no generic `bkt copr` command — users must explicitly choose `bkt dev copr` or `bkt system copr` based on their target environment.

## Drawbacks

1. **Breaking change** — Existing `bkt dnf` commands will fail. However, the only user is the author, so this is acceptable.

2. **More commands to remember** — Two commands instead of one. However, each command is clearer about what it does.

3. **Loss of context auto-detection** — Users must know whether they want dev or system. This is actually a feature — explicitness prevents mistakes.

## Rationale and Alternatives

### Why not keep `bkt dnf` with subcommands?

```bash
bkt dev dnf install gcc
bkt system dnf add virt-manager
```

This preserves the `dnf` namespace but adds nesting. The proposed structure is flatter and matches how users think: "I'm working on my dev environment" vs "I'm changing my system image."

### Why not `bkt image add` instead of `bkt system add`?

Both could work. `system` was chosen because:
- It parallels `system-packages.json`
- "Image" might confuse with container images generally
- "System" emphasizes "this is your operating system"

### Why remove `bkt dnf` entirely?

The query commands (`search`, `info`, `provides`) add no value over running `dnf` directly. Keeping them would maintain a confusing namespace that hides the dev/system split.

## Prior Art

- **Nix**: Separates `nix-env -i` (user profile) from `configuration.nix` (system)
- **Homebrew on Linux**: User-space package manager, separate from system packages
- **npm**: `npm install` (local) vs system package managers

## Unresolved Questions

1. **Should `bkt system sync` exist?** It would regenerate the Containerfile from the manifest. Currently this happens automatically during `bkt system add/remove`.

2. **Should `bkt dev` work from the host?** Currently requires being inside the toolbox. Could detect and use `distrobox-enter` wrapper if on host.

3. **Naming for future dev toolchains**: When we add `bkt dev rustup` and `bkt dev npm`, should they be subcommands of `bkt dev` or their own top-level commands?

## Future Possibilities

### Extended `bkt dev` Commands

```bash
bkt dev rustup default stable       # manage Rust toolchain
bkt dev npm install -g typescript   # global npm packages
bkt dev script add <url>            # tracked install scripts
```

### Extended `bkt system` Commands

```bash
bkt system copr enable <repo>       # COPR repos in image
bkt system kargs append <arg>       # kernel arguments
```

## Implementation Plan

1. Create `bkt/src/commands/system.rs` with new `SystemAction` enum
2. Split existing `dnf.rs`:
   - Move host/image package logic into `system.rs`
   - Move toolbox/dev package logic into `dev.rs`
3. Rename/refactor `dev.rs` to implement new `DevAction` enum
4. Hide `bkt dnf` command from CLI (keep working for compatibility)
5. Update help text and documentation
6. Update `bkt capture` to use new command structure
