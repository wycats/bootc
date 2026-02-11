# RFC 0013: Build Descriptions and Release Changelogs

- Feature Name: `build_descriptions`
- Start Date: 2026-01-15
- RFC PR: (leave this empty until PR is opened)
- Tracking Issue: (leave this empty)

## Summary

Generate a `build-info.json` document that describes what changed between two commits, plus tooling to render it to Markdown and produce a short summary string for OCI labels. The current implementation focuses on diff generation and rendering; publishing to GHCR or release rollups is not implemented.

## Motivation

When `topgrade` pulls the latest bootc image, there's currently no easy, structured way to see what changed. We need a consistent, machine-readable diff that can be rendered for humans and consumed in CI.

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

### Example Output (Rendered)

```markdown
## Upstream Changes

### Base Image: bazzite-stable

**Digest**: `sha256:abc123...` → `sha256:def456...`

| Package             | Previous        | Current         |
| ------------------- | --------------- | --------------- |
| kernel              | 6.12.4-200.fc41 | 6.12.5-201.fc41 |
| mesa-vulkan-drivers | 24.2.3          | 24.2.4          |

## Manifest Changes

### Flatpak Apps

| Change   | App                  | Details         |
| -------- | -------------------- | --------------- |
| ➕ Added | `com.spotify.Client` | Remote: flathub |

### GSettings

| Schema                      | Key          | Previous  | Current       |
| --------------------------- | ------------ | --------- | ------------- |
| org.gnome.desktop.interface | color-scheme | 'default' | 'prefer-dark' |
```

## Guide-level Explanation

### Build Lifecycle

Every push to `main` triggers a container build. Each build can:

1. **Detect changes** from the previous commit
2. **Generate build-info.json** via `bkt build-info generate`
3. **Render Markdown** via `bkt build-info render`
4. **Produce a short summary** via `bkt build-info summary` for OCI labels

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

| Package             | Previous        | Current         |
| ------------------- | --------------- | --------------- |
| kernel              | 6.12.4-200.fc41 | 6.12.5-201.fc41 |
| mesa-vulkan-drivers | 24.2.3          | 24.2.4          |

#### Packages Added

- `new-firmware-1.0.0`

### Upstream Tools

| Tool     | Previous | Current |
| -------- | -------- | ------- |
| lazygit  | v0.56.0  | v0.57.0 |
| starship | v1.23.0  | v1.24.1 |
```

#### 2. Manifest Changes (Explicitly Controlled)

These are changes to our declarative manifests:

| Manifest                | Change Types                                     |
| ----------------------- | ------------------------------------------------ |
| `flatpak-apps.json`     | Added, Removed, Remote Changed                   |
| `flatpak-remotes.json`  | Added, Removed, URL Changed                      |
| `system-packages.json`  | Added, Removed                                   |
| `gnome-extensions.json` | Added, Removed, Version Pinned, Enabled/Disabled |
| `gsettings.json`        | Added, Removed, Value Changed                    |
| `host-shims.json`       | Added, Removed, Command Changed                  |
| `appimage-apps.json`    | Added, Removed                                   |
| `toolbox-packages.json` | Added, Removed                                   |

**Example output**:

```markdown
### Flatpak Apps

| Change            | App                  | Details          |
| ----------------- | -------------------- | ---------------- |
| ➕ Added          | `com.spotify.Client` | Remote: flathub  |
| ➖ Removed        | `org.example.OldApp` |                  |
| 🔄 Remote Changed | `com.example.App`    | fedora → flathub |

### GNOME Extensions

| Change            | Extension              | Details |
| ----------------- | ---------------------- | ------- |
| ➕ Added          | `blur-my-shell@aunetx` |         |
| 📌 Version Pinned | `dash-to-dock@micxgx`  | → v97   |

### GSettings

| Schema                      | Key          | Previous  | Current       |
| --------------------------- | ------------ | --------- | ------------- |
| org.gnome.desktop.interface | color-scheme | 'default' | 'prefer-dark' |
```

#### 3. System Config Changes (Files)

Changes to tracked system configuration files, presented as **semantic diffs** rather than line-based diffs:

| Directory            | Contents              | Diff Format    |
| -------------------- | --------------------- | -------------- |
| `system/keyd/`       | Key remapping         | Binding table  |
| `system/fontconfig/` | Font configuration    | Rule changes   |
| `systemd/`           | User services, timers | Property table |
| `skel/`              | Skeleton files        | File presence  |

**Example output** (keyd config):

```markdown
### system/keyd/default.conf

| Binding  | Previous | Current              |
| -------- | -------- | -------------------- |
| capslock | esc      | overload(nav, esc)   |
| leftmeta | _(none)_ | layer(meta_override) |
```

**Example output** (systemd unit):

```markdown
### systemd/user/bootc-bootstrap.service (Added)

| Property  | Value                  |
| --------- | ---------------------- |
| Type      | oneshot                |
| ExecStart | /usr/bin/bkt bootstrap |
| After     | default.target         |
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

| Component                | Source                          | Verification  |
| ------------------------ | ------------------------------- | ------------- |
| Base Image               | ghcr.io/ublue-os/bazzite-stable | ✅ Sigstore   |
| Flatpak: flathub         | https://flathub.org             | ✅ GPG        |
| lazygit                  | github:jesseduffield/lazygit    | ✅ SHA256     |
| Extension: blur-my-shell | extensions.gnome.org            | ⚠️ Unverified |
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
│     └── (Optional) embed summary as OCI annotation              │
│                                                                 │
│  4. Render outputs                                              │
│     ├── Render build-info.json → Markdown                       │
│     └── Generate summary string for OCI labels                  │
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

# Generate a short summary string (for OCI annotations)
bkt build-info summary build-info.json
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

| Scenario               | Time    |
| ---------------------- | ------- |
| Cold cache (first run) | 2-4 min |
| Upstream changed       | 1-2 min |
| Hot cache (rebuild)    | 1-2 sec |

**Future possibility**: Propose to Universal Blue that they publish package manifests as SBOM attestations, enabling instant zero-pull queries.

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
| Binding  | Previous | Current              |
| -------- | -------- | -------------------- |
| capslock | esc      | overload(nav, esc)   |
| leftmeta | (none)   | layer(meta_override) |
```

**For systemd units**:

```markdown
| Property  | Previous       | Current               |
| --------- | -------------- | --------------------- |
| ExecStart | /usr/bin/foo   | /usr/bin/bar          |
| After     | network.target | network-online.target |
```

**For gsettings-style files**:

```markdown
| Key          | Previous  | Current       |
| ------------ | --------- | ------------- |
| color-scheme | 'default' | 'prefer-dark' |
```

This requires format-specific parsers for each config type, but produces much more readable output than raw diffs.

## Current Gaps

- No GHCR publishing or release rollup commands are implemented.
- `provenance` entries are defined in the schema but not generated yet.
- Build-info rendering is local/CI-only; there is no hosted changelog endpoint.

## Open Questions

(None - all questions resolved)

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
