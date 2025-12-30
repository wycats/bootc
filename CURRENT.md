# Current Work: Repository Improvements

This document tracks the improvements identified during the codebase review. Work through these items in order; check them off as completed.

---

## Overview

| ID  | Item                                                                | Priority  | Status |
| --- | ------------------------------------------------------------------- | --------- | ------ |
| 1   | [Unified manifest CLI (`bkt`)](#1-unified-manifest-cli-bkt)         | 🔴 High   | ⬜     |
| 2   | [PR automation (`--pr` flag)](#2-pr-automation---pr-flag)           | 🔴 High   | ⬜     |
| 3   | [Repository identity metadata](#3-repository-identity-metadata)     | 🔴 High   | ⬜     |
| 4   | [System profile rationalization](#4-system-profile-rationalization) | 🔴 High   | ⬜     |
| 5   | [Skel management philosophy](#5-skel-management-philosophy)         | 🟡 Medium | ⬜     |
| 6   | [JSON schema validation](#6-json-schema-validation)                 | 🟡 Medium | ⬜     |
| 7   | [Toolbox update recipe](#7-toolbox-update-recipe)                   | 🟡 Medium | ⬜     |
| 8   | [Machine detection](#8-machine-detection)                           | 🟢 Low    | ⬜     |
| 9   | [Secrets documentation](#9-secrets-documentation)                   | 🟢 Low    | ⬜     |
| 10  | [Structural cleanup](#10-structural-cleanup)                        | 🟢 Low    | ⬜     |
| 11  | [Stale documentation fixes](#11-stale-documentation-fixes)          | 🟢 Low    | ⬜     |

---

## 1. Unified Manifest CLI (`bkt`)

**Status:** ⬜ Not started

### Problem

Currently only `shim` has a CLI. Flatpaks, extensions, and gsettings require direct JSON editing.

### Decision: CLI Name

**`bkt`** (bucket) — 3 characters, evokes "collecting things into buckets", no conflicts (`dnf provides bkt` is empty).

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

1. Create `/usr/bin/bkt` script with subcommand dispatch
2. Port `shim` logic into `bkt shim` subcommand
3. Add `bkt flatpak` subcommand
4. Add `bkt extension` subcommand
5. Add `bkt gsetting` subcommand
6. Add `bkt skel` subcommand (see #5)
7. Add `bkt profile` subcommand (see #4)
8. Remove standalone `/usr/bin/shim`

---

## 2. PR Automation (`--pr` flag)

**Status:** ⬜ Not started

### Problem

README promises the "apply locally AND open PR" workflow, but it's not implemented:

```bash
# Vision: apply locally AND open PR
shim add --pr nmcli
```

### Proposed Interface

```bash
bkt flatpak add --pr org.mozilla.firefox
bkt shim add --pr nmcli
bkt extension add --pr some-extension@author
```

The `--pr` flag should:

1. Apply the change locally (immediate effect)
2. Clone/update the source repo (see [#3](#3-repository-identity-metadata))
3. Create a feature branch
4. Update the system manifest file
5. Commit with a descriptive message
6. Push and open a PR via `gh pr create`

### Dependencies

- Requires [#3 Repository identity metadata](#3-repository-identity-metadata) to know where to push
- Requires `gh` CLI (already installed)

### Implementation Plan

1. Add `--pr` flag parsing to `bkt` CLI
2. Implement `bkt_pr_workflow()` function:
   - Determine repo path from metadata
   - Create branch: `bkt/<type>-<name>-<timestamp>`
   - Edit appropriate manifest file
   - Commit with message: `feat(manifests): add <type> <name>`
   - Push and create PR
3. Handle error cases (not logged in, no permissions, etc.)

---

## 3. Repository Identity Metadata

**Status:** ⬜ Not started

### Problem

The `--pr` workflow needs to know the source repository. This should be baked into the image, not configured per-machine.

### Proposed Solution

Bake repository metadata into the container image at build time:

```dockerfile
# In Containerfile
LABEL org.wycats.bootc.repo.owner="wycats"
LABEL org.wycats.bootc.repo.name="bootc"
LABEL org.wycats.bootc.repo.url="https://github.com/wycats/bootc"
```

Or create a static file:

```bash
# /usr/share/bootc/repo.json
{
  "owner": "wycats",
  "name": "bootc",
  "url": "https://github.com/wycats/bootc",
  "default_branch": "main"
}
```

### Design Considerations

- **Labels vs file:** Labels are OCI-standard but harder to read from scripts. A JSON file is easier to consume.
- **Recommendation:** Use both — labels for OCI tooling, file for scripts.

### Implementation Plan

1. Add `LABEL` statements to Containerfile
2. Create `/usr/share/bootc/repo.json` during build
3. Update `bkt` CLI to read from `/usr/share/bootc/repo.json`
4. Add helper function: `bkt repo info` to display metadata

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

**Status:** ⬜ Not started

### Problem

The system profile (`build-system-profile`) and drift detection (`check-drift`) are separate tools that duplicate effort. The profile's role in the workflow isn't clearly articulated.

### Insight: Profile as Ground Truth

The system profile is the **ground truth snapshot** — "what actually exists on this machine right now." It's the inverse of manifests:

| Artifact       | Direction        | Purpose                   |
| -------------- | ---------------- | ------------------------- |
| Manifests      | Desired → Actual | "Make reality match this" |
| System Profile | Actual → Record  | "Capture what reality is" |

### Workflow Implications

1. **Migration:** Profile → diff against manifests → add missing items to manifests
2. **Drift detection:** Profile captures actual; compare to manifests; report gaps
3. **Debugging:** "Why is X installed?" Check if it's in manifests or profile-only
4. **Archaeology:** When did this package appear? Git history of profile snapshots

### Proposed Interface

```bash
# Capture current state
bkt profile capture              # outputs JSON to stdout
bkt profile capture -o file.json # saves to file

# Compare to manifests (what check-drift does)
bkt profile diff                 # shows drift

# Check for unowned binaries (new!)
bkt profile unowned              # binaries not from packages
```

### Implementation Plan

1. Refactor `build-system-profile` into `bkt profile capture`
2. Refactor `check-drift` to use profile internally: `bkt profile diff`
3. Add `bkt profile unowned` (inspired by `host-unowned-executables.txt`)
4. Keep `check-drift` as ujust alias for discoverability: `ujust check-drift` → `bkt profile diff`

---

## 5. Skel Management Philosophy

**Status:** ⬜ Not started

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

### Key Insight

We don't want an "escape valve for customizations" — we want a **fast path from experiment to permanent**. The escape valve is just "don't merge the PR yet."

### Proposed Interface

```bash
# Copy a dotfile from $HOME to skel and open PR
bkt skel add .bashrc --pr

# View diff between current $HOME and baked skel
bkt skel diff

# List managed skel files
bkt skel list

# Migrate custom dotfiles to repo (new!)
bkt skel migrate
```

### What About Existing Users?

**Decision:** Automatic sync for new setups, opt-in for existing.

1. **New installs:** `bootc-bootstrap` auto-syncs skel on first login
2. **Existing users:** `bkt skel diff` shows differences; `ujust skel-sync` applies
3. **Migration helper:** `bkt skel migrate` scans for customizations in managed files and helps move them to the repo with PRs

### Implementation Plan

1. Add `bkt skel` subcommand
2. Implement `skel diff` (compare `/etc/skel` to `$HOME`)
3. Implement `skel add <file>` (copy from `$HOME` to repo's `skel/`)
4. Implement `skel migrate` (interactive helper to move customizations)
5. Add `ujust skel-sync` for opt-in sync
6. Update `bootc-bootstrap` to auto-sync for new users
7. Integrate `--pr` flag

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

**Status:** ⬜ Not started

### Problem

PLAN.md references `ujust toolbox-update` but it doesn't exist.

### Clarification: Does OS upgrade update toolbox?

**No.** The host image and toolbox image are separate:

- `bootc upgrade` updates the host
- Toolbox must be explicitly recreated to pick up new toolbox image

### Proposed Recipe

Add to `ujust/60-custom.just`:

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

### Implementation Plan

1. Add recipe to `ujust/60-custom.just`
2. Test from host and from inside toolbox (should work from both via flatpak-spawn)

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
