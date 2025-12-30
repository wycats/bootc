# Current Work: Repository Improvements

This document tracks the improvements identified during the codebase review. Work through these items in order; check them off as completed.

---

## Overview

| ID  | Item                                                                | Priority  | Status |
| --- | ------------------------------------------------------------------- | --------- | ------ |
| 1   | [Unified manifest CLI (`bkt`)](#1-unified-manifest-cli-bkt)         | 🔴 High   | 🟡     |
| 2   | [PR automation (`--pr` flag)](#2-pr-automation---pr-flag)           | 🔴 High   | ✅     |
| 3   | [Repository identity metadata](#3-repository-identity-metadata)     | 🔴 High   | ✅     |
| 4   | [System profile rationalization](#4-system-profile-rationalization) | 🔴 High   | ✅     |
| 5   | [Skel management philosophy](#5-skel-management-philosophy)         | 🟡 Medium | ✅     |
| 6   | [JSON schema validation](#6-json-schema-validation)                 | 🟡 Medium | ⬜     |
| 7   | [Toolbox update recipe](#7-toolbox-update-recipe)                   | 🟡 Medium | ✅     |
| 8   | [Machine detection](#8-machine-detection)                           | 🟢 Low    | ⬜     |
| 9   | [Secrets documentation](#9-secrets-documentation)                   | 🟢 Low    | ⬜     |
| 10  | [Structural cleanup](#10-structural-cleanup)                        | 🟢 Low    | ⬜     |
| 11  | [Stale documentation fixes](#11-stale-documentation-fixes)          | 🟢 Low    | ⬜     |

---

## 1. Unified Manifest CLI (`bkt`)

**Status:** 🟡 In progress — `bkt shim` fully implemented, other commands are stubs

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
    │   └── repo.rs         # info/path (partially implemented)
    └── manifest/
        ├── mod.rs
        ├── flatpak.rs      # FlatpakApp, FlatpakRemote with load/save/merge (✅ complete)
        ├── extension.rs    # GnomeExtensionsManifest with load/save/merge (✅ complete)
        ├── gsetting.rs     # GSettingsManifest with load/save/merge (✅ complete)
        └── shim.rs         # Shim struct with load/save/merge (✅ complete)
```

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
4. ⬜ Implement `bkt flatpak` subcommand
5. ⬜ Implement `bkt extension` subcommand
6. ⬜ Implement `bkt gsetting` subcommand
7. ⬜ Implement `bkt skel` subcommand (see #5)
8. ⬜ Implement `bkt profile` subcommand (see #4)
9. ✅ Add CI build step for bkt binary
10. ✅ Update Containerfile to install bkt
11. ⬜ Remove standalone `/usr/bin/shim`

---

## 2. PR Automation (`--pr` flag)

**Status:** ✅ Complete

### Problem

README promises the "apply locally AND open PR" workflow, but it's not implemented:

```bash
# Vision: apply locally AND open PR
shim add --pr nmcli
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

**Status:** ⬜ Not started

### Problem

Manifests reference `$schema` URLs that don't exist:

```json
"$schema": "https://wycats.github.io/bootc/schemas/flatpak-apps.schema.json"
```

### Implementation Plan

1. Create `schemas/` directory in repo
2. Write JSON schemas for each manifest type:
   - `flatpak-apps.schema.json`
   - `flatpak-remotes.schema.json`
   - `gnome-extensions.schema.json`
   - `gsettings.schema.json`
   - `host-shims.schema.json`
3. Add CI step in `.github/workflows/build.yml`:
   ```yaml
   - name: Validate manifests
     run: |
       npm install -g ajv-cli
       ajv validate -s schemas/flatpak-apps.schema.json -d manifests/flatpak-apps.json
       # ... etc
   ```
4. (Optional) Host schemas via GitHub Pages so the `$schema` URLs resolve

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

**Status:** ⬜ Not started

### Problem

Optional features are currently enabled via build ARGs, but there's no runtime detection for machine-specific features.

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

**Status:** ⬜ Not started

### Problem

1Password CLI is installed but there's no documented pattern for using it.

### Question: Is `op read` Sufficient?

Yes, for most cases. The 1Password CLI handles:

- Authentication (biometric, browser, CLI login)
- Secure storage
- Environment variable injection

### Proposed Documentation

Create `docs/SECRETS.md`:

```markdown
# Secrets Management

This image uses 1Password CLI for secrets. Never commit secrets to this repo.

## Setup

1. Install 1Password desktop app (Flatpak)
2. Enable CLI integration in 1Password settings

## Usage

# Read a secret

op read "op://Personal/GitHub Token/credential"

# Inject into environment

export GITHUB_TOKEN=$(op read "op://Personal/GitHub Token/credential")

# Use in scripts

gh auth login --with-token <<< $(op read "op://Personal/GitHub Token/credential")
```

### Implementation Plan

1. Create `docs/SECRETS.md`
2. Add reference in README.md

---

## 10. Structural Cleanup

**Status:** ⬜ Not started

### Tasks

- [ ] Move `PLAN.md` to `docs/PLAN.md`
- [ ] Delete `inventory/` directory (migration artifacts, insights extracted to #4)
- [ ] Delete `system_profile.txt` at root (can be regenerated anytime)
- [ ] Move `journald-logind-policy-notes.txt` to `docs/notes/` if it has lasting value

### Decision: Inventory Files

These are migration artifacts. The insight about per-category outputs has been captured in #4 (System Profile Rationalization). Specifically:

- `host-unowned-executables.txt` → inspired `bkt profile unowned` command
- The category separation → informs future `bkt profile capture --category=flatpaks` option

**Action:** Delete the files; the insight lives in the tooling design.

---

## 11. Stale Documentation Fixes

**Status:** ⬜ Not started

### Tasks

- [ ] Remove "Toolbox Containerfile — currently referenced but not implemented" from PLAN.md (it exists now)
- [ ] Update README to reference `bkt` instead of `shim`
- [ ] Update any other stale references found during implementation

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
