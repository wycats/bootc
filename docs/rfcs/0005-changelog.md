# RFC 0005: Changelog and Version Management

- Feature Name: `changelog`
- Start Date: 2025-12-31
- RFC PR: (leave this empty until PR is opened)
- Tracking Issue: (leave this empty)

## Summary

Establish a structured approach to tracking changes, versioning images, and maintaining a human-readable changelog for the personal distribution.

## Motivation

A personal distribution evolves over time. Without tracking:
- It's unclear what changed between image builds
- Rollback decisions lack context
- The "why" behind changes is lost

### Goals

1. **Transparency**: Know exactly what's in each image
2. **Rollback Context**: Understand what you're rolling back to
3. **History**: Preserve institutional knowledge about decisions

## Guide-level Explanation

### Automatic Change Tracking

Every `bkt` command that modifies manifests automatically records the change:

```bash
bkt flatpak add org.gnome.Calculator
# Recorded: "Added Flatpak: org.gnome.Calculator"

bkt dnf install htop
# Recorded: "Added package: htop"
```

### Changelog Format

```markdown
# Changelog

All notable changes to this distribution are documented here.

## [2025.01.02.1] - 2025-01-02

### Added
- Flatpak: org.gnome.Calculator
- Package: htop, neovim
- GNOME Extension: Blur my Shell

### Changed
- Updated cursor theme to Bibata-Modern-Ice
- Changed GTK theme to adw-gtk3-dark

### Removed
- Package: nano (replaced by neovim)

## [2025.01.01.1] - 2025-01-01

### Added
- Initial distribution based on Bazzite
- Core Flatpak applications
- Development toolbox configuration
```

### Version Scheme

Format: `YYYY.MM.DD.N`

- `YYYY.MM.DD`: Date of the build
- `N`: Build number for that day (starting at 1)

Examples:
- `2025.01.02.1` - First build on January 2, 2025
- `2025.01.02.2` - Second build on January 2, 2025

**Rationale**: CalVer (Calendar Versioning) naturally communicates age and sequence without implying semantic compatibility.

### PR-based Changelog Entries

Each PR automatically generates a changelog entry:

```yaml
# In PR description (parsed by bkt)
changelog:
  - type: added
    message: "Flatpak: org.gnome.Calculator"
```

When merged, `bkt` extracts and appends to `CHANGELOG.md`.

#### Multi-PR Version Bundling

Multiple PRs merged before a build are grouped into a single version:

```markdown
## [2025.01.02.1] - 2025-01-02

### Added
- Flatpak: org.gnome.Calculator (PR #42)
- Package: htop (PR #43)

### Changed
- Cursor theme update (PR #44)
```

Each entry references its source PR for traceability.

### Commands

```bash
# View recent changes
bkt changelog show

# View changes since last build
bkt changelog pending

# View changes in specific version
bkt changelog show 2025.01.01.1

# Generate changelog entry for current changes
bkt changelog generate
```

## Reference-level Explanation

### Changelog Storage

```
CHANGELOG.md           # Human-readable, git-tracked
.changelog/
├── pending/           # Uncommitted changes
│   ├── 001-flatpak-calculator.yaml
│   └── 002-htop.yaml
└── versions/          # Historical data
    ├── 2025.01.01.1.yaml
    └── 2025.01.02.1.yaml
```

### Change Entry Format

```yaml
# .changelog/pending/001-flatpak-calculator.yaml
timestamp: 2025-01-02T10:30:00Z
type: added
category: flatpak
message: "org.gnome.Calculator"
pr: null  # Set when PR is created
command: "bkt flatpak add org.gnome.Calculator"
```

### Version Metadata

```yaml
# .changelog/versions/2025.01.02.1.yaml
version: 2025.01.02.1
date: 2025-01-02
image_digest: sha256:abc123...
base_image: ghcr.io/ublue-os/bazzite:stable@sha256:def456...
changes:
  - type: added
    category: flatpak
    message: "org.gnome.Calculator"
    pr: 42
```

### Changelog Generation Pipeline

```
+-------------+     +-------------+     +-------------+
|  bkt cmd    |---->|  pending/   |---->|  PR merge   |
|  executed   |     |  *.yaml     |     |  triggers   |
+-------------+     +-------------+     +-------------+
                                               |
                                               v
+-------------+     +-------------+     +-------------+
| CHANGELOG   |<----| versions/   |<----| CI build    |
| .md updated |     |  *.yaml     |     | assigns     |
+-------------+     +-------------+     | version     |
                                        +-------------+
```

### Pruning Strategy

Changelog entries are archived after 6 months:

- `.changelog/versions/` retains YAML files for 6 months
- Older entries remain in `CHANGELOG.md` but YAML metadata is pruned
- Full history available via git

```bash
# Manual pruning (if needed)
bkt changelog prune --before 2024-06-01
```

### GitHub Release Integration

Each image build creates a corresponding GitHub Release:

- Tag: `v2025.01.02.1`
- Release notes: Auto-generated from changelog entries
- Assets: None (images are in container registry)

```bash
# Triggered by CI after successful build
bkt changelog release 2025.01.02.1
# Creates GitHub Release with changelog content
```

### Rollback Context

When using `bootc rollback`, `bkt` can show what you're rolling back to:

```bash
bkt rollback --show
# Current: 2025.01.02.1
# Rolling back to: 2025.01.01.1
#
# You will lose:
#   - Flatpak: org.gnome.Calculator
#   - Package: htop
#
# Proceed? [y/N]
```

**Note**: Rollback awareness is informational. Actual rollback uses `bootc rollback`.

## Drawbacks

### Extra Files

Multiple YAML files add clutter. Mitigation: they're in `.changelog/` subdirectory.

### Manual Changelog Edits

Users might want to edit `CHANGELOG.md` directly. The YAML-based system may conflict.

## Rationale and Alternatives

### Why CalVer?

SemVer implies API compatibility, which doesn't apply to a personal OS.

### Alternative: Git Tags Only

Less discoverable than a changelog file.

### Alternative: Commit Messages Only

Harder to parse and aggregate.

## Prior Art

- **Keep a Changelog**: Format inspiration
- **Conventional Commits**: Structured commit messages
- **Nix Generations**: Numbered system states

## Unresolved Questions

### Q1: Pruning Thresholds

**Resolution**: 6 months retention for YAML, full history in CHANGELOG.md and git.

### Q2: Multi-PR Versions

**Resolution**: Group all PRs merged before a build into one version, reference each PR.

### Q3: GitHub Releases

**Resolution**: Auto-create GitHub Release with each image build, tagged `vYYYY.MM.DD.N`.

### Q4: Rollback Awareness

**Resolution**: Defer full implementation. Provide informational `bkt rollback --show` that displays changes between current and previous versions.

## Future Possibilities

- **Breaking Change Markers**: Flag changes that need user action
- **Release Notes Email**: Send summary to configured email
- **Changelog Website**: Generate static site from changelog
