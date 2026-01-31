# RFC 0032: Binary Acquisition (fetchbin)

- Feature Name: `binary_acquisition`
- Start Date: 2026-01-27
- RFC PR: (leave this empty until PR is opened)
- Tracking Issue: (leave this empty)

## Summary

`fetchbin` is a standalone Rust crate and CLI that installs binaries from npm, cargo-binstall, and GitHub releases into a user-scoped store, without requiring those ecosystems on PATH. It manages Node.js and pnpm as invisible infrastructure, records installs in a manifest, and keeps binaries available via a dedicated bin directory.

## Motivation

Many CLI tools are distributed through ecosystem-specific channels (npm, cargo, GitHub releases) even though users just want a standalone executable. Requiring those ecosystems on PATH adds clutter and makes installs non-reproducible. `fetchbin` provides a single, minimal toolchain for acquiring binaries, tracking versions, and keeping runtime dependencies isolated.

## Guide-level Explanation

### Basic usage

```bash
# Install from npm
fetchbin install npm:turbo

# Install from cargo-binstall
fetchbin install cargo:ripgrep

# Install from GitHub releases (auto-match platform assets)
fetchbin install github:jesseduffield/lazygit

# Choose a specific GitHub asset pattern
fetchbin install github:jesseduffield/lazygit --asset "lazygit_*_Linux_x86_64.tar.gz"

# Select a specific binary name from a multi-binary package
fetchbin install npm:@biomejs/biome --bin biome

# List, update, remove
fetchbin list
fetchbin update
fetchbin remove lazygit
```

### Key behaviors

- Installs go under $HOME/.local/share/fetchbin
- A bin directory is maintained at $HOME/.local/share/fetchbin/bin
- `fetchbin list` only reports what is recorded in the manifest
- npm packages are installed with pnpm and wrapper scripts are generated to run them with the managed Node runtime

### Directory layout

```
~/.local/share/fetchbin/
├── bin/                 # Symlinks to installed binaries
├── store/               # Versioned installs (npm/cargo/github)
├── toolchains/          # Managed Node, pnpm, cargo-binstall
├── manifest.json        # Installed binaries + metadata
└── runtime.json         # Runtime pool state
```

### Out of scope for this RFC

The following are explicitly not implemented in the current code and are deferred:

- `--all-bins` flag
- Interactive prompts for multi-binary selection
- Idempotent installs (install always re-downloads)
- Self-healing checks on `list`
- npm `dist.integrity` verification
- `--allow-scripts` for npm lifecycle scripts
- `verified` field in manifest
- `--require-verified` flag

## Reference-level Explanation

### Sources

`fetchbin` implements three sources via the `BinarySource` trait:

- **NpmSource**
  - Resolves versions from the npm registry.
  - Reads `bin` and `engines.node` metadata.
  - Installs via pnpm with `--ignore-scripts`.
  - Generates wrapper scripts (not symlinks) that invoke the managed Node runtime.
  - `--bin` selects a specific binary; otherwise multiple binaries raise a `MultipleBinaries` error.

- **CargoSource**
  - Resolves versions via crates.io metadata.
  - Uses a managed `cargo-binstall` download.
  - Installs to a versioned store and selects a binary (default: crate name, or `--bin`).

- **GithubSource**
  - Lists GitHub releases and selects a matching asset.
  - Supports `--asset` to override the pattern.
  - Falls back to platform-specific heuristics when no `--asset` is provided.
  - Extracts `tar.gz`, `tgz`, `zip`, or writes raw assets.
  - Verifies checksum when a matching checksum asset is present.

### RuntimePool

`RuntimePool` manages runtime infrastructure and stores state in runtime.json:

- **Node.js**: Downloaded from nodejs.org with SHA256 verification using `SHASUMS256.txt`.
- **pnpm**: Downloaded from GitHub releases with SHA256 verification from the `.sha256` asset.
- **cargo-binstall**: Downloaded from GitHub releases with checksum validation via `checksums.txt`.

Node selection behavior:

1. If an npm package specifies `engines.node`, that requirement is used.
2. Otherwise, the pool defaults to the current LTS.
3. The pool downloads missing versions automatically.

### Manifest tracking

- `manifest.json` tracks installed binaries, including source information, chosen binary name, SHA256 of the installed artifact, and runtime association.
- `runtime.json` tracks runtime state (installed Node versions and pnpm version).

Example manifest entry:

```json
{
  "binaries": {
    "turbo": {
      "source": { "type": "npm", "package": "turbo", "version": "2.3.4" },
      "binary": "turbo",
      "sha256": "abc123...",
      "installed_at": "1706359200",
      "runtime": { "type": "node", "version": "22.2.0" }
    }
  }
}
```

### Install/update flow

1. Resolve the latest matching version.
2. Remove any existing store directory for that version.
3. Fetch the binary from the selected source.
4. Link it into $HOME/.local/share/fetchbin/bin.
5. Update manifest.json.
6. Prune unused Node runtimes and save runtime.json.

## Drawbacks

- Installs are not idempotent; repeated installs always re-download.
- No interactive selection for multi-binary packages.
- No self-healing on `list`.
- npm tarball integrity (`dist.integrity`) is not verified.
- npm lifecycle scripts are always ignored.

## Rationale and Alternatives

- **pnpm for npm installs**: pnpm is fast, supports isolated installs, and is easy to bootstrap.
- **Wrapper scripts for Node CLIs**: ensures each binary runs with the managed Node runtime without polluting PATH.
- **cargo-binstall**: uses prebuilt binaries instead of compiling from source.
- **Platform heuristics for GitHub assets**: reduces the need for manual asset selection when releases are consistent.

Alternatives include delegating to system package managers or requiring node/cargo on PATH. Those options add global state and reduce reproducibility.

## Prior art

- `pipx` for isolated per-package installs
- `cargo-binstall` for fast Rust binary acquisition
- `nvm`/`fnm` for Node runtime management
- GitHub release downloaders (e.g., `gh`)

## Unresolved questions

- Should `fetchbin list` gain optional verification (`--verify`) or a separate repair command? (tracked in RFC 0033)
- Should npm integrity verification be mandatory once implemented? (tracked in RFC 0033)
