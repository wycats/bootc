# RFC 0003: Developer Tools (`bkt dev`)

- Feature Name: `bkt_dev`
- Start Date: 2025-12-31
- RFC PR: (leave this empty until PR is opened)
- Tracking Issue: (leave this empty)

## Summary

Implement `bkt dev` commands for managing the development toolbox environment, including packages, language runtimes, and toolchain configuration, with automatic Containerfile generation.

## Motivation

Development environments need:
1. **Language runtimes**: Node.js, Rust, Go, Python with specific versions
2. **Build tools**: compilers, linkers, cmake, meson
3. **CLI utilities**: git, gh, jq, ripgrep
4. **IDE support**: language servers, debuggers

On an immutable OS, these belong in a toolbox container, not on the host. But toolbox containers are ephemeral by default - they need a Containerfile to be reproducible.

### Current Pain Points

```bash
# Enter toolbox
toolbox enter

# Install a bunch of stuff
sudo dnf install gcc cmake ninja-build
rustup default stable
rustup component add rust-analyzer
npm install -g typescript-language-server

# Now... how do I make this permanent?
# Manually edit a Containerfile? Which one?
# What about rustup/npm - those aren't dnf packages?
```

### The Solution

```bash
# DNF packages
bkt dev dnf install gcc cmake ninja-build

# Rust toolchain
bkt dev rustup default stable
bkt dev rustup component add rust-analyzer

# Node packages
bkt dev npm install -g typescript-language-server

# All changes are tracked and the Containerfile is auto-generated
bkt dev rebuild  # Rebuild toolbox from Containerfile
```

## Guide-level Explanation

### The Development Toolbox

`bkt` manages a development toolbox with a layered configuration:

```
toolbox/
├── Containerfile          # Generated - do not edit
├── manifest.json          # Source of truth
├── scripts/               # Custom setup scripts (curl-pipe workflows)
│   ├── starship.sh
│   └── mise.sh
└── dotfiles/              # Files to copy into container
    └── .bashrc.d/
        └── bkt.sh
```

### Package Management

```bash
# Install packages (updates manifest, regenerates Containerfile)
bkt dev dnf install neovim ripgrep fd-find

# Install package groups
bkt dev dnf group install "C Development Tools and Libraries"

# Remove packages
bkt dev dnf remove nano
```

### Language Runtime Management

#### Rust (rustup)

```bash
# Set default toolchain
bkt dev rustup default stable

# Add components
bkt dev rustup component add rust-analyzer clippy

# Add targets
bkt dev rustup target add wasm32-unknown-unknown
```

This generates:

```dockerfile
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y \
    --default-toolchain stable \
    --component rust-analyzer,clippy \
    --target wasm32-unknown-unknown
```

#### Node.js (nvm/fnm/volta)

```bash
# Install Node version
bkt dev node use 20

# Install global packages
bkt dev npm install -g typescript prettier eslint
```

#### Python (pyenv/uv)

```bash
# Install Python version
bkt dev python use 3.12

# The toolbox uses uv for fast installs
bkt dev pip install ipython
```

### Curl-Pipe Scripts: `bkt dev script`

Many developer tools use the `curl | sh` installation pattern. `bkt` provides a managed way to handle these:

```bash
bkt dev script add https://starship.rs/install.sh
# 1. First run: Displays script, prompts for confirmation
# 2. Records SHA256 hash in manifest
# 3. Adds to Containerfile
# 4. Future runs: Verifies hash before execution
```

#### Security Model

```
+-------------------------------------------------------------+
|                    First Run                                |
+-------------------------------------------------------------+
|  1. Fetch script from URL                                   |
|  2. Display script content for review                       |
|  3. Prompt: "Execute this script? [y/N]"                    |
|  4. If yes:                                                 |
|     - Execute script                                        |
|     - Record SHA256 in manifest                             |
|     - Add to Containerfile                                  |
+-------------------------------------------------------------+

+-------------------------------------------------------------+
|                    Subsequent Runs                          |
+-------------------------------------------------------------+
|  1. Fetch script from URL                                   |
|  2. Compute SHA256                                          |
|  3. Compare against manifest                                |
|  4. If match: Execute silently                              |
|  5. If mismatch:                                            |
|     - Display diff                                          |
|     - Prompt: "Script changed. Review and approve? [y/N]"   |
|     - If yes: Update hash, execute                          |
+-------------------------------------------------------------+
```

#### Manifest Entry

```json
{
  "scripts": [
    {
      "name": "starship",
      "url": "https://starship.rs/install.sh",
      "sha256": "abc123...",
      "args": ["-y"],
      "first_approved": "2025-01-02T10:30:00Z"
    }
  ]
}
```

#### Containerfile Generation

```dockerfile
# === SCRIPTS (managed by bkt) ===
# starship (approved: 2025-01-02, sha256: abc123...)
RUN curl -fsSL https://starship.rs/install.sh | sh -s -- -y
# === END SCRIPTS ===
```

### Containerfile Integration

`bkt dev` generates and maintains `toolbox/Containerfile`:

```dockerfile
FROM registry.fedoraproject.org/fedora-toolbox:41

# === COPR REPOSITORIES (managed by bkt) ===
RUN dnf -y copr enable atim/starship
# === END COPR REPOSITORIES ===

# === SYSTEM PACKAGES (managed by bkt) ===
RUN dnf install -y \
    cmake \
    fd-find \
    gcc \
    neovim \
    ripgrep \
    starship
# === END SYSTEM PACKAGES ===

# === SCRIPTS (managed by bkt) ===
# starship (approved: 2025-01-02, sha256: abc123...)
RUN curl -fsSL https://starship.rs/install.sh | sh -s -- -y
# === END SCRIPTS ===

# === RUST TOOLCHAIN (managed by bkt) ===
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y \
    --default-toolchain stable \
    --component rust-analyzer,clippy
# === END RUST TOOLCHAIN ===

# === NODE.JS (managed by bkt) ===
ENV NODE_VERSION=20
RUN curl -fsSL https://fnm.vercel.app/install | bash
RUN fnm install $NODE_VERSION && fnm default $NODE_VERSION
RUN npm install -g typescript prettier eslint
# === END NODE.JS ===

# === DOTFILES (managed by bkt) ===
COPY dotfiles/ /etc/skel/
# === END DOTFILES ===
```

### Building and Updating

```bash
# Incremental update: Install changes in current container + update manifest
bkt dev update

# Full rebuild: Regenerate Containerfile and rebuild image
bkt dev rebuild

# Show what would change
bkt dev diff
```

**`bkt dev update`**: Applies changes to the running toolbox without rebuilding. Fast for iteration.

**`bkt dev rebuild`**: Destroys current toolbox and rebuilds from Containerfile. Ensures clean state.

### Environment Files

```bash
# Add environment variable
bkt dev env set EDITOR nvim

# Add to PATH
bkt dev path add ~/.cargo/bin

# Source a file
bkt dev source ~/.bashrc.d/bkt.sh
```

## Reference-level Explanation

### Manifest Structure

```json
// toolbox/manifest.json
{
  "base_image": "registry.fedoraproject.org/fedora-toolbox:41",
  "packages": {
    "dnf": ["gcc", "cmake", "ninja-build"],
    "groups": ["C Development Tools and Libraries"]
  },
  "scripts": [
    {
      "name": "starship",
      "url": "https://starship.rs/install.sh",
      "sha256": "abc123...",
      "args": ["-y"]
    }
  ],
  "rust": {
    "default_toolchain": "stable",
    "components": ["rust-analyzer", "clippy"],
    "targets": ["wasm32-unknown-unknown"]
  },
  "node": {
    "version": "20",
    "manager": "fnm",
    "global_packages": ["typescript", "prettier", "eslint"]
  },
  "python": {
    "version": "3.12",
    "manager": "uv"
  },
  "environment": {
    "EDITOR": "nvim"
  },
  "path_additions": [
    "~/.cargo/bin",
    "~/.local/bin"
  ]
}
```

### Containerfile Generation

The Containerfile is **always generated** from `manifest.json`. Users should never edit it directly.

Generation happens:
- After any `bkt dev` mutating command
- On `bkt dev rebuild`
- On `bkt dev generate` (explicit regeneration)

### Command Execution Flow

```
bkt dev dnf install gcc
         |
         v
+---------------------+
| 1. Validate package |
|    exists in repos  |
+---------------------+
         |
         v
+---------------------+
| 2. Run in toolbox:  |
|    dnf install gcc  |
+---------------------+
         |
         v
+---------------------+
| 3. Update manifest  |
|    packages.dnf[]   |
+---------------------+
         |
         v
+---------------------+
| 4. Regenerate       |
|    Containerfile    |
+---------------------+
         |
         v
+---------------------+
| 5. Stage changes    |
|    (no PR for dev)  |
+---------------------+
```

### Toolbox Lifecycle

```bash
# Create/recreate toolbox from Containerfile
bkt dev create

# Enter toolbox (creates if needed)
bkt dev enter

# Destroy toolbox (keep Containerfile)
bkt dev destroy

# List available toolboxes
bkt dev list
```

## Drawbacks

### Complexity of Multi-Runtime Support

Supporting Rust, Node, Python, etc. adds complexity. Mitigation: modular implementation.

### Containerfile as Output Only

Users can't customize the Containerfile directly. Mitigation: extensibility via scripts.

### Script Security

Curl-pipe scripts are inherently risky. Mitigation: hash verification and explicit approval.

## Rationale and Alternatives

### Why Generate Containerfile?

Maintaining both a manifest and Containerfile manually leads to drift. Generation ensures consistency.

### Alternative: Just Use Containerfile

Rejected because Containerfiles are hard to parse/modify programmatically.

### Alternative: Nix Flakes

More powerful but much steeper learning curve.

## Prior Art

- **devcontainer.json**: VS Code's approach (similar manifest to container concept)
- **mise/asdf**: Multi-runtime version management
- **Brewfile**: Homebrew's declarative approach

## Unresolved Questions

### Q1: Multiple Toolboxes

**Resolution**: Single default toolbox for Phase 1. Future: named toolboxes.

### Q2: IDE Integration

**Resolution**: Document VS Code devcontainer.json integration path for future.

### Q3: GPU/Hardware Access

**Resolution**: Document manual Containerfile additions for special hardware.

### Q4: Ephemeral vs Persistent

**Resolution**: Toolbox is rebuilt from Containerfile. Data in `~/` persists via bind mount.

## Future Possibilities

- **Project-specific Toolboxes**: `.bkt/toolbox/` in project root
- **Remote Toolboxes**: Run toolbox on remote machine
- **Snapshots**: Save/restore toolbox state
- **Devcontainer Export**: Generate devcontainer.json
