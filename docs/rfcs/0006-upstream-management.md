# RFC 0006: Upstream Dependency Management

- Feature Name: `upstream_management`
- Start Date: 2025-12-31
- RFC PR: (leave this empty until PR is opened)
- Tracking Issue: (leave this empty)

## Summary

Define how `bkt` manages dependencies on external resources (themes, icons, fonts, extensions) with cryptographic verification, version pinning, and update workflows.

## Motivation

A personal distribution depends on many upstream projects:

- **Themes**: Colloid, adw-gtk3, WhiteSur
- **Icons**: Papirus, WhiteSur, Bibata
- **Fonts**: JetBrains Mono, Inter, Nerd Fonts
- **GNOME Extensions**: Blur my Shell, Dash to Dock

These dependencies need:
1. **Pinning**: Know exactly which version you have
2. **Verification**: Ensure downloads aren't tampered with
3. **Updates**: Easy path to newer versions
4. **Rollback**: Return to previous versions if updates break things

### Current Pain Points

```bash
# Download a theme
curl -L https://github.com/user/theme/archive/v1.0.tar.gz | tar xz

# Months later...
# - What version do I have?
# - Is there an update?
# - Did I verify the download?
```

### The Solution

```bash
# Add upstream dependency
bkt upstream add github:vinceliuice/Colloid-gtk-theme

# Pin to specific version with automatic verification
bkt upstream pin Colloid-gtk-theme v2024-12-01

# Check for updates
bkt upstream check

# Update with verification
bkt upstream update Colloid-gtk-theme
```

## Guide-level Explanation

### Adding Upstream Dependencies

```bash
# From GitHub release
bkt upstream add github:vinceliuice/Colloid-gtk-theme

# From GitHub with specific asset pattern
bkt upstream add github:ful1e5/Bibata_Cursor --asset "Bibata-Modern-Ice.tar.xz"

# From direct URL
bkt upstream add url:https://example.com/resource.tar.gz --name my-resource
```

### Version Pinning

```bash
# Pin to latest release
bkt upstream pin Colloid-gtk-theme latest

# Pin to specific tag
bkt upstream pin Colloid-gtk-theme v2024-12-01

# Pin to specific commit (for repos without releases)
bkt upstream pin some-theme abc123def
```

### Checking and Updating

```bash
# Check all upstreams for updates
bkt upstream check
# Colloid-gtk-theme: v2024-12-01 -> v2025-01-01 (update available)
# Bibata_Cursor: v2.0.6 (up to date)

# Update specific upstream
bkt upstream update Colloid-gtk-theme

# Update all
bkt upstream update --all
```

### Verification Workflow

All upstream resources are verified cryptographically:

```
+-------------------------------------------------------------+
|                    First Add                                |
+-------------------------------------------------------------+
|  1. Download resource                                       |
|  2. Compute SHA256 checksum                                 |
|  3. Record in manifest                                      |
|  4. Optionally verify GPG signature (if available)          |
+-------------------------------------------------------------+

+-------------------------------------------------------------+
|                    Subsequent Builds                        |
+-------------------------------------------------------------+
|  1. Download resource                                       |
|  2. Compute SHA256                                          |
|  3. Compare against manifest                                |
|  4. FAIL if mismatch                                        |
+-------------------------------------------------------------+
```

### Containerfile Variable Strategy

Based on research into Containerfile best practices, `bkt` uses a **hybrid approach** for optimal caching:

#### The Problem

Containerfiles don't have native variable interpolation. Common approaches:

1. **ARG/ENV**: Limited, doesn't work well with `COPY`
2. **Build-time substitution**: Requires preprocessing
3. **External files**: Best for cache-friendly COPYs

#### The Solution

`bkt` generates individual files for each resource, enabling optimal Docker layer caching:

```
upstream/
├── manifest.json              # Source of truth
├── manifest.verified          # SHA256 of verified manifest
├── Colloid-gtk-theme/
│   ├── version               # "v2024-12-01"
│   ├── url                   # Full download URL
│   └── sha256                # Checksum
└── Bibata_Cursor/
    ├── version
    ├── url
    └── sha256
```

This allows the Containerfile to use cache-friendly patterns:

```dockerfile
# Each COPY creates a layer that only invalidates when that file changes
COPY upstream/Colloid-gtk-theme/url /tmp/colloid-url
COPY upstream/Colloid-gtk-theme/sha256 /tmp/colloid-sha256
RUN curl -fsSL "$(cat /tmp/colloid-url)" -o /tmp/theme.tar.gz \
    && echo "$(cat /tmp/colloid-sha256) /tmp/theme.tar.gz" | sha256sum -c \
    && tar xf /tmp/theme.tar.gz -C /usr/share/themes
```

### Verified Manifest Hash

**Problem**: How does CI verify that the manifest checksums are trustworthy?

**Solution**: The `manifest.verified` file contains a SHA256 hash of the manifest after successful local verification:

```bash
# Locally, after adding/updating upstreams:
bkt upstream verify
# 1. Downloads all resources
# 2. Verifies all checksums
# 3. Writes SHA256 of manifest.json to manifest.verified
# 4. Commits both files

# In CI:
# 1. CI reads manifest.verified
# 2. Computes SHA256 of manifest.json
# 3. Compares: if mismatch, build fails
# 4. Uses checksums from manifest.json for downloads
```

This ensures:
- Checksums can't be modified without re-running verification
- CI builds are reproducible
- No need for CI to re-download everything just to verify

### Architecture Handling

Some upstreams have architecture-specific assets:

```json
{
  "name": "some-binary",
  "assets": {
    "x86_64": {
      "url": "https://example.com/binary-amd64.tar.gz",
      "sha256": "abc123..."
    },
    "aarch64": {
      "url": "https://example.com/binary-arm64.tar.gz",
      "sha256": "def456..."
    }
  }
}
```

At build time, the appropriate architecture is selected using `$(uname -m)` or buildx's `TARGETARCH`.

## Reference-level Explanation

### Manifest Structure

```json
// upstream/manifest.json
{
  "upstreams": [
    {
      "name": "Colloid-gtk-theme",
      "source": {
        "type": "github",
        "repo": "vinceliuice/Colloid-gtk-theme",
        "asset_pattern": null
      },
      "pinned": {
        "version": "v2024-12-01",
        "commit": "abc123def456...",
        "url": "https://github.com/.../archive/v2024-12-01.tar.gz",
        "sha256": "deadbeef...",
        "gpg_verified": false,
        "pinned_at": "2025-01-02T10:30:00Z"
      }
    },
    {
      "name": "Bibata_Cursor",
      "source": {
        "type": "github",
        "repo": "ful1e5/Bibata_Cursor",
        "asset_pattern": "Bibata-Modern-Ice.tar.xz"
      },
      "pinned": {
        "version": "v2.0.6",
        "commit": "def456...",
        "url": "https://github.com/.../releases/download/v2.0.6/Bibata-Modern-Ice.tar.xz",
        "sha256": "cafebabe...",
        "gpg_verified": false,
        "pinned_at": "2025-01-02T10:30:00Z"
      }
    }
  ]
}
```

### Individual File Generation

From `manifest.json`, `bkt upstream generate` creates:

```
upstream/Colloid-gtk-theme/version    -> "v2024-12-01"
upstream/Colloid-gtk-theme/url        -> "https://github.com/..."
upstream/Colloid-gtk-theme/sha256     -> "deadbeef..."
```

These files enable Containerfile patterns with optimal caching.

### Update Detection

```bash
bkt upstream check Colloid-gtk-theme
```

1. Query GitHub API for latest release
2. Compare with pinned version
3. Report if update available

### GPG Verification

For upstreams that provide GPG signatures:

```bash
bkt upstream add github:example/project --gpg-key KEYID
```

- Signature verification is attempted when available
- Graceful degradation: warn if signature missing, don't fail
- `gpg_verified` field in manifest records outcome

### Prerelease Handling

By default, `bkt upstream check` ignores prereleases:

```bash
# Include prereleases
bkt upstream check --include-prereleases

# Pin to prerelease explicitly
bkt upstream pin project v2.0.0-beta.1
```

### Fallback Mirrors

For critical dependencies, specify fallback URLs:

```json
{
  "name": "critical-resource",
  "mirrors": [
    "https://primary.example.com/resource.tar.gz",
    "https://mirror.example.com/resource.tar.gz"
  ]
}
```

Build tries each mirror in order.

## Drawbacks

### Maintenance Overhead

Tracking upstreams adds work. Mitigation: `bkt upstream check` makes it easy.

### GitHub API Rate Limits

Frequent checks may hit rate limits. Mitigation: caching and authenticated requests.

## Rationale and Alternatives

### Why Individual Files?

Docker layer caching works best when files are granular. A single `manifest.json` would invalidate the entire layer on any change.

### Alternative: Download in CI

More secure but slower and requires network access during build.

### Alternative: Vendor Everything

Bloats the repository with large binary files.

## Prior Art

- **Nix fetchFromGitHub**: Pinning with hashes
- **Go modules**: Checksum database
- **cargo-vendor**: Vendoring dependencies

## Unresolved Questions

### Q1: Checksum Verification in CI

**Resolution**: Use verified manifest hash approach. Local `bkt upstream verify` writes hash of manifest to `manifest.verified`. CI validates manifest hash before using its checksums.

### Q2: Architecture Interpolation

**Resolution**: Use jq in Containerfile with `$(uname -m)` to select appropriate asset from manifest.

### Q3: Prerelease Handling

**Resolution**: Ignore prereleases by default. Opt-in with `--include-prereleases` flag.

### Q4: GPG Verification

**Resolution**: Attempt GPG verification when available, graceful degradation with warning if signature missing.

### Q5: Fallback URLs

**Resolution**: Defer to future work. Document manual fallback approach.

### Q6: Vendoring

**Resolution**: Defer to future work. Current approach downloads at build time.

## Future Possibilities

- **Automatic Update PRs**: Dependabot-style for upstreams
- **License Tracking**: Record license of each upstream
- **Security Advisories**: Check for known vulnerabilities
- **Local Cache**: Avoid re-downloading during development
