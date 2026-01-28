# Current Work: Phase 4 — Manifest Fidelity & Workflow Gaps

This document tracks Phase 4 of the bootc distribution development. Phase 3 is archived at [docs/history/003-phase3-complete-the-loop.md](docs/history/003-phase3-complete-the-loop.md).

Quality backlog: [QUALITY.md](QUALITY.md)
Distrobox integration RFC: [docs/rfcs/0017-distrobox-integration.md](docs/rfcs/0017-distrobox-integration.md)

---

## Vision

**Phase 3** closed the manifest→Containerfile loop: auto-generation, post-reboot automation, drift visibility, and ephemeral tracking all work.

**Phase 4** addresses the gaps between what users expect and what `bkt` actually captures:

1. **Manifest Fidelity**: Capture the _full_ configuration state, not just "is it installed?"
2. **Command Punning Completion**: `bkt dnf install` should work for host packages, not just toolbox
3. **Development Environment**: `bkt dev` commands should actually execute, not just update manifests
4. **Upstream Dependencies**: Track themes, icons, fonts with pinned versions

The guiding principle: **If you configured it, `bkt` should capture it.**

### The Fidelity Gap

```
What the system knows          What bkt captures
─────────────────────          ─────────────────
Extension: installed ✓         ✅ Captured
Extension: enabled/disabled    ❌ Not captured ← Gap
Flatpak: installed ✓           ✅ Captured
Flatpak: permissions (Flatseal)❌ Not captured ← Gap
GSettings: current values      ⚠️ Manual schema only
Themes/Icons/Fonts             ❌ Not tracked  ← Gap
```

Phase 4 closes these fidelity gaps.

---

## 🔥 Urgent: Distrobox Live Capture

**Status:** ✅ Complete  
**Priority:** P0 - Do immediately after PR #78 merges

### Problem

`bkt distrobox capture` only reads from `distrobox.ini` (a format converter), NOT from running containers. Users can't use BoxBuddy or `dnf install` inside a container and have `bkt` capture the changes.

### Current Behavior

```bash
# User installs package via BoxBuddy or directly in container
distrobox enter bootc-dev -- dnf install htop

# bkt capture sees nothing - it only reads distrobox.ini
bkt distrobox capture  # No change detected

# Package is lost on next container rebuild
```

### Solution

Add `--live` or `--packages` flag to capture that introspects running containers:

```bash
bkt distrobox capture --packages bootc-dev
```

### Existing Infrastructure (ready to reuse)

| Component                       | Location                            | Purpose                      |
| ------------------------------- | ----------------------------------- | ---------------------------- |
| `extract_packages_from_image()` | `bkt/src/manifest/build_info.rs:94` | Runs `rpm -qa` in containers |
| `packages` field                | `bkt/src/manifest/distrobox.rs:34`  | Already in schema            |
| `run_command()`                 | `bkt/src/commands/delegation.rs`    | Host command delegation      |

### Implementation Approach

1. Run `distrobox enter CONTAINER -- rpm -qa` → current packages
2. Run `podman run --rm IMAGE rpm -qa` → base image packages
3. Compute diff = user-installed packages
4. Update manifest's `packages` field

### Deliverables

- [x] Add `capture_container_packages(name: &str)` function
- [x] Add `--packages` flag to `bkt distrobox capture`
- [x] Diff against base image to find user-installed packages only
- [x] Update manifest with captured packages

### UX Improvements (Quality Backlog)

- [ ] Better progress output during `bkt distrobox apply` ("this may take a while" → real progress)
- [ ] Condense export output (25 lines → "Exported 25 binaries from ~/.cargo/bin")

---

## Overview

| ID  | Item                                                             | Size | Deps | Status      |
| --- | ---------------------------------------------------------------- | ---- | ---- | ----------- |
| 0   | [Distrobox Live Capture](#-urgent-distrobox-live-capture)        | M    | —    | ✅ Complete |
| 1   | [Extension Enabled State](#1-extension-enabled-state)            | M    | —    | ✅ Complete |
| 2   | [Flatpak Override Capture](#2-flatpak-override-capture)          | M    | —    | ✅ Complete |
| 3   | [Host Package Install](#3-host-package-install)                  | L    | —    | Not Started |
| 4   | [Dev Command Execution](#4-dev-command-execution)                | M    | —    | Not Started |
| 5   | [GSettings Auto-Discovery](#5-gsettings-auto-discovery)          | M    | —    | Not Started |
| 6   | [Upstream Dependency Tracking](#6-upstream-dependency-tracking)  | XL   | —    | Not Started |
| 7   | [Drift Resolution](#7-drift-resolution)                          | M    | 1, 2 | Not Started |
| 8   | [Dev Toolchain Management](#8-dev-toolchain-management)          | L    | 4    | Not Started |
| 9   | [Binary Acquisition Crate](#9-binary-acquisition-crate-fetchbin) | XL   | —    | In Progress |

**Size Legend:** S = Small, M = Medium, L = Large, XL = Extra Large

---

## 9. Binary Acquisition Crate (fetchbin)

**Source:** [RFC-0032](docs/rfcs/0032-binary-acquisition.md)  
**Size:** XL  
**Dependencies:** None  
**Status:** ✅ Complete

### Review Findings (2026-01-27)

Post-implementation review identified critical issues blocking RFC goals.

#### Critical Issues (Must Fix)

| Issue                         | Description                                            | Fix                                       |
| ----------------------------- | ------------------------------------------------------ | ----------------------------------------- |
| **RuntimePool not persisted** | `fetch()` takes `&RuntimePool`, downloads aren't saved | Change to `&mut RuntimePool`              |
| **pnpm bootstrap broken**     | No CLI path sets initial pnpm version                  | Auto-resolve latest on first npm install  |
| **Node not used at runtime**  | Symlinks point to binaries directly                    | Add execution wrappers using managed Node |

#### Major Issues

| Issue              | Description                                      | Fix                              |
| ------------------ | ------------------------------------------------ | -------------------------------- |
| Version comparison | `bkt fetchbin add` only checks name, not version | Compare with semver              |
| Semver ranges      | Version constraints treated as exact strings     | Implement proper semver matching |
| Runtime pruning    | `update()` and `prune()` exist but never called  | Wire into every operation        |
| Checksum gaps      | pnpm/cargo-binstall lack verification            | Add checksum support             |
| Platform support   | Only Linux asset patterns                        | Add macOS/Windows patterns       |

#### Design Decisions Made

| Question         | Decision                                                  |
| ---------------- | --------------------------------------------------------- |
| Platform support | All platforms (Linux, macOS, Windows)                     |
| Version fields   | Semver ranges like Cargo (default `^`, support arbitrary) |
| Runtime pruning  | After every operation (per RFC)                           |

#### Fix Implementation Steps

| Step | Description                             | Size | Status      |
| ---- | --------------------------------------- | ---- | ----------- |
| 9.9  | RuntimePool mutation fix                | S    | ✅ Complete |
| 9.10 | pnpm auto-bootstrap                     | S    | ✅ Complete |
| 9.11 | Semver range support                    | M    | ✅ Complete |
| 9.12 | Runtime pruning on every op             | S    | ✅ Complete |
| 9.13 | macOS/Windows platform patterns         | M    | ✅ Complete |
| 9.14 | Checksum verification for pnpm/binstall | M    | ✅ Complete |
| 9.15 | Node execution wrappers                 | L    | ✅ Complete |

### Questions Resolved by Prepare Agent

These decisions were made during implementation planning:

| Question                  | Resolution                                                       |
| ------------------------- | ---------------------------------------------------------------- |
| GitHub API rate limiting  | Support `GITHUB_TOKEN` env var for authenticated requests        |
| Prerelease handling       | Exclude by default; add `--prerelease` flag later                |
| Checksum failure policy   | Fail hard if checksum exists but mismatches; warn if no checksum |
| Node version caching      | No caching for v1; always fetch fresh index                      |
| Node extraction structure | Keep tarball structure (don't flatten)                           |
| `--allow-scripts` flag    | Defer to future PR; v1 always ignores scripts                    |
| Multiple binaries         | Error with `MultipleBinaries`; add `--binary` flag later         |

### Crate Name Decision Needed

RFC lists candidates: `fetchbin`, `binst`, `binsrc`, `grab`. Scaffold uses `fetchbin` as placeholder.

**Recommendation:** Decide before releasing to avoid renaming.

### Problem

CLI tools distributed via npm (turbo, vercel), cargo (ripgrep), or GitHub releases (lazygit) require their source ecosystems on PATH. Users want binaries without ecosystem clutter.

### Solution

Standalone Rust crate `fetchbin` that:

- Installs binaries from npm, cargo-binstall, GitHub releases
- Manages Node/pnpm invisibly as infrastructure
- Isolates each package (pipx model)
- Tracks versions and checksums in manifest

### Current State

**52 tests passing.** CLI is functional with install, list, update, remove commands.

Verified working:

```bash
fetchbin install github:jesseduffield/lazygit  # ✓ Downloads and extracts
fetchbin list                                   # ✓ Shows installed binaries
fetchbin remove lazygit                         # ✓ Cleans up
```

### Implementation Steps

| Step | Description                                                    | Size | Status      |
| ---- | -------------------------------------------------------------- | ---- | ----------- |
| 9.1  | [GitHubSource Implementation](#91-githubsource-implementation) | M    | ✅ Complete |
| 9.2  | [RuntimePool + Node Download](#92-runtimepool--node-download)  | M    | ✅ Complete |
| 9.3  | [pnpm Bootstrap](#93-pnpm-bootstrap)                           | S    | ✅ Complete |
| 9.4  | [NpmSource Implementation](#94-npmsource-implementation)       | L    | ✅ Complete |
| 9.5  | [cargo-binstall Bootstrap](#95-cargo-binstall-bootstrap)       | S    | ✅ Complete |
| 9.6  | [CargoSource Implementation](#96-cargosource-implementation)   | M    | ✅ Complete |
| 9.7  | [CLI Wiring](#97-cli-wiring)                                   | M    | ✅ Complete |
| 9.8  | [bkt Integration](#98-bkt-integration)                         | L    | ✅ Complete |

### 9.1 GitHubSource Implementation

**Size:** M  
**Dependencies:** None  
**Status:** Ready for implementation

Implement GitHub release asset download:

- Query GitHub Releases API for versions
- Match assets using platform-specific patterns
- Download, verify checksum (if available), extract
- Support tar.gz and zip archives

<details>
<summary><strong>Implementation Plan</strong> (click to expand)</summary>

#### Files to Modify

| File                            | Changes                                                                        |
| ------------------------------- | ------------------------------------------------------------------------------ |
| `fetchbin/src/source/github.rs` | Full `BinarySource` trait implementation                                       |
| `fetchbin/src/error.rs`         | Add `AssetNotFound`, `ChecksumMismatch`, `NoDownloadUrl`, `GitHubApi` variants |
| `fetchbin/src/platform.rs`      | Add `asset_patterns()` and `matches_asset()` methods                           |
| `fetchbin/Cargo.toml`           | Add `tar = "0.4"`                                                              |

#### New Modules

- `fetchbin/src/source/github/api.rs` — GitHub API types (`Release`, `Asset`)
- `fetchbin/src/source/github/archive.rs` — Archive extraction (`extract_tar_gz`, `extract_zip`, `write_raw`)
- `fetchbin/src/source/github/checksum.rs` — Checksum verification

#### Key Functions

```rust
impl GithubSource {
    fn fetch_releases(&self, repo: &str) -> Result<Vec<Release>, FetchError>;
    fn find_asset(&self, release: &Release, pattern: Option<&str>) -> Result<&Asset, FetchError>;
    fn fetch_checksum(&self, release: &Release, asset: &Asset) -> Result<Option<String>, FetchError>;
    fn download_asset(&self, asset: &Asset) -> Result<Vec<u8>, FetchError>;
}
```

#### Test Cases

- `test_parse_release_response` — Deserialize GitHub API JSON
- `test_asset_pattern_matching_linux_x86_64` — Platform pattern matching
- `test_checksum_file_parsing` — Parse various checksum file formats
- `test_archive_type_detection` — `.tar.gz`, `.zip`, raw binary
- `test_extract_tar_gz_single_binary` — Extract binary at root
- `test_extract_zip_single_binary` — Extract from zip

#### Blocking Questions

1. **GitHub API Rate Limiting** — Support `GITHUB_TOKEN` env var? **→ Yes, check and use if present**
2. **Prerelease Handling** — Include by default? **→ No, exclude. Add `--prerelease` flag later**
3. **Multiple Binaries in Archive** — Which to choose? **→ Use `binary_name` from spec, or match repo name**
4. **Checksum Failure Policy** — Fail or warn? **→ Fail if checksum exists but mismatches; warn if no checksum**

</details>

### 9.2 RuntimePool + Node Download

**Size:** M  
**Dependencies:** None  
**Status:** Ready for implementation

Implement Node.js runtime management:

- Download Node from nodejs.org with SHA256 verification
- Store in `~/.local/share/fetchbin/toolchains/node/{version}/`
- Track installed versions in manifest
- Auto-prune unused versions

<details>
<summary><strong>Implementation Plan</strong> (click to expand)</summary>

#### Files to Modify

| File                           | Changes                                                                                          |
| ------------------------------ | ------------------------------------------------------------------------------------------------ |
| `fetchbin/src/runtime/mod.rs`  | Implement `RuntimePool` struct fully                                                             |
| `fetchbin/src/runtime/node.rs` | Implement `NodeVersionIndex`, `download_node()`                                                  |
| `fetchbin/src/error.rs`        | Add `UnsupportedPlatform`, `NoCompatibleNode`, `NodeDownloadFailed`, `ChecksumMismatch` variants |

#### Key Types

```rust
pub struct NodeVersionIndex {
    pub versions: Vec<NodeVersionInfo>,
}

pub struct NodeVersionInfo {
    pub version: String,      // e.g., "22.2.0"
    pub lts: Option<String>,  // e.g., "Jod" or None
    pub date: String,
}

impl RuntimePool {
    pub fn load(data_dir: PathBuf) -> Result<Self, RuntimeError>;
    pub fn get_node(&mut self, requirement: Option<&str>) -> Result<NodeRuntime, RuntimeError>;
    pub fn update(&mut self) -> Result<RuntimeUpdateReport, RuntimeError>;
    pub fn prune(&mut self, used_versions: &HashSet<String>) -> Result<PruneReport, RuntimeError>;
}
```

#### Download Flow

1. Build URL: `https://nodejs.org/dist/v{version}/node-v{version}-{os}-{arch}.tar.gz`
2. Fetch `SHASUMS256.txt` from same directory
3. Download tarball, verify SHA256
4. Extract to `toolchains/node/{version}/`

#### Test Cases

- `test_parse_shasum` — Parse hash from SHASUMS256.txt format
- `test_node_download_url_linux_x64` — Correct URL construction
- `test_version_index_find_compatible` — Semver matching logic
- `test_version_index_prefers_lts` — LTS preference
- `test_get_node_uses_cached` — Returns installed without download
- `test_prune_removes_unused` — Deletes versions not in used set

#### Blocking Questions

1. **Version index caching** — Cache to disk? **→ No caching for v1, always fetch**
2. **Extraction structure** — Flatten or keep tarball structure? **→ Keep tarball structure**
3. **Concurrent downloads** — File locking needed? **→ Defer, assume single-process**

</details>

### 9.3 pnpm Bootstrap

**Size:** S  
**Dependencies:** 9.2

Download pnpm standalone binary:

- Fetch from GitHub releases
- Store in `~/.local/share/fetchbin/toolchains/pnpm/{version}/`
- Verify checksum

### 9.4 NpmSource Implementation

**Size:** L  
**Dependencies:** 9.2, 9.3  
**Status:** Ready for implementation (after 9.2, 9.3)

Implement npm package installation:

- Query npm registry for package metadata
- Check `engines.node` for version requirements
- Install with `--ignore-scripts` by default
- Isolate each package in own node_modules
- Auto-detect binary names from package.json

<details>
<summary><strong>Implementation Plan</strong> (click to expand)</summary>

#### Files to Modify

| File                           | Changes                                                                             |
| ------------------------------ | ----------------------------------------------------------------------------------- |
| `fetchbin/src/source/npm.rs`   | Full `BinarySource` trait implementation                                            |
| `fetchbin/src/error.rs`        | Add `NpmRegistry`, `BinaryNotFound`, `RequiresScripts`, `MultipleBinaries` variants |
| `fetchbin/src/runtime/pnpm.rs` | Implement pnpm acquisition                                                          |

#### Key Types

```rust
struct NpmPackageMetadata {
    name: String,
    dist_tags: DistTags,
    versions: HashMap<String, NpmVersionMetadata>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum BinField {
    Single(String),                    // "bin": "./cli.js"
    Multiple(HashMap<String, String>), // "bin": { "turbo": "./bin/turbo" }
}
```

#### npm Registry API

- Endpoint: `https://registry.npmjs.org/{package}`
- Handle scoped packages: `@scope/pkg` → URL-encode as `@scope%2Fpackage`
- Parse `dist-tags.latest`, `versions[x].engines.node`, `versions[x].bin`

#### Binary Discovery Algorithm

1. If `bin` is a string → binary name = package name
2. If `bin` is an object → keys are binary names
3. If no `bin` field → error

#### Installation Flow

```bash
cd store/npm/{package}/{version}/
echo '{ "dependencies": { "{package}": "{version}" } }' > package.json
pnpm add --ignore-scripts {package}@{version}
ln -s node_modules/{package}/bin/{binary} ./{binary}
```

#### Test Cases

- `parse_single_bin` — Parse `"bin": "./cli.js"`
- `parse_multiple_bins` — Parse `"bin": { "a": "...", "b": "..." }`
- `parse_engines_node` — Extract node version requirement
- `resolve_latest_version` — Mock registry, verify latest tag
- `scoped_package_url` — Verify `@scope/pkg` encodes correctly
- `integrity_verification` — SRI hash parsing and validation

#### Blocking Questions

1. **RuntimePool.get_node() prerequisite** — Must implement 9.2 first
2. **--allow-scripts flag** — Include in v1? **→ No, defer to future PR**
3. **Multiple binary handling** — Prompt or error? **→ Error with `MultipleBinaries`; add `--binary` flag later**

#### Dependencies to Add

```toml
base64 = "0.21"  # For SRI hash decoding (sha512-base64)
```

</details>

### 9.5 cargo-binstall Bootstrap

**Size:** S  
**Dependencies:** 9.1 (uses GitHubSource internally)

Bootstrap cargo-binstall from GitHub releases:

- Download cargo-binstall binary
- Store in toolchains directory
- Self-bootstrap using GitHubSource

### 9.6 CargoSource Implementation

**Size:** M  
**Dependencies:** 9.5

Implement cargo-binstall integration:

- Query crates.io for versions
- Run binstall with isolated CARGO_HOME
- Extract and symlink binaries

### 9.7 CLI Wiring

**Size:** M  
**Dependencies:** 9.1, 9.4, 9.6

Wire CLI commands to implementations:

- `fetchbin install npm:turbo` → NpmSource
- `fetchbin install cargo:ripgrep` → CargoSource
- `fetchbin install github:user/repo` → GitHubSource
- `fetchbin list`, `update`, `remove`

### 9.8 bkt Integration

**Size:** L  
**Dependencies:** 9.7  
**Status:** ✅ Complete

Create `BinarySubsystem` in bkt:

- Implement `Subsystem` trait
- Sync from `host-binaries.json` manifest
- Capture from fetchbin state
- PR workflow integration

**Verified working:**

```bash
bkt fetchbin add npm:turbo      # Add to manifest
bkt fetchbin list               # Show manifest vs installed
bkt fetchbin sync               # Install from manifest
bkt fetchbin capture            # Generate manifest from installed
bkt fetchbin remove turbo       # Remove from manifest
```

<details>
<summary><strong>Implementation Plan</strong> (click to expand)</summary>

#### Files to Create

| File                                | Purpose                    |
| ----------------------------------- | -------------------------- |
| `bkt/src/commands/binary.rs`        | CLI command implementation |
| `bkt/src/manifest/binary.rs`        | Manifest types             |
| `schemas/host-binaries.schema.json` | JSON Schema                |
| `manifests/host-binaries.json`      | Initial empty manifest     |

#### Files to Modify

| File                      | Changes                                   |
| ------------------------- | ----------------------------------------- |
| `bkt/Cargo.toml`          | Add `fetchbin = { path = "../fetchbin" }` |
| `bkt/src/commands/mod.rs` | Add `pub mod binary;`                     |
| `bkt/src/manifest/mod.rs` | Add `pub mod binary;`                     |
| `bkt/src/subsystem.rs`    | Add `BinarySubsystem`, register           |

#### Manifest Format (`manifests/host-binaries.json`)

```json
{
  "binaries": [
    {
      "name": "turbo",
      "source": { "type": "npm", "package": "turbo", "version": "2.3.4" }
    },
    {
      "name": "lazygit",
      "source": {
        "type": "github",
        "repo": "jesseduffield/lazygit",
        "version": "v0.44.0"
      }
    }
  ]
}
```

#### Commands

- `bkt binary add npm:turbo` — Add to manifest, optionally install
- `bkt binary remove turbo` — Remove from manifest
- `bkt binary list` — Show manifest vs installed status
- `bkt binary sync` — Install missing binaries from manifest
- `bkt binary capture` — Generate manifest from fetchbin state

#### Integration Approach

Use library integration (import fetchbin as dependency) for type safety:

```rust
use fetchbin::{Manifest, PackageSpec, source::BinarySource};
```

#### Test Cases

- `test_binary_manifest_roundtrip` — Serialize/deserialize
- `test_binary_sync_plan_detects_missing` — Plan shows install ops
- `test_binary_capture_from_fetchbin` — Read fetchbin state

</details>

---

## 1. Extension Enabled State

**Source:** Gap Analysis  
**Size:** M  
**Dependencies:** None  
**Status:** ✅ Complete

### Problem

Extensions track installation but not enabled/disabled state. When you disable an extension via Extension Manager, `bkt capture` doesn't notice. On next `bkt apply`, disabled extensions get re-enabled.

### Current Behavior

```bash
# User disables extension via Extension Manager
gnome-extensions disable blur-my-shell@aunetx

# bkt capture sees it's still installed, does nothing
bkt extension capture  # No change detected

# bkt apply re-enables it (not what user wanted)
bkt apply
```

### Solution

1. Schema already supports `enabled` field via `ExtensionConfig` object
2. Update capture to query `gnome-extensions list --enabled`
3. Store extensions as objects with `enabled: true/false`
4. Update apply to respect enabled state

### Deliverables

- [ ] Update `bkt extension capture` to detect enabled/disabled state
- [ ] Store captured extensions as `ExtensionConfig` objects (not just UUID strings)
- [ ] Update `bkt extension sync` to enable/disable based on manifest
- [ ] Update `bkt extension enable/disable` to update manifest enabled state
- [ ] Add tests for enabled state round-trip

### Acceptance Criteria

- Disabling extension via Extension Manager → captured as `enabled: false`
- `bkt apply` respects enabled state (doesn't re-enable disabled extensions)
- `bkt extension disable blur-my-shell` updates manifest AND disables on system

---

## 2. Flatpak Override Capture

**Source:** Gap Analysis  
**Size:** M  
**Dependencies:** None  
**Status:** ✅ Complete

### Problem

Flatseal changes (filesystem access, device permissions, environment variables) are stored in `~/.local/share/flatpak/overrides/` but not captured by `bkt`. These changes are lost on image rebuild.

### Current Behavior

```bash
# User grants Discord microphone access via Flatseal
# Creates ~/.local/share/flatpak/overrides/com.discordapp.Discord

# bkt capture sees Discord is installed, but ignores overrides
bkt flatpak capture  # Overrides not captured

# After image rebuild, Flatseal changes are gone
```

### Solution

1. Schema already supports `overrides` field in `FlatpakApp`
2. Read override files from `~/.local/share/flatpak/overrides/`
3. Parse and store in manifest
4. Apply overrides during `bkt flatpak sync`

### Deliverables

- [ ] Add `parse_flatpak_overrides(app_id: &str) -> Option<FlatpakOverrides>`
- [ ] Update `bkt flatpak capture` to include overrides
- [ ] Update `bkt flatpak sync` to write override files
- [ ] Add `bkt flatpak override show <app>` command
- [ ] Add tests for override round-trip

### Acceptance Criteria

- Flatseal permission changes → captured in manifest overrides field
- `bkt apply` restores Flatseal permissions
- Override changes create proper PR diff

---

## 3. Host Package Install

**Source:** RFC-0002, Gap Analysis  
**Size:** L  
**Dependencies:** None  
**Status:** Not Started

### Problem

`bkt dnf install htop` only works in toolbox context. For host packages, users must manually edit the Containerfile, violating the command punning promise.

### Current Behavior

```bash
# In toolbox: works
bkt dnf install htop  # Updates toolbox manifest

# On host: doesn't work as expected
bkt dnf install htop  # Updates system-packages.json but NOT Containerfile
                      # User must manually edit Containerfile
```

### Solution

When running on host (not toolbox):

1. Update `manifests/system-packages.json`
2. Regenerate Containerfile SYSTEM_PACKAGES section
3. Create PR with both changes

### Deliverables

- [ ] Detect host vs toolbox context in `bkt dnf install`
- [ ] For host: update system-packages.json AND Containerfile
- [ ] Integrate with PR workflow (both files in same commit)
- [ ] Add `--containerfile-only` flag to skip manifest (for manual use)
- [ ] Update help text to clarify host vs toolbox behavior

### Acceptance Criteria

- `bkt dnf install htop` (on host) updates manifest + Containerfile + creates PR
- `bkt dnf remove htop` (on host) removes from both
- PR contains atomic commit with both changes

---

## 4. Dev Command Execution

**Source:** RFC-0003, Gap Analysis  
**Size:** M  
**Dependencies:** None  
**Status:** Not Started

### Problem

`bkt dev dnf install gcc` updates `toolbox-packages.json` but doesn't actually execute the install in the toolbox. User must manually run the install.

### Current Behavior

```bash
bkt dev dnf install gcc
# Output: Added gcc to toolbox-packages.json
# But gcc is NOT installed in toolbox!
# User must also run: dnf install gcc
```

### Solution

Execute the package installation in addition to updating the manifest:

1. Run `dnf install` in current toolbox
2. Update `toolbox-packages.json`
3. Optionally create PR

### Deliverables

- [ ] Execute `dnf install` when `bkt dev dnf install` is run
- [ ] Handle install failures gracefully (rollback manifest change?)
- [ ] Add `--manifest-only` flag to skip execution
- [ ] Add `--no-pr` flag to skip PR creation
- [ ] Update error messages for failed installs

### Acceptance Criteria

- `bkt dev dnf install gcc` installs gcc AND updates manifest
- Failed install doesn't corrupt manifest
- `--manifest-only` skips execution (current behavior, for scripting)

---

## 5. GSettings Auto-Discovery

**Source:** Gap Analysis  
**Size:** M  
**Dependencies:** None  
**Status:** Not Started

### Problem

`bkt gsetting capture` requires knowing the exact schema and key. Users can't discover which settings have drifted from defaults.

### Current Behavior

```bash
# User changes font size in Settings app
# Which schema was that? User doesn't know.

# Must guess the schema:
bkt gsetting capture org.gnome.desktop.interface text-scaling-factor

# No way to find all changed settings
```

### Solution

1. Dump current dconf state
2. Compare against baseline (GNOME defaults or saved baseline)
3. Show changed schemas/keys
4. Allow selective capture

### Deliverables

- [ ] Add `bkt gsetting diff` command to show changed settings
- [ ] Create baseline snapshot on first run (`~/.local/share/bkt/gsettings-baseline.txt`)
- [ ] Add `bkt gsetting capture --all-changed` to capture all drifted settings
- [ ] Filter out transient/unimportant schemas (window positions, recent files, etc.)

### Acceptance Criteria

- `bkt gsetting diff` shows settings that differ from baseline
- `bkt gsetting capture --all-changed` captures all meaningful changes
- Baseline can be reset with `bkt gsetting baseline reset`

---

## 6. Upstream Dependency Tracking

**Source:** RFC-0006, Gap Analysis  
**Size:** XL  
**Dependencies:** None  
**Status:** Not Started

### Problem

Themes, icons, fonts, and external tools are not tracked or versioned. Users manually download these, and they're lost on image rebuild.

### Current State

- `upstream/manifest.json` exists but is unused
- `bkt upstream` command exists as stub
- No implementation

### Solution

Full implementation of RFC-0006:

1. `bkt upstream add github:vinceliuice/Colloid-gtk-theme`
2. Pin to specific release/commit
3. SHA256 verification
4. Containerfile generation for downloads
5. Update checking

### Deliverables

- [ ] Implement `bkt upstream add <source>` with GitHub support
- [ ] Implement version pinning (release tag or commit SHA)
- [ ] Implement SHA256 verification
- [ ] Generate Containerfile UPSTREAM section with curl/extract commands
- [ ] Implement `bkt upstream check` for available updates
- [ ] Implement `bkt upstream update <name>` to bump versions
- [ ] Add tests for GitHub API integration

### Acceptance Criteria

- `bkt upstream add github:vinceliuice/Colloid-gtk-theme` pins and downloads
- Theme installed in image at build time
- `bkt upstream check` shows available updates
- SHA256 verification prevents tampered downloads

---

## 7. Drift Resolution

**Source:** RFC-0007, Gap Analysis  
**Size:** M  
**Dependencies:** 1 (Extension Enabled State), 2 (Flatpak Override Capture)  
**Status:** Not Started

### Problem

`bkt drift check` exists (via Python script) but there's no `bkt drift resolve` for interactive resolution. Users must manually decide what to capture vs apply.

### Solution

1. Rewrite drift detection in Rust (using existing manifest types)
2. Add `bkt drift resolve` with interactive prompts
3. For each drift item: capture to manifest, apply from manifest, or skip

### Deliverables

- [ ] Rewrite drift detection in Rust (replace Python script)
- [ ] Implement `bkt drift resolve` with interactive mode
- [ ] Show clear diff for each item (system state vs manifest)
- [ ] Support batch operations (capture all, apply all)
- [ ] Add `--dry-run` flag

### Acceptance Criteria

- `bkt drift resolve` walks through each drifted item
- User can choose: capture, apply, skip for each
- Batch mode: `bkt drift resolve --capture-all`

---

## 8. Dev Toolchain Management

**Source:** RFC-0003, Gap Analysis  
**Size:** L  
**Dependencies:** 4 (Dev Command Execution)  
**Status:** Not Started

### Problem

`bkt dev rustup` and `bkt dev npm` don't exist. Rust toolchains and global npm packages aren't managed declaratively.

### Solution

Extend `bkt dev` with toolchain-specific subcommands:

1. `bkt dev rustup default stable` - Sets default toolchain
2. `bkt dev npm install -g typescript` - Installs global npm package
3. Update toolbox Containerfile accordingly

### Deliverables

- [ ] Implement `bkt dev rustup` subcommand
- [ ] Implement `bkt dev npm` subcommand
- [ ] Store toolchain config in `toolbox-packages.json` or new manifest
- [ ] Generate toolbox Containerfile with toolchain setup
- [ ] Add `bkt dev script add <url>` with SHA256 verification (curl-pipe scripts)

### Acceptance Criteria

- `bkt dev rustup default stable` installs stable Rust and updates manifest
- `bkt dev npm install -g typescript` installs and tracks in manifest
- Toolbox Containerfile includes toolchain setup commands

---

## Dependency Graph

```
┌──────────────────┐
│ 1. Extension     │
│    Enabled State │──────┐
└──────────────────┘      │
                          ▼
┌──────────────────┐   ┌──────────────────┐
│ 2. Flatpak       │──▶│ 7. Drift         │
│    Overrides     │   │    Resolution    │
└──────────────────┘   └──────────────────┘

┌──────────────────┐   ┌──────────────────┐
│ 4. Dev Command   │──▶│ 8. Dev Toolchain │
│    Execution     │   │    Management    │
└──────────────────┘   └──────────────────┘

Independent:
┌──────────────────┐   ┌──────────────────┐   ┌──────────────────┐
│ 3. Host Package  │   │ 5. GSettings     │   │ 6. Upstream      │
│    Install       │   │    Discovery     │   │    Dependencies  │
└──────────────────┘   └──────────────────┘   └──────────────────┘
```

---

## Suggested Implementation Order

### Wave 1: Daily Pain Points (No Dependencies)

Start with items that fix daily workflow friction:

1. **Item 1: Extension Enabled State** (M) — Fixes re-enabling disabled extensions
2. **Item 2: Flatpak Override Capture** (M) — Fixes losing Flatseal changes
3. **Item 3: Host Package Install** (L) — Fixes manual Containerfile editing

### Wave 2: Development Workflow

4. **Item 4: Dev Command Execution** (M) — Fixes `bkt dev dnf` not executing
5. **Item 5: GSettings Auto-Discovery** (M) — Reduces guesswork for settings

### Wave 3: Dependent Features

6. **Item 7: Drift Resolution** (M) — Requires 1, 2 for full fidelity
7. **Item 8: Dev Toolchain Management** (L) — Requires 4 for execution pattern

### Wave 4: Large Feature

8. **Item 6: Upstream Dependency Tracking** (XL) — Independent but large scope

---

## Immediate Follow-ups: Workflow Visibility

These items emerged from investigating why upstream Bazzite updates weren't being detected properly.

### Completed Fixes

- ✅ **BASE_IMAGE mismatch**: Fixed workflow checking wrong image (`bazzite:stable` vs `bazzite-gnome:stable`)
- ✅ **Race condition**: Workflow now pins Containerfile to the exact detected digest

### Near-term Follow-ups

| Item                               | Description                                                                                                                                                     | Reference                                         |
| ---------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------- |
| **Build-info base image section**  | `bkt build-info` should show base image package diffs when upstream changes. Currently queries OCI labels; needs to compare current vs previous image labels.   | [RFC-0013](docs/rfcs/0013-build-descriptions.md)  |
| **`bkt status` visibility**        | `bkt status` should surface: last build date, current vs latest upstream digest, pending changes. Prevents "silent failure" where update loop breaks unnoticed. | —                                                 |
| **Pinned tool update checking**    | Scheduled workflow to run `bkt upstream check` and create PRs for available updates (starship, lazygit, keyd, etc.)                                             | [RFC-0006](docs/rfcs/0006-upstream-management.md) |
| **Release changelog completeness** | GitHub Releases should include full upstream change info when base image updated                                                                                | [RFC-0013](docs/rfcs/0013-build-descriptions.md)  |

### Design Decision: Digest Tracking via OCI Labels

The workflow stores `org.wycats.bootc.base.digest` as an OCI label on each published image. This is the source of truth for "what Bazzite digest did we build against?"

- `upstream/bazzite-stable.digest` file is **documentation only** (not used by workflow)
- `bkt build-info` should query OCI labels from current and previous images to detect base changes
- No git commits needed for digest tracking (avoids permissions complexity)

---

## Deferred to Phase 5+

- Multi-machine sync
- Interactive TUI mode
- `bkt init` command for new distributions
- Plugin system
- Remote management
- Automatic changelog generation (RFC-0005)
- Monitoring via systemd timer

---

## Questions to Resolve

1. **Override format**: Should we use Flatpak's native override format or normalize to JSON?
2. **GSettings baseline**: Ship a baseline with the image, or create on first run?
3. **Toolchain manifests**: Separate `rustup.json`/`npm.json` or extend `toolbox-packages.json`?
4. **Upstream verification**: SHA256 of archive or individual files?

---

## Testing Ideas (Backlog)

### PR Workflow Unit Tests

Add unit tests for the git command construction in `bkt/src/pr.rs`. The recent fix for incorrect `--` placement in `git checkout` commands would have been caught by tests that verify the argument arrays passed to `Command::new("git")`.

Suggested tests:

- `test_branch_creation_args()` — verify `checkout -b <branch>` without errant `--`
- `test_branch_switch_args()` — verify `checkout <branch>` for switching
- `test_git_add_args()` — verify `add -- <path>` (correct `--` usage for pathspecs)

These don't require running git; just assert on the argument arrays.
