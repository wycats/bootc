# RFC 0013: Build Descriptions and Release Changelogs

- Feature Name: `build_descriptions`
- Start Date: 2026-01-15
- RFC PR: (leave this empty until PR is opened)
- Tracking Issue: (leave this empty)

## Summary

Automatically generate rich, detailed descriptions for each container image build that show exactly what changed compared to the previous build. These descriptions appear on GHCR package pages and can be aggregated into tagged release changelogs.

## Motivation

When `topgrade` pulls the latest bootc image, there's currently no visibility into what changed. The GHCR package page shows a bare container with no context about its contents or differences from previous builds.

### The Problem

1. **Opacity**: No way to know what's in a build without inspecting it
2. **No diff context**: Can't see what changed between builds
3. **Upstream blindness**: Base image updates are invisible
4. **Missing provenance**: No record of where components came from

### The Vision

Every build should answer:

- What upstream changes are included? (base image packages, tool versions)
- What manifest changes are included? (flatpaks, extensions, settings)
- What system config changed? (keyd, systemd units, etc.)
- Where did everything come from? (provenance and verification status)

### Example: Current vs. Proposed

**Current GHCR Description**: *(empty or minimal)*

**Proposed GHCR Description**:

```markdown
## Upstream Changes

### Base Image: bazzite-stable
**Digest**: `sha256:abc123...` → `sha256:def456...`

| Package | Previous | Current |
|---------|----------|---------|
| kernel | 6.12.4-200.fc41 | 6.12.5-201.fc41 |
| mesa-vulkan-drivers | 24.2.3 | 24.2.4 |

## Manifest Changes

### Flatpak Apps
| Change | App | Details |
|--------|-----|---------|
| ➕ Added | `com.spotify.Client` | Remote: flathub |

### GSettings
| Schema | Key | Previous | Current |
|--------|-----|----------|---------|
| org.gnome.desktop.interface | color-scheme | 'default' | 'prefer-dark' |

## System Config Changes
| File | Change |
|------|--------|
| system/keyd/default.conf | Modified |
```

## Guide-level Explanation

### Build Lifecycle

Every push to `main` triggers a container build. Each build:

1. **Detects changes** from the previous build
2. **Generates a description** in structured markdown
3. **Attaches the description** to the container image
4. **Updates the GHCR package page** via API or OCI annotation

### Change Categories

#### 1. Upstream Changes (Observed, Not Controlled)

These are changes in external dependencies we consume but don't directly manage:

**Base Image Changes**:
- Digest change (always present when base image updates)
- Package additions/removals/updates (via `rpm -qa` diff)
- Kernel version changes

**Pinned Tool Updates** (from `upstream/manifest.json`):
- Version changes: `lazygit v0.56.0 → v0.57.0`
- New pins added
- Pins removed

**Example output**:
```markdown
### Base Image: bazzite-stable
**Digest**: `sha256:abc123...` → `sha256:def456...`

#### Package Updates
| Package | Previous | Current |
|---------|----------|---------|
| kernel | 6.12.4-200.fc41 | 6.12.5-201.fc41 |
| mesa-vulkan-drivers | 24.2.3 | 24.2.4 |

#### Packages Added
- `new-firmware-1.0.0`

### Upstream Tools
| Tool | Previous | Current |
|------|----------|---------|
| lazygit | v0.56.0 | v0.57.0 |
| starship | v1.23.0 | v1.24.1 |
```

#### 2. Manifest Changes (Explicitly Controlled)

These are changes to our declarative manifests:

| Manifest | Change Types |
|----------|--------------|
| `flatpak-apps.json` | Added, Removed, Remote Changed |
| `flatpak-remotes.json` | Added, Removed, URL Changed |
| `system-packages.json` | Added, Removed |
| `gnome-extensions.json` | Added, Removed, Version Pinned, Enabled/Disabled |
| `gsettings.json` | Added, Removed, Value Changed |
| `host-shims.json` | Added, Removed, Command Changed |
| `appimage-apps.json` | Added, Removed |
| `toolbox-packages.json` | Added, Removed |

**Example output**:
```markdown
### Flatpak Apps
| Change | App | Details |
|--------|-----|---------|
| ➕ Added | `com.spotify.Client` | Remote: flathub |
| ➖ Removed | `org.example.OldApp` | |
| 🔄 Remote Changed | `com.example.App` | fedora → flathub |

### GNOME Extensions
| Change | Extension | Details |
|--------|-----------|---------|
| ➕ Added | `blur-my-shell@aunetx` | |
| 📌 Version Pinned | `dash-to-dock@micxgx` | → v97 |

### GSettings
| Schema | Key | Previous | Current |
|--------|-----|----------|---------|
| org.gnome.desktop.interface | color-scheme | 'default' | 'prefer-dark' |
```

#### 3. System Config Changes (Files)

Changes to tracked system configuration files, presented as **semantic diffs** rather than line-based diffs:

| Directory | Contents | Diff Format |
|-----------|----------|-------------|
| `system/keyd/` | Key remapping | Binding table |
| `system/fontconfig/` | Font configuration | Rule changes |
| `systemd/` | User services, timers | Property table |
| `skel/` | Skeleton files | File presence |

**Example output** (keyd config):
```markdown
### system/keyd/default.conf

| Binding | Previous | Current |
|---------|----------|---------|
| capslock | esc | overload(nav, esc) |
| leftmeta | *(none)* | layer(meta_override) |
```

**Example output** (systemd unit):
```markdown
### systemd/user/bootc-bootstrap.service (Added)

| Property | Value |
|----------|-------|
| Type | oneshot |
| ExecStart | /usr/bin/bkt bootstrap |
| After | default.target |
```

For files without semantic parsers, fall back to summary:
```markdown
### system/NetworkManager/conf.d/99-custom.conf (Modified)

2 lines added, 1 line removed
```

#### 4. Provenance Information

Track where components come from and their verification status:

```markdown
## Provenance

| Component | Source | Verification |
|-----------|--------|--------------|
| Base Image | ghcr.io/ublue-os/bazzite-stable | ✅ Sigstore |
| Flatpak: flathub | https://flathub.org | ✅ GPG |
| lazygit | github:jesseduffield/lazygit | ✅ SHA256 |
| Extension: blur-my-shell | extensions.gnome.org | ⚠️ Unverified |
```

### Tagged Release Rollups

Releases come in two forms:

**Weekly calendar releases** (automatic):
```markdown
## Weekly Release v2026.W03

**Period**: 2026-01-13 to 2026-01-19
**Builds included**: 7
**Previous release**: v2026.W02

### Summary
- 3 upstream base image updates
- 5 flatpak changes (4 added, 1 removed)
- 2 extension updates
- 8 system config modifications

### Full Changelog
[Aggregated details from each build...]
```

**Named releases** (manual via `bkt release create`):
```markdown
## Release v2026.01.15-gaming-setup

**Description**: Added gaming-focused apps and optimizations
**Commits included**: 12 (abc123..def456)
**Previous release**: v2026.W02

### Highlights
- Added Steam, Lutris, MangoHud
- Configured gamemode integration
- Updated kernel to 6.12.5 (better AMD support)

### Full Changelog
[Aggregated details...]
```

## Reference-level Explanation

### Data Flow

```
┌─────────────────────────────────────────────────────────────────┐
│                         CI Build                                │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  1. Checkout code                                               │
│                                                                 │
│  2. Detect changes (bkt build-info generate)                    │
│     ├── Compare manifests vs previous commit                    │
│     ├── Compare system/ files vs previous commit                │
│     ├── Fetch base image package list (if digest changed)       │
│     └── Generate build-info.json                                │
│                                                                 │
│  3. Build container                                             │
│     └── Embed build-info.json as OCI annotation or layer        │
│                                                                 │
│  4. Push to GHCR                                                │
│                                                                 │
│  5. Update package description                                  │
│     ├── Render build-info.json → Markdown                       │
│     └── POST to GitHub Packages API                             │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

### Build Info Schema

```json
{
  "$schema": "../schemas/build-info.schema.json",
  "build": {
    "commit": "abc123def",
    "timestamp": "2026-01-15T10:30:00Z",
    "previous_commit": "xyz789abc"
  },
  "upstream": {
    "base_image": {
      "name": "ghcr.io/ublue-os/bazzite-stable",
      "previous_digest": "sha256:abc...",
      "current_digest": "sha256:def...",
      "packages": {
        "added": [...],
        "removed": [...],
        "updated": [
          { "name": "kernel", "from": "6.12.4", "to": "6.12.5" }
        ]
      }
    },
    "tools": {
      "updated": [
        { "name": "lazygit", "from": "v0.56.0", "to": "v0.57.0" }
      ]
    }
  },
  "manifests": {
    "flatpak_apps": {
      "added": [{ "id": "com.spotify.Client", "remote": "flathub" }],
      "removed": [],
      "changed": []
    },
    "gsettings": {
      "added": [],
      "removed": [],
      "changed": [
        {
          "schema": "org.gnome.desktop.interface",
          "key": "color-scheme",
          "from": "default",
          "to": "prefer-dark"
        }
      ]
    }
    // ... other manifests
  },
  "system_config": {
    "added": [],
    "removed": [],
    "modified": [
      {
        "path": "system/keyd/default.conf",
        "diff": "@@ -12,6 +12,7 @@..."
      }
    ]
  },
  "provenance": [
    {
      "component": "base_image",
      "source": "ghcr.io/ublue-os/bazzite-stable",
      "verification": "sigstore"
    }
  ]
}
```

### CLI Commands

```bash
# Generate build info for current state vs previous commit
bkt build-info generate

# Generate comparing specific commits
bkt build-info generate --from abc123 --to def456

# Render build info as markdown
bkt build-info render build-info.json

# Generate and upload to GHCR (CI use)
bkt build-info publish --image ghcr.io/wycats/bootc:sha-abc123
```

### Base Image Package Diffing

**Challenge**: Getting the package list from the base image requires either:
1. Running the image and executing `rpm -qa`
2. Extracting the RPM database from image layers
3. Caching package lists alongside digests

**Proposed approach**:

```bash
# During build, before and after base image fetch
podman run --rm $OLD_IMAGE rpm -qa --queryformat '%{NAME} %{VERSION}-%{RELEASE}\n' > old-packages.txt
podman run --rm $NEW_IMAGE rpm -qa --queryformat '%{NAME} %{VERSION}-%{RELEASE}\n' > new-packages.txt
diff old-packages.txt new-packages.txt
```

The package lists can be cached in a dedicated branch or artifact storage to avoid repeated container runs.

### GHCR Package Description Update

Due to the **512 character limit** on `org.opencontainers.image.description`, we use a two-tier approach:

#### Tier 1: OCI Annotation (Summary)

Embedded at build time, limited to 512 characters:

```dockerfile
LABEL org.opencontainers.image.description="⬆️ kernel 6.12.4→6.12.5, mesa 24.2.3→24.2.4 | ➕ Flatpak: Spotify | 🔧 keyd ⮕ https://github.com/wycats/bootc/releases/tag/sha-abc123"
```

Format: `[emoji] category: changes | ... ⮕ [link]`

Emoji key:
- ⬆️ Upstream updates
- ➕ Added
- ➖ Removed
- 🔄 Changed
- 🔧 Config

#### Tier 2: Release Asset (Full Details)

Uploaded alongside each build:

```bash
gh release create sha-abc123 \
  build-info.md \
  build-info.json \
  --title "Build sha-abc123" \
  --notes-file build-description.md
```

The release page shows the full markdown description with no size limits.

### Rollup Aggregation

For tagged releases, aggregate build-info from multiple commits:

```bash
# Create a release rollup
bkt build-info rollup --from v2026.01.08 --to v2026.01.15

# Output: aggregated changes, deduplicated, with counts
```

Aggregation rules:
- **Additions**: List all added items
- **Removals**: List all removed items
- **Updates**: Show first→last version, skip intermediate
- **Config changes**: Show final diff from start to end

## Design Decisions

### D1: Base Image Package List Acquisition

**Decision**: Cached runtime extraction with digest-keyed artifact caching.

**Research findings**:
- OCI annotations/SBOMs are not published by Bazzite/Universal Blue
- Layer inspection still requires pulling most of the image
- Registry APIs don't expose file contents
- Runtime extraction (`podman run --rm $IMAGE rpm -qa`) is simple and reliable

**Implementation**:

```yaml
# GitHub Actions workflow
- name: Restore cached package list
  id: restore
  uses: actions/cache/restore@v4
  with:
    path: packages-${{ steps.previous.outputs.digest }}.txt
    key: packages-${{ steps.previous.outputs.digest }}

- name: Extract package list (cache miss)
  if: steps.restore.outputs.cache-hit != 'true'
  run: |
    podman run --rm "ghcr.io/ublue-os/bazzite@${{ steps.previous.outputs.digest }}" \
      rpm -qa --qf '%{NAME}\t%{EVR}\n' | sort > packages-${{ steps.previous.outputs.digest }}.txt
    
- name: Save to cache
  if: steps.restore.outputs.cache-hit != 'true'
  uses: actions/cache/save@v4
  with:
    path: packages-${{ steps.previous.outputs.digest }}.txt
    key: packages-${{ steps.previous.outputs.digest }}
```

**Performance characteristics**:

| Scenario | Time |
|----------|------|
| Cold cache (first run) | 2-4 min |
| Upstream changed | 1-2 min |
| Hot cache (rebuild) | 1-2 sec |

**Future possibility**: Propose to Universal Blue that they publish package manifests as SBOM attestations, enabling instant zero-pull queries.

### D2: Description Size Limits

**Decision**: Summary in OCI annotation + link to full release asset.

**Research findings**:
- `org.opencontainers.image.description` is **limited to 512 characters** (text-only)
- This is a fundamental OCI/GHCR limit, not just a UI constraint
- Collapsible sections (`<details>`) won't work in annotations

**Implementation**:

1. **OCI annotation** (512 char limit): One-line summary with link
   ```
   ⬆️ Base: kernel 6.12.4→6.12.5, mesa 24.2.3→24.2.4 | ➕ Flatpak: Spotify | 🔧 keyd config
   Full changelog: https://github.com/wycats/bootc/releases/download/sha-abc123/build-info.md
   ```

2. **Release asset**: Full `build-info.md` with complete details
   - Uploaded as release asset for each build
   - JSON version (`build-info.json`) also available for tooling

3. **GitHub Release body** (for tagged releases): Full markdown, no size limit

### D3: Rollup Trigger Mechanism

**Decision**: Weekly calendar-based + manual named releases.

**Calendar releases** (automatic):
- Run weekly (e.g., Sunday night)
- Named by week: `v2026.W03` (ISO week number)
- Aggregate all builds since last weekly release

**Named releases** (manual):
- Created via `bkt release create v2026.01.15-gaming-setup`
- User provides name and optional description
- Aggregates builds since last release (weekly or named)

### D4: Historical Package Data

**Decision**: Backfill on demand.

When comparing against a build without cached package data:
1. Pull the old image by digest
2. Extract package list
3. Cache for future comparisons
4. Never skip - always provide full details

### D5: Provenance Depth

**Decision**: Direct sources only (for now).

Track immediate sources and their verification status:
- Base image → Sigstore signature status
- Flatpak remotes → GPG verification
- Upstream tools → SHA256 verification

Transitive dependencies are out of scope initially.

### D6: Diff Format for System Config

**Decision**: Semantic diffs, not line-based diffs.

Instead of showing unified diff output, extract and present meaningful changes:

**For keyd config**:
```markdown
| Binding | Previous | Current |
|---------|----------|---------|
| capslock | esc | overload(nav, esc) |
| leftmeta | (none) | layer(meta_override) |
```

**For systemd units**:
```markdown
| Property | Previous | Current |
|----------|----------|---------|
| ExecStart | /usr/bin/foo | /usr/bin/bar |
| After | network.target | network-online.target |
```

**For gsettings-style files**:
```markdown
| Key | Previous | Current |
|-----|----------|---------|
| color-scheme | 'default' | 'prefer-dark' |
```

This requires format-specific parsers for each config type, but produces much more readable output than raw diffs.

## Open Questions

(None - all questions resolved)

## Future Possibilities

### SBOM Integration

Generate Software Bill of Materials (SBOM) for each build, linking to the build description:

```bash
bkt build-info sbom --format spdx
```

### Security Advisory Correlation

Cross-reference package updates with CVE databases:

```markdown
### Security Updates
| Package | CVE | Severity |
|---------|-----|----------|
| openssl | CVE-2026-1234 | High |
```

### Build Comparison UI

Web interface to compare any two builds:

```
https://wycats.github.io/bootc/compare/sha-abc123...sha-def456
```

### Notification Integration

Alert on significant changes:
- Kernel updates
- Security-relevant package updates
- Breaking manifest changes

## Implementation Plan

### Phase 1: Manifest Diffing (Local)

1. Add `bkt build-info generate` command
2. Implement manifest diff logic for all manifest types
3. Generate JSON build info for local testing

### Phase 2: System Config Diffing

1. Add file-based diff detection for `system/`, `systemd/`, `skel/`
2. Generate unified diffs for changed files
3. Integrate into build-info JSON

### Phase 3: CI Integration

1. Add build-info generation step to GitHub Actions
2. Implement markdown rendering
3. Update GHCR description via API

### Phase 4: Base Image Diffing

1. Implement package list extraction
2. Add caching mechanism for package lists
3. Integrate into build-info generation

### Phase 5: Rollup Releases

1. Implement aggregation logic
2. Add `bkt release` commands
3. Create tagged release workflow

## Implementation Notes

### Error Handling for Shallow Clones

CI environments often use shallow clones (`--depth=1`). When git history is unavailable:

1. **Auto-detect**: Check if `git rev-parse HEAD~1` fails
2. **Require explicit commits**: In CI mode, `--from` and `--to` should be required
3. **Helpful error**: "Shallow clone detected. Use `--from <commit> --to <commit>` or fetch full history with `git fetch --unshallow`"

### Summary Truncation for 512-char OCI Limit

When the summary exceeds 512 characters, truncate with priority ordering:

1. Kernel updates (highest priority, always shown)
2. Security-relevant packages
3. Flatpak changes
4. Extension changes
5. Config changes (lowest priority)

Format for overflow: `... and 5 more changes ⮕ [link]`

### Provenance Implementation

Initial implementation marks all sources as verification status based on what we can detect:
- Base image: Check for Sigstore signature
- Flatpak remotes: Check for GPG keys in remote config
- Upstream tools: Check SHA256 in `upstream/manifest.json`
- Extensions: Mark as "Unverified" (no standard verification)

Full provenance chain tracking deferred to future work.

### Schema Versioning

Add `schema_version` field to BuildInfo for forward compatibility:

```json
{
  "schema_version": "1.0.0",
  "build": { ... }
}
```

## Related RFCs

- **RFC-0005**: Changelog and Version Management - Per-PR changelog entries
- **RFC-0006**: Upstream Dependency Management - Pinned tool versions
- **RFC-0007**: Drift Detection - Detecting divergence from manifests
