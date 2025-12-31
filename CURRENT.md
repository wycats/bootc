# Current Work: Repository Improvements

This document tracks the improvements identified during the codebase review. Work through these items in order; check them off as completed.

---

## Overview

| ID  | Item                                                                         | Priority  | Status      |
| --- | ---------------------------------------------------------------------------- | --------- | ----------- |
| 1   | [Unified manifest CLI (`bkt`)](#1-unified-manifest-cli-bkt)                  | 🔴 High   | ✅          |
| 2   | [PR automation (`--pr` flag)](#2-pr-automation---pr-flag)                    | 🔴 High   | ✅          |
| 3   | [Repository identity metadata](#3-repository-identity-metadata)              | 🔴 High   | ✅          |
| 4   | [System profile rationalization](#4-system-profile-rationalization)          | 🔴 High   | ✅          |
| 5   | [Skel management philosophy](#5-skel-management-philosophy)                  | 🟡 Medium | ✅          |
| 6   | [JSON schema validation](#6-json-schema-validation)                          | 🟡 Medium | ✅          |
| 7   | [Toolbox update recipe](#7-toolbox-update-recipe)                            | 🟡 Medium | ✅          |
| 8   | [Machine detection](#8-machine-detection)                                    | 🟢 Low    | 🔵 Deferred |
| 9   | [Secrets documentation](#9-secrets-documentation)                            | 🟢 Low    | ✅          |
| 10  | [Structural cleanup](#10-structural-cleanup)                                 | 🟢 Low    | ✅          |
| 11  | [Stale documentation fixes](#11-stale-documentation-fixes)                   | 🟢 Low    | ✅          |
| 12  | [Code cleanup: unused BktError](#12-code-cleanup-unused-bkterror)            | 🟢 Low    | ✅          |
| 13  | [Clean up skel placeholder](#13-clean-up-skel-placeholder)                   | 🟢 Low    | ✅          |
| 14  | [Clean up redundant manifest fields](#14-clean-up-redundant-manifest-fields) | 🟢 Low    | ✅          |
| 15  | [Schema hosting](#15-schema-hosting)                                         | 🟡 Medium | ✅          |
| 16  | [CI manifest validation](#16-ci-manifest-validation)                         | 🟡 Medium | ✅          |
| 17  | [Shell completions](#17-shell-completions)                                   | 🟡 Medium | ✅          |
| 18  | [Pre-flight checks](#18-pre-flight-checks)                                   | 🟢 Low    | ✅          |
| 19  | [Status command](#19-status-command)                                         | 🟡 Medium | ✅          |
| 20  | [Colored output](#20-colored-output)                                         | 🟡 Medium | ✅          |
| 21  | [Structured logging (tracing)](#21-structured-logging-tracing)               | 🟡 Medium | ✅          |
| 22  | [Global dry-run support](#22-global-dry-run-support)                         | 🟡 Medium | ✅          |

---

## 1. Unified Manifest CLI (`bkt`)

**Status:** ✅ Complete — all commands implemented with `--pr` support, comprehensive test suite (105 tests)

### Problem

Currently only `shim` has a CLI. Flatpaks, extensions, and gsettings require direct JSON editing.

### Decision: CLI Name

**`bkt`** (bucket) — 3 characters, evokes "collecting things into buckets", no conflicts (`dnf provides bkt` is empty).

### Decision: Implementation Language

**Rust** — Single binary, type-safe manifest handling, clap for CLI, proper error handling, testable.

### Current State

Rust CLI in `bkt/`:

```
bkt/
├── Cargo.toml              # clap, serde, serde_json, anyhow, thiserror, directories, chrono, whoami
├── tests/
│   ├── cli.rs              # 34 integration tests using assert_cmd
│   └── properties.rs       # 11 property-based tests using proptest
└── src/
    ├── main.rs             # CLI entry with clap derive, 7 subcommands
    ├── error.rs            # BktError enum with thiserror
    ├── repo.rs             # RepoConfig struct + find_repo_path()
    ├── pr.rs               # PR automation workflow (✅ complete)
    ├── commands/
    │   ├── mod.rs
    │   ├── flatpak.rs      # add/remove/list/sync (✅ complete with --pr)
    │   ├── shim.rs         # add/remove/list/sync (✅ complete with --pr)
    │   ├── extension.rs    # add/remove/list/sync (✅ complete with --pr)
    │   ├── gsetting.rs     # set/unset/list/apply (✅ complete with --pr)
    │   ├── skel.rs         # add/diff/list/sync (✅ complete with --pr)
    │   ├── profile.rs      # capture/diff/unowned (✅ complete)
    │   └── repo.rs         # info/path (✅ complete)
    └── manifest/
        ├── mod.rs
        ├── flatpak.rs      # FlatpakApp, FlatpakRemote with load/save/merge (✅ complete)
        ├── extension.rs    # GnomeExtensionsManifest with load/save/merge (✅ complete)
        ├── gsetting.rs     # GSettingsManifest with load/save/merge (✅ complete)
        └── shim.rs         # Shim struct with load/save/merge (✅ complete) + 60 unit tests
```

**CI/CD:** GitHub Actions workflow (`.github/workflows/ci.yml`) runs `cargo fmt --check`, `cargo clippy`, `cargo test`, and `cargo build --release` on every push/PR.

**CLI Aliases:**

- `bkt flatpak` (alias: `fp`)
- `bkt extension` (alias: `ext`)
- `bkt gsetting` (alias: `gs`)

### Proposed Interface

```bash
# Flatpaks
bkt flatpak add org.mozilla.firefox
bkt flatpak remove org.mozilla.firefox
bkt flatpak list

# Extensions
bkt extension add dash-to-dock@micxgx.gmail.com
bkt extension remove <uuid>
bkt extension list

# GSettings
bkt gsetting set org.gnome.desktop.interface color-scheme prefer-dark
bkt gsetting unset org.gnome.desktop.interface color-scheme
bkt gsetting list

# Shims (consolidated from standalone `shim` CLI)
bkt shim add nmcli
bkt shim remove nmcli
bkt shim list

# Skel (dotfile management)
bkt skel add .bashrc
bkt skel diff
bkt skel list

# Profile (system state capture — see #4)
bkt profile capture
bkt profile diff
```

### Decision: Consolidate `shim`

The existing `shim` CLI will be consolidated into `bkt shim`. Benefits:

- One command to learn (`bkt`)
- Consistent patterns across all manifest types
- Simpler mental model

### Implementation Plan

1. ✅ Create Rust scaffold with clap CLI structure
2. ✅ Define manifest structs matching existing JSON
3. ✅ Implement `bkt shim` (port from bash `shim` script)
4. ✅ Implement `bkt flatpak` subcommand
5. ✅ Implement `bkt extension` subcommand
6. ✅ Implement `bkt gsetting` subcommand
7. ✅ Implement `bkt skel` subcommand (see #5)
8. ✅ Implement `bkt profile` subcommand (see #4)
9. ✅ Add CI build step for bkt binary
10. ✅ Update Containerfile to install bkt
11. ✅ Remove standalone `/usr/bin/shim`
12. ✅ Add comprehensive test suite (60 unit + 34 integration + 11 property tests)

---

## 2. PR Automation (`--pr` flag)

**Status:** ✅ Complete

### Problem

README promises the "apply locally AND open PR" workflow, but it's not implemented:

```bash
# Vision: apply locally AND open PR
bkt shim add --pr nmcli
```

### Implementation

Created `bkt/src/pr.rs` with complete PR workflow:

```rust
pub struct PrChange {
    pub manifest_type: String,  // "shim", "flatpak", etc.
    pub action: String,         // "add", "remove"
    pub name: String,           // item name
    pub manifest_file: String,  // target file
}

pub fn run_pr_workflow(change: &PrChange, manifest_content: &str) -> Result<()>
```

Workflow steps:

1. Ensure repo exists at `~/.local/share/bootc/source/` (clones if needed)
2. Check `gh auth status`
3. Create branch: `bkt/<type>-<action>-<name>-<timestamp>`
4. Write manifest content
5. Commit with message: `feat(manifests): <action> <type> <name>`
6. Push and create PR via `gh pr create`

Integrated into `bkt shim add --pr` and `bkt shim remove --pr`.

---

## 3. Repository Identity Metadata

**Status:** ✅ Complete

### Problem

The `--pr` workflow needs to know the source repository. This should be baked into the image, not configured per-machine.

### Implementation

Created `/usr/share/bootc/repo.json` with repository metadata:

```json
{
  "owner": "wycats",
  "name": "bootc",
  "url": "https://github.com/wycats/bootc",
  "default_branch": "main"
}
```

Added to Containerfile:

```dockerfile
RUN mkdir -p /usr/share/bootc
COPY repo.json /usr/share/bootc/repo.json
```

`bkt` CLI reads this file to determine where to clone/push for PR workflow.

### Decision: Repo Detection Strategy

Detect existing checkout first, fallback to fixed location:

1. Check if `$PWD` is inside a git repo matching `/usr/share/bootc/repo.json`
2. Fallback to `~/.local/share/bootc/source/`

```bash
find_repo() {
    local expected_url
    expected_url=$(jq -r .url /usr/share/bootc/repo.json)

    # Check current directory
    if git rev-parse --is-inside-work-tree &>/dev/null; then
        local remote_url
        remote_url=$(git remote get-url origin 2>/dev/null || true)
        if [[ "$remote_url" == "$expected_url"* ]]; then
            git rev-parse --show-toplevel
            return 0
        fi
    fi

    # Fallback to fixed location
    echo "${XDG_DATA_HOME:-$HOME/.local/share}/bootc/source"
}
```

---

## 4. System Profile Rationalization

**Status:** ✅ Complete

### Problem

The system profile (`build-system-profile`) and drift detection (`check-drift`) are separate tools that duplicate effort. The profile's role in the workflow isn't clearly articulated.

### Implementation

Created `bkt profile` with three subcommands:

```bash
bkt profile capture              # Capture system state as JSON
bkt profile capture -o file.json # Save to file
bkt profile diff                 # Compare system vs manifests
bkt profile diff -s flatpak      # Diff specific section
bkt profile unowned              # Show files not owned by RPM
bkt profile unowned -d /usr/bin  # Scan specific directory
```

Features:

- `capture`: Gets installed flatpaks, GNOME extensions, enabled extensions
- `diff`: Compares against system manifests, shows missing/extra items
- `unowned`: Uses `rpm -qf` to find unpackaged binaries

---

## 5. Skel Management Philosophy

**Status:** ✅ Complete

### Problem

The current approach copies files to `/etc/skel` but has no mechanism to:

- Sync changes to existing users
- Allow temporary experimentation before baking in
- Submit dotfile changes as PRs

### Philosophy

**Skel is fully managed by the bootc image.** The workflow should be:

1. **Try it:** Edit `~/.bashrc` locally to experiment
2. **Bake it:** Run `bkt skel add .bashrc --pr` to:
   - Copy `~/.bashrc` to `skel/.bashrc` in the repo
   - Open a PR to make it permanent
3. **Apply it:** On next image build + upgrade, new users get the new skel

### Implementation

Created `bkt skel` with four subcommands:

```bash
bkt skel list              # List all files in skel/
bkt skel diff              # Diff all skel files vs $HOME
bkt skel diff .bashrc      # Diff specific file
bkt skel add .bashrc       # Copy from $HOME to skel/
bkt skel add .bashrc --pr  # Same + open PR
bkt skel sync              # Copy skel files to $HOME
bkt skel sync --dry-run    # Preview what would be copied
bkt skel sync --force      # Overwrite existing files
```

Features:

- Lists files from repo's `skel/` directory
- Shows colorized unified diff between skel and $HOME
- Copies files preserving directory structure
- Supports `--pr` flag for PR automation

---

## 6. JSON Schema Validation

**Status:** ✅ Complete

### Problem

Manifests reference `$schema` URLs that don't exist.

### Implementation

Implemented using the `schemars` crate to auto-generate schemas from Rust types:

1. ✅ Added `schemars = "1"` to dependencies with chrono feature
2. ✅ Derived `JsonSchema` on all manifest types:
   - `FlatpakApp`, `FlatpakAppsManifest`, `FlatpakRemote`, `FlatpakRemotesManifest`
   - `GnomeExtensionsManifest`
   - `GSetting`, `GSettingsManifest`
   - `Shim`, `ShimsManifest`
3. ✅ Created `bkt schema` command with subcommands:
   - `bkt schema list` — List available schema types
   - `bkt schema generate` — Output all schemas to stdout
   - `bkt schema generate -o DIR` — Write schemas to directory
4. ✅ Generated schemas to `schemas/` directory:
   - `flatpak-app.schema.json`
   - `flatpak-apps.schema.json`
   - `flatpak-remote.schema.json`
   - `flatpak-remotes.schema.json`
   - `gnome-extensions.schema.json`
   - `gsetting.schema.json`
   - `gsettings.schema.json`
   - `shim.schema.json`
   - `host-shims.schema.json`

### Future Enhancements

- Add CI step to validate manifests against schemas
- Host schemas via GitHub Pages so `$schema` URLs resolve
- Update manifest `$schema` fields to use relative paths

---

## 7. Toolbox Update Recipe

**Status:** ✅ Complete

### Problem

PLAN.md references `ujust toolbox-update` but it doesn't exist.

### Clarification: Does OS upgrade update toolbox?

**No.** The host image and toolbox image are separate:

- `bootc upgrade` updates the host
- Toolbox must be explicitly recreated to pick up new toolbox image

### Implementation

Added to `ujust/60-custom.just`:

```just
# Recreate toolbox with latest image
toolbox-update:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Pulling latest toolbox image..."
    podman pull ghcr.io/wycats/bootc-toolbox:latest
    echo "Removing existing toolbox 'dev'..."
    toolbox rm -f dev || true
    echo "Creating new toolbox 'dev'..."
    toolbox create -i ghcr.io/wycats/bootc-toolbox:latest dev
    echo "✓ Toolbox 'dev' recreated. Run: toolbox enter dev"
```

---

## 8. Machine Detection

**Status:** 🔵 Deferred — implement when a specific use case arises

### Problem

Optional features are currently enabled via build ARGs, but there's no runtime detection for machine-specific features.

### Decision

**Defer implementation.** The current build ARG approach works for known hardware configurations (Asahi, NVIDIA). Implement this when a specific use case arises that requires runtime detection.

### Proposed Solution

Create `/usr/bin/bootc-detect-machine`:

```bash
#!/bin/bash
# Outputs machine capabilities as shell variables

# Apple Silicon detection
if [[ -d /sys/firmware/devicetree/base ]] && grep -q "Apple" /sys/firmware/devicetree/base/compatible 2>/dev/null; then
    echo "BOOTC_APPLE_SILICON=true"
else
    echo "BOOTC_APPLE_SILICON=false"
fi

# NVIDIA GPU detection
if lspci 2>/dev/null | grep -qi nvidia; then
    echo "BOOTC_NVIDIA_GPU=true"
else
    echo "BOOTC_NVIDIA_GPU=false"
fi

# ... etc
```

Usage in ujust:

```just
enable-some-feature:
    #!/usr/bin/env bash
    eval "$(bootc-detect-machine)"
    if [[ "$BOOTC_APPLE_SILICON" == "true" ]]; then
        echo "Enabling Apple Silicon-specific feature..."
    fi
```

### Implementation Plan

1. Create `/usr/bin/bootc-detect-machine` script
2. Add to Containerfile COPY
3. Update relevant ujust recipes to use it

---

## 9. Secrets Documentation

**Status:** ✅ Complete

### Problem

1Password CLI is installed but there's no documented pattern for using it.

### Implementation

Created `docs/SECRETS.md` documenting:

- Prerequisites (1Password desktop app + CLI integration)
- Usage patterns (`op read`, environment injection, scripts)
- Secret reference format (`op://<vault>/<item>/<field>`)
- Troubleshooting common errors

---

## 10. Structural Cleanup

**Status:** ✅ Complete

### Tasks

- [x] Move `PLAN.md` to `docs/PLAN.md`
- [x] Delete `inventory/` directory (migration artifacts, insights extracted to #4)
- [x] Delete `system_profile.txt` at root (can be regenerated anytime)
- [x] Move `journald-logind-policy-notes.txt` to `docs/notes/`

### Decision: Inventory Files

These are migration artifacts. The insight about per-category outputs has been captured in #4 (System Profile Rationalization). Specifically:

- `host-unowned-executables.txt` → inspired `bkt profile unowned` command
- The category separation → informs future `bkt profile capture --category=flatpaks` option

**Action:** Delete the files; the insight lives in the tooling design.

---

## 11. Stale Documentation Fixes

**Status:** ✅ Complete

### Tasks

- [x] Remove "Toolbox Containerfile — currently referenced but not implemented" from PLAN.md (it exists now)
- [x] Update README to reference `bkt` instead of `shim`
- [x] Update PLAN.md link in README (moved to docs/)
- [x] Update repository layout in README to show bkt/ directory

---

## 12. Code Cleanup: Unused BktError

**Status:** ✅ Complete

### Problem

The `bkt/src/error.rs` file defined a `BktError` enum with `thiserror`, but all commands used `anyhow::Result` directly. The custom error type was never used.

### Implementation

Removed the unused code:

1. ✅ Deleted `bkt/src/error.rs`
2. ✅ Removed `mod error;` from `main.rs`
3. ✅ Removed `thiserror` from `Cargo.toml` dependencies

---

## 13. Clean Up Skel Placeholder

**Status:** ✅ Complete

### Problem

The `skel/.bashrc` file was a placeholder with confusing content.

### Implementation

1. ✅ Deleted `skel/.bashrc`
2. ✅ Created `skel/.gitkeep` to preserve directory structure

Users should now use `bkt skel add` to populate real dotfiles.

---

## 14. Clean Up Redundant Manifest Fields

**Status:** ✅ Complete

### Problem

The `manifests/host-shims.json` had entries where `host` field duplicated `name`.

### Implementation

Edited `manifests/host-shims.json` to remove redundant `host` fields. The Rust code's `host_command()` method falls back to `name` when `host` is not specified.

---

## 15. Schema Hosting

**Status:** ✅ Complete

### Problem

Manifests reference `$schema` URLs (e.g., `https://wycats.github.io/bootc/flatpak-apps.schema.json`) that don't actually resolve. The schemas exist locally in `schemas/` but aren't hosted anywhere.

### Implementation

1. ✅ Created `.github/workflows/pages.yml` to deploy schemas via GitHub Pages
2. ✅ Updated manifest `$schema` fields to use hosted URLs:
   - `https://wycats.github.io/bootc/flatpak-apps.schema.json`
   - `https://wycats.github.io/bootc/gnome-extensions.schema.json`
   - etc.
3. ✅ Added `$schema` to `host-shims.json` (was missing)

### Benefits

- Editors (VS Code, etc.) can fetch schemas for autocomplete and validation
- External tools can validate manifests without cloning the repo
- Schema URLs become self-documenting

**Note:** After PR merge, enable GitHub Pages in repository settings (Settings → Pages → Source: GitHub Actions).

---

## 16. CI Manifest Validation

**Status:** ✅ Complete

### Problem

Manifests can contain typos or invalid structures that aren't caught until runtime. The schemas exist but aren't enforced.

### Implementation

Added two new CI jobs in `.github/workflows/ci.yml`:

1. **`validate-manifests`** — Validates JSON syntax and schema compliance:

   - Uses `jq` to check JSON syntax
   - Uses `ajv-cli` to validate against schemas
   - Runs on every PR and push

2. **`check-schemas-current`** — Ensures committed schemas match generated:
   - Builds bkt and runs `bkt schema generate`
   - Compares output with committed `schemas/`
   - Fails if they differ (prevents stale schemas)
3. Fail the build if any manifest is invalid
4. Optionally: validate that `bkt schema generate` output matches committed schemas

### Example CI Step

```yaml
- name: Validate manifests
  run: |
    npx ajv-cli validate -s schemas/flatpak-apps.schema.json -d manifests/flatpak-apps.json
    npx ajv-cli validate -s schemas/gnome-extensions.schema.json -d manifests/gnome-extensions.json
    # ... etc
```

---

## 17. Shell Completions

**Status:** ✅ Complete

### Problem

The `bkt` CLI has many subcommands and options. Shell completions would improve discoverability and reduce typos.

### Implementation

Added shell completion support using `clap_complete` and `clap_complete_nushell`:

1. ✅ Added `clap_complete` and `clap_complete_nushell` to dependencies
2. ✅ Created `bkt completions <shell>` command supporting:
   - `bash` — Bash completion script
   - `zsh` — Zsh completion script
   - `fish` — Fish completion script
   - `nushell` — Nushell extern module

### Usage

```bash
# Generate and install completions
bkt completions bash > ~/.local/share/bash-completion/completions/bkt
bkt completions zsh > ~/.local/share/zsh/site-functions/_bkt
bkt completions fish > ~/.config/fish/completions/bkt.fish
bkt completions nushell > ~/.config/nushell/completions/bkt.nu
```

### Nushell Integration

For nushell, source the completions in `config.nu`:

```nushell
source ~/.config/nushell/completions/bkt.nu
```

---

## 18. Pre-flight Checks

**Status:** ✅ Complete

### Problem

The `--pr` workflow depends on external tools (`gh`, `git`) being properly configured. If `gh auth status` fails or git isn't configured, the workflow fails partway through with a confusing error.

### Implementation

Added comprehensive pre-flight checking in `pr.rs`:

1. ✅ Created `PreflightResult` struct with pass/fail status, message, and fix hints
2. ✅ Added `run_preflight_checks()` that verifies:
   - `gh` CLI is installed
   - `gh auth status` succeeds (user is authenticated)
   - `git` is available
   - `git config user.name` is set
   - `git config user.email` is set
   - `/usr/share/bootc/repo.json` exists
3. ✅ Added `--skip-preflight` flag to all `--pr` commands
4. ✅ Created `bkt doctor` command for standalone system check

### Usage

```bash
# Check system readiness
bkt doctor

# Skip preflight checks (for advanced users)
bkt shim add foo --pr --skip-preflight
```

---

## 19. Status Command

**Status:** ✅ Complete — `bkt status` command implemented with table/JSON output, colored display

### Problem

There's no single command to see an overview of all manifest types and their current state.

### Implementation Plan

Create `bkt status` command that shows:

```bash
$ bkt status
Flatpaks:    28 apps (2 pending sync)
Extensions:  12 installed, 8 enabled
GSettings:   2 configured
Shims:       15 synced
Skel:        3 files (1 differs from $HOME)
```

Features:

- Load all manifests and show counts
- Detect pending syncs (manifest vs. system state)
- Highlight items needing attention
- Support `--json` for scripting

### Code Structure

```rust
// bkt/src/commands/status.rs
pub struct StatusArgs {
    #[arg(long)]
    json: bool,
}

pub fn run(args: StatusArgs) -> Result<()> {
    let flatpaks = FlatpakAppsManifest::merged(...);
    let extensions = GnomeExtensionsManifest::merged(...);
    // ... gather all manifests

    // Compare manifest vs. system state
    // Output summary
}
```

---

## 20. Colored Output

**Status:** ✅ Complete — `owo-colors` integrated in status and effects modules

### Problem

CLI output uses plain text with ✓/✗ symbols but no color. Color improves readability and status recognition.

### Implementation Plan

Add `owo-colors` crate (zero-dependency, modern):

```toml
# Cargo.toml
owo-colors = "4"
```

Apply consistently across all commands:

- Green: success (✓), "Added", "Synced"
- Red: errors (✗), "Failed", "Missing"
- Yellow: warnings, "Skipped", "Already exists"
- Cyan: info, file paths, item names

### Code Pattern

```rust
use owo_colors::OwoColorize;

println!("{} Generated {} shims", "✓".green(), count);
println!("{} Failed to install {}: {}", "✗".red(), app_id, err);
println!("{} Skipping {} (already installed)", "⚠".yellow(), app_id);
```

### Terminal Detection

`owo-colors` auto-detects terminal capabilities. For explicit control:

```rust
// Respect NO_COLOR environment variable
if std::env::var("NO_COLOR").is_ok() {
    owo_colors::set_override(false);
}
```

---

## 21. Structured Logging (tracing)

**Status:** ✅ Complete — `tracing` initialized with `RUST_LOG` env filter support

### Problem

No way to debug issues or see what `bkt` is doing internally. Users report "it didn't work" with no details.

### Implementation Plan

Add `tracing` ecosystem:

```toml
# Cargo.toml
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
```

### Integration

```rust
// main.rs
use tracing_subscriber::{fmt, EnvFilter};

fn main() -> Result<()> {
    // Initialize logging from RUST_LOG env var
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();
    // ...
}
```

### Usage in Commands

```rust
use tracing::{info, debug, warn, error, instrument};

#[instrument(skip(manifest_content))]
pub fn run_pr_workflow(change: &PrChange, manifest_content: &str) -> Result<()> {
    debug!("Starting PR workflow for {:?}", change);

    let repo_path = ensure_repo()?;
    info!(repo = %repo_path.display(), "Using repository");

    // ...

    debug!("Creating branch: {}", branch);
    // ...
}
```

### User Experience

```bash
# Normal usage - no logs
bkt flatpak add org.gnome.Calculator

# Debugging - see what's happening
RUST_LOG=bkt=debug bkt flatpak add org.gnome.Calculator

# Verbose debugging
RUST_LOG=bkt=trace bkt flatpak add org.gnome.Calculator
```

---

## 22. Global Dry-Run Support

**Status:** ✅ Complete — global `-n`/`--dry-run` flag, `Effect` enum, `Executor` struct with dry-run logging

### Problem

Some commands have `--dry-run` but it's inconsistent. Users can't preview changes before applying them.

### Implementation Plan

Add global `--dry-run` flag that works with all commands:

```rust
// main.rs
#[derive(Debug, Parser)]
pub struct Cli {
    /// Show what would be done without making changes
    #[arg(long, short = 'n', global = true)]
    pub dry_run: bool,

    #[command(subcommand)]
    pub command: Commands,
}
```

### Effect System Pattern

Create an `Executor` abstraction that encapsulates all side effects:

```rust
// bkt/src/effects.rs

/// Represents a side effect the CLI can perform
#[derive(Debug, Clone)]
pub enum Effect {
    WriteFile { path: PathBuf, description: String },
    RunCommand { program: String, args: Vec<String>, description: String },
    GitCreateBranch { branch_name: String },
    GitCommit { message: String },
    GitPush,
    CreatePullRequest { title: String, body: String },
}

impl Effect {
    pub fn describe(&self) -> String {
        match self {
            Effect::WriteFile { path, description } => {
                format!("Write {}: {}", path.display(), description)
            }
            Effect::RunCommand { program, args, description } => {
                format!("Run `{} {}`: {}", program, args.join(" "), description)
            }
            // ... etc
        }
    }
}

/// Execution context that tracks and optionally performs effects
pub struct Executor {
    dry_run: bool,
    effects: Vec<Effect>,
}

impl Executor {
    pub fn new(dry_run: bool) -> Self {
        Self { dry_run, effects: Vec::new() }
    }

    pub fn write_file(&mut self, path: &Path, content: &str, desc: &str) -> Result<()> {
        let effect = Effect::WriteFile {
            path: path.to_path_buf(),
            description: desc.to_string(),
        };
        if self.dry_run {
            println!("  Would: {}", effect.describe());
            self.effects.push(effect);
            Ok(())
        } else {
            std::fs::write(path, content)?;
            Ok(())
        }
    }

    pub fn run_command(&mut self, program: &str, args: &[&str], desc: &str) -> Result<bool> {
        let effect = Effect::RunCommand {
            program: program.to_string(),
            args: args.iter().map(|s| s.to_string()).collect(),
            description: desc.to_string(),
        };
        if self.dry_run {
            println!("  Would: {}", effect.describe());
            self.effects.push(effect);
            Ok(true)
        } else {
            let status = std::process::Command::new(program).args(args).status()?;
            Ok(status.success())
        }
    }

    pub fn summarize(&self) {
        if self.dry_run && !self.effects.is_empty() {
            println!("\n[DRY-RUN] {} operations would be performed", self.effects.len());
        }
    }
}
```

### Migration Path

1. Add global `--dry-run` flag
2. Create `Executor` in `src/effects.rs`
3. Refactor commands one at a time, starting with sync operations
4. Add tests that verify dry-run collects effects without executing

### Example Usage

```bash
# Preview what would be installed
bkt --dry-run flatpak sync

# Preview PR creation
bkt --dry-run shim add nmcli --pr

# Preview all changes
bkt -n profile apply
```

---

## Future Considerations

These items are not currently planned but represent potential future directions for the project.

### Machine Detection (Deferred from Item 8)

The vision for machine detection is to enable runtime-conditional behavior based on hardware capabilities. While the current build ARG approach works for known configurations (Asahi, NVIDIA), a runtime detection system would allow:

- Single image that adapts to different hardware
- Automatic feature enablement based on detected capabilities
- Graceful degradation when expected hardware isn't present

This remains deferred until a concrete use case emerges that can't be solved with build-time configuration.

### Interactive TUI Mode

A terminal UI mode for `bkt` could provide:

- Browse and toggle flatpaks/extensions interactively
- Preview diffs before applying changes
- Guided PR creation workflow with prompts
- Real-time sync status visualization

Potential implementation: `ratatui` crate for Rust TUI.

### Plugin System for Custom Manifest Types

Allow users to define custom manifest types without modifying `bkt` source code:

- Plugin discovery from `~/.config/bkt/plugins/` or `/usr/share/bkt/plugins/`
- Simple interface: read manifest → apply changes → write manifest
- Example use cases: custom dconf paths, application-specific configs, system service management

### Multi-Machine Sync

Support managing multiple machines from a single manifest set:

- Machine-specific manifest overrides (e.g., `flatpak-apps.laptop.json`)
- Inheritance/layering of configurations
- Central dashboard showing drift across machines
- Selective sync: "apply this change to all machines" vs "just this one"

This would transform the project from single-machine config management to fleet management.

### Man Page Generation

Generate and install man pages for better discoverability:

- Add `clap_mangen` crate to generate man pages from clap definitions
- Install to `/usr/share/man/man1/bkt.1` and subcommand pages
- Include in Containerfile build

```toml
clap_mangen = "0.2"
```

### `bkt init` Command

Bootstrap new user configuration:

```bash
$ bkt init
Creating ~/.config/bootc/...
  → Created host-shims.json
  → Created gsettings.json
  → Running initial sync...
✓ bkt initialized! Run 'bkt status' to see your configuration.
```

### Validation on Add

Validate items before adding to manifests to prevent typos:

- **Flatpak:** Query remote to verify app exists (`flatpak search`)
- **Extension:** Check extensions.gnome.org API for UUID validity
- **GSettings:** Verify schema exists (`gsettings list-schemas`)

This prevents invalid entries from polluting manifests.

### Backup/Undo System

Before sync operations, create backups for recovery:

```rust
pub fn backup_file(path: &Path) -> Result<PathBuf> {
    let backup_dir = dirs::data_dir()?.join("bkt/backups");
    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
    let backup_path = backup_dir.join(format!("{}-{}", timestamp, path.file_name()));
    fs::copy(path, &backup_path)?;
    Ok(backup_path)
}
```

Add `bkt undo` command to restore from backups.

### Security Hardening

Additional security improvements:

- Specify full paths for external commands (`/usr/bin/flatpak` not `flatpak`)
- Add manifest integrity checks (SHA256 verification)
- Handle concurrent access (lock file when modifying manifests)
- Rate-limit PR creation (prevent accidental spam)

---

## Appendix: When to Use ujust?

This came up during review. Guidelines:

### Use ujust when:

1. **User-facing operations** — things a human runs interactively
2. **Discoverability matters** — `ujust` with no args lists all recipes
3. **Host operations from toolbox** — ujust handles the flatpak-spawn dance
4. **Optional feature toggles** — `ujust enable-*` / `ujust disable-*`

### Use standalone scripts when:

1. **Called by other scripts** — easier to invoke without ujust wrapper
2. **Complex logic** — ujust recipes get unwieldy past ~20 lines
3. **Need to be in PATH** — like `shim`, `check-drift`

### Hybrid approach:

Many tools do both:

- `/usr/bin/check-drift` — the actual script
- `ujust check-drift` — thin wrapper for discoverability

This is the recommended pattern: functionality in a script, discoverability via ujust.
