# Vision: Capture-First Configuration Management

## The Core Loop

`bkt` implements a **capture-first** workflow for managing a Linux workstation. The philosophy is:

1. **Capture** is the daily verb — you make changes interactively, then capture them into manifests
2. **Apply** is for bootstrap and recovery — it reconstructs your environment from manifests

This inverts the traditional "edit config, then apply" model. Instead, your running system is the source of truth, and manifests are derived artifacts that enable reproducibility.

## Two-Tier Architecture

The system has two distinct tiers with different change semantics:

### Tier 1: Immutable Root Filesystem (bootc)

Changes here require a **reboot** to take effect:

- System packages (`dnf install`)
- Kernel arguments
- Systemd units
- System configuration in `/etc`

These are managed via the main `Containerfile` and `bootc` image builds.

### Tier 2: User-Space (Live Changes)

Changes here take effect **immediately**:

- Flatpaks
- AppImages
- GNOME extensions
- GSettings
- Distrobox containers

These are the primary domain for the capture-first workflow.

## The Distrobox Strategy

Development tools (compilers, runtimes, package managers) live in a distrobox container:

- **`bootc-dev`** — The primary development container
- **Host exports** — Selected binaries are exported to `/usr/bin` on the host

### Toolchain Philosophy

**Axiom**: Toolchain _installers_ are baked into the image; toolchain _state_ lives in the user's home directory.

| Component       | Lives In                        | Why                                             |
| --------------- | ------------------------------- | ----------------------------------------------- |
| `rustup` binary | Image (`/usr/local/bin/rustup`) | Available immediately on container creation     |
| `~/.rustup/`    | User's home (bind-mounted)      | Toolchains persist across container rebuilds    |
| `proto` binary  | Image (`/usr/local/bin/proto`)  | Available immediately on container creation     |
| `~/.proto/`     | User's home (bind-mounted)      | Node versions persist across container rebuilds |

### Init Hooks Axiom

**Init hooks must be idempotent AND bring the box up to date with what would happen on a fresh bootstrap.**

This means:

1. **Idempotent** — Running the hook twice produces the same result as running it once
2. **Bootstrap-equivalent** — After the hook runs, the container is in the same state as a freshly provisioned machine

#### Valid Init Hooks

```bash
# ✅ rustup update - idempotent, ensures latest stable toolchain
rustup update stable

# ✅ proto install - idempotent, ensures node is available
proto install node

# ✅ Set default toolchain - idempotent, ensures consistency
rustup default stable
```

#### Invalid Init Hooks

```bash
# ❌ Appending to a file - NOT idempotent (grows on each run)
echo "export PATH=$PATH" >> ~/.bashrc

# ❌ Installing a specific version - NOT bootstrap-equivalent
# (a fresh bootstrap would get the latest, not this pinned version)
rustup install 1.70.0

# ❌ Downloading something to a temp location - NOT bootstrap-equivalent
curl -o /tmp/setup.sh https://example.com/setup.sh
```

The key insight: if you ran `bkt apply` on a brand new machine, the init hooks should leave you in an identical state to an existing machine that has been using those hooks for months.

### Toolbox Image Update Workflow

The intended workflow for changing the toolbox image is:

1. Get the toolbox behaving correctly **locally**.
2. Update [toolbox/Containerfile](toolbox/Containerfile) to encode exactly what worked.
3. Open a PR and let CI rebuild/publish the image.
4. Validate that upgrading the distrobox works end-to-end once the PR image is available.

## Export Paths

The distrobox exports binaries from the container to the host. For toolchains installed via rustup and proto:

```json
{
  "bins": {
    "from": ["~/.cargo/bin", "~/.proto/bin", "~/.proto/shims"]
  }
}
```

Why three paths?

- `~/.cargo/bin` — Rust toolchain binaries (`cargo`, `rustc`, `rustfmt`, etc.)
- `~/.proto/bin` — Proto-managed tool binaries (`node`, `npm`, etc.)
- `~/.proto/shims` — Proto shims that delegate to the correct version

## Future Work

### Manifest-Driven Toolbox Containerfile

Currently, the `toolbox/Containerfile` is manually maintained. It should eventually use the same managed-sections pattern as the main `Containerfile`:

```dockerfile
# === TOOLBOX_PACKAGES (managed by bkt) ===
RUN dnf install -y \
    git \
    gcc \
    ...
# === END TOOLBOX_PACKAGES ===
```

This would allow `bkt containerfile` to manage both the base image and the toolbox image from their respective manifests.

### Auto-Capture Service

A systemd user service that periodically captures drift and commits changes:

```
~/.config/systemd/user/bkt-capture.timer
```

See [RFC 0007: Drift Detection](rfcs/0007-drift-detection.md) for details.
