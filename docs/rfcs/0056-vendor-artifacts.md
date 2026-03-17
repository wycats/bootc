# RFC 0056: Vendor Artifacts

| Status     | Draft                                                    |
| ---------- | -------------------------------------------------------- |
| Created    | 2026-03-12                                               |
| Depends on | RFC-0045 (Layer Minimization), RFC-0050 (Layer Grouping) |

## Problem Statement

External RPM packages like VS Code are currently sourced from vendor YUM repositories via `external-repos.json`. The build downloads packages using `dnf download --enablerepo=<name>`, which depends on the vendor keeping their YUM repo metadata up to date.

In practice, vendors like Microsoft publish new RPM artifacts to their direct download CDN **before** updating their YUM repository metadata. VS Code 1.111.0 was available via the direct download API for days while the YUM repo still only advertised 1.110.1.

This means the current pipeline silently delivers stale packages even when the vendor has already released updates.

## Solution

Introduce a **vendor artifacts** manifest that describes how to discover and install packages directly from vendor release feeds, bypassing stale YUM repository metadata.

### Design Principles

1. **Git tracks intent, not resolved state.** The manifest declares "follow latest stable VS Code." It does not pin a specific version.
2. **Resolution is a build artifact.** A resolver step queries vendor APIs and produces a generated resolution file consumed by the build. This file is not checked into git.
3. **Generic template substitution.** Discovery URLs use `{param}` placeholders filled from manifest-supplied parameters. No vendor-specific logic in the template engine.
4. **Builds are internally consistent.** Resolution happens once per build cycle. The build consumes the resolved artifact, never the live vendor API.

### Manifest: `manifests/vendor-artifacts.json`

Git-tracked intent manifest. Declares what to follow, not what version to use.

```json
{
  "$schema": "../schemas/vendor-artifacts.schema.json",
  "artifacts": [
    {
      "name": "code",
      "display_name": "Visual Studio Code",
      "kind": "rpm",
      "source": {
        "type": "vendor-feed",
        "url": "https://update.code.visualstudio.com/api/update/{platform}/{channel}/latest",
        "params": {
          "channel": "stable"
        },
        "platforms": {
          "x86_64": "linux-rpm-x64",
          "aarch64": "linux-rpm-arm64"
        }
      },
      "layer_group": "independent"
    }
  ]
}
```

**Fields:**

| Field          | Required | Description                                                               |
| -------------- | -------- | ------------------------------------------------------------------------- |
| `name`         | yes      | Unique identifier                                                         |
| `display_name` | yes      | Human-readable name                                                       |
| `kind`         | yes      | Artifact type (`rpm` for now)                                             |
| `source`       | yes      | Discovery specification                                                   |
| `layer_group`  | no       | Deployment layer grouping (`independent` or `bundled`, default `bundled`) |

**Source fields:**

| Field       | Required | Description                                    |
| ----------- | -------- | ---------------------------------------------- |
| `type`      | yes      | Source type (`vendor-feed`)                    |
| `url`       | yes      | URL template with `{param}` placeholders       |
| `params`    | no       | Key-value parameters for template substitution |
| `platforms` | no       | Architecture → platform identifier mapping     |

### Template Substitution

All `{param}` placeholders in `source.url` are expanded from:

1. `source.params` — explicit key-value pairs
2. `platform` — derived from `source.platforms[current_arch]`

**Rules:**

- Missing placeholder parameter → hard error
- Template parameters are simple string substitution
- No vendor-specific logic in the template engine

### Resolution Artifact: `.cache/bkt/vendor-artifacts.resolved.json`

Generated file, not checked into git. Produced by the resolver, consumed by the build.

```json
{
  "resolved_at": "2026-03-12T00:00:00Z",
  "arch": "x86_64",
  "artifacts": [
    {
      "name": "code",
      "kind": "rpm",
      "version": "1.111.0",
      "url": "https://vscode.download.prss.microsoft.com/dbazure/download/stable/ce099c1ed25d9eb3076c11e4a280f3eb52b4fbeb/code-1.111.0-1772846667.el8.x86_64.rpm",
      "sha256": "c64f744ce4091b940c800dda8ba19d3c56c495bde6a100b2d19ae564d0c81bf2",
      "vendor_revision": "ce099c1ed25d9eb3076c11e4a280f3eb52b4fbeb",
      "metadata": {
        "productVersion": "1.111.0",
        "timestamp": 1772846448148
      }
    }
  ]
}
```

### Resolver

The resolver reads `manifests/vendor-artifacts.json`, queries each artifact's source URL, and writes the resolution artifact.

**For `vendor-feed` sources:**

1. Determine current architecture
2. Look up `platform` from `source.platforms[arch]`
3. Combine with `source.params`
4. Expand URL template
5. HTTP GET the expanded URL
6. Parse JSON response (expects `url`, `productVersion`/`name`, `sha256hash`, `version`)
7. Write resolved entry

**Command:** `bkt-build resolve-vendor-artifacts`

Runs inside CI before the Docker build. Writes `.cache/bkt/vendor-artifacts.resolved.json`.

### Build Integration

The Containerfile generator emits stages for vendor artifacts:

```dockerfile
# ── Vendor artifact stages (parallel, each fetches one resolved artifact) ────

FROM base AS vendor-code
COPY .cache/bkt/vendor-artifacts.resolved.json /tmp/vendor-artifacts.resolved.json
RUN bkt-build install-vendor-artifact code

# In final image assembly:
COPY --from=vendor-code / /
```

`bkt-build install-vendor-artifact` reads the resolved manifest, finds the named artifact, downloads the exact URL, verifies SHA256, and installs via `rpm -i --nodb --noscripts --nodeps`.

### CI Workflow

New step in `.github/workflows/build.yml`, after `validate-manifests` and before `build`:

```yaml
- name: Resolve vendor artifacts
  run: |
    ./scripts/bkt-build resolve-vendor-artifacts \
      --manifest manifests/vendor-artifacts.json \
      --output .cache/bkt/vendor-artifacts.resolved.json
```

The resolved file is then available in the Docker build context.

### Migration from `external-repos.json`

When a package moves from `external-repos.json` to `vendor-artifacts.json`:

1. Remove the repo entry from `external-repos.json`
2. Add the artifact entry to `vendor-artifacts.json`
3. Regenerate Containerfile: `bkt containerfile generate`
4. The build now sources the package from the vendor feed instead of the YUM repo

For VS Code specifically:

- Remove the `code` repo from `external-repos.json`
- Keep `code-insiders` in `external-repos.json` (or move it too with `channel: "insider"`)
- Add `code` to `vendor-artifacts.json`

### Image Labels

Resolved vendor artifact metadata is baked into OCI image labels:

```
org.wycats.bootc.vendor-artifact.code.version=1.111.0
org.wycats.bootc.vendor-artifact.code.sha256=c64f...
```

## Implementation Plan

### Phase 1: Manifest and Types

1. Add `VendorArtifactsManifest` types to `bkt/src/manifest/vendor_artifacts.rs`
2. Add shared types to `bkt-common/src/manifest.rs`
3. Register schema in `bkt/src/commands/schema.rs`
4. Create `manifests/vendor-artifacts.json` with VS Code entry

### Phase 2: Resolver

1. Add `resolve-vendor-artifacts` command to `bkt-build`
2. Implement template substitution
3. Implement vendor-feed HTTP resolution
4. Write `.cache/bkt/vendor-artifacts.resolved.json`

### Phase 3: Build Integration

1. Add `install-vendor-artifact` command to `bkt-build`
2. Integrate into Containerfile generator (new emit functions)
3. Update `load_generator_input()` to load vendor artifacts manifest
4. Update CI workflow

### Phase 4: Migration

1. Move VS Code from `external-repos.json` to `vendor-artifacts.json`
2. Regenerate Containerfile
3. Verify build produces correct image

## Alternatives Considered

### Extend `external-repos.json` with direct URL support

- **Con:** Mixes repo-based and direct-download semantics
- **Con:** `baseurl` and `gpg_key` become awkward nullable fields
- **Con:** `bkt-build download-rpms` would need major refactor
- **Verdict:** Wrong abstraction level

### Extend `upstream/manifest.json` with RPM install type

- **Pro:** Reuses existing pinning infrastructure
- **Con:** Upstream model assumes git-tracked pinned versions
- **Con:** RPM install has different semantics than archive/binary
- **Verdict:** Close but pinning model conflicts with "follow latest" goal

### Pin versions in git manifest

- **Pro:** Fully deterministic builds from git alone
- **Con:** Requires git commit for every upstream release
- **Con:** Fundamentally conflicts with project philosophy (git = intent, not upstream freshness)
- **Verdict:** Wrong tradeoff for this project

## Success Criteria

1. VS Code updates appear in builds within hours of vendor release
2. No git commit required for upstream version bumps
3. Builds remain internally consistent (one resolution per build)
4. `bkt containerfile check` passes
5. Migration from `external-repos.json` is clean and reversible
