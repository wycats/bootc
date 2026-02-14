# RFC 0042: `bkt-build` Helper Binary and Fully Managed Containerfile

## Status

Draft — reviewed

## Problem

The multi-stage Containerfile (RFC 0041) works, but it has two problems:

1. **Verbose and error-prone.** Each upstream fetch stage is 8-15 lines
   of shell that constructs URLs, downloads, verifies checksums, and
   extracts — all from `jq` queries against the manifest. This pattern
   repeats 7 times with slight variations. The base stage has 30 lines
   of `printf` to write `.repo` files. Every hand-edit is a bug risk
   (e.g., the `DNF_CACHE_EPOCH` silent-ignore bug).

2. **Partially managed.** bkt owns 5 sections via `=== MARKERS ===`, but
   the other ~300 lines are hand-maintained. Any time we edit the
   Containerfile by hand, significant bugs result. The goal is a fully
   managed Containerfile where bkt owns the entire file.

There is also a latent bug: the SYSTEM*PACKAGES section lists external
packages by name (`1password`, `code`, `microsoft-edge-stable`), so
`dnf install` re-downloads them from repos. The pre-downloaded RPMs
in `/tmp/rpms/` from the dl-* stages sit unused. The install line
should be `dnf install -y /tmp/rpms/_.rpm curl distrobox ...` — local
files for external packages, repo resolution for Fedora packages only.

These problems have complementary solutions that reinforce each other.

## Proposal: Two Complementary Tools

### 1. `bkt-build`: A Build-Time Helper Binary

A standalone Rust binary (like `rpmcheck`) that runs **inside** the
Containerfile during `docker build`. It reads the manifests that are
already COPY'd into the build context and replaces verbose shell
pipelines with single commands.

`bkt-build` is a new crate at `bkt-build/` that depends on a shared
`bkt-common` library crate extracted from `fetchbin`. The shared crate
provides HTTP download (`ureq` + rustls), SHA256 verification, and
archive extraction (tar.gz, tar.xz via `lzma-rs`, zip). `bkt-build`
is compiled as a **statically linked musl binary**
(`x86_64-unknown-linux-musl`) for maximum portability across build
environments.

#### Before (fetch-starship, 10 lines)

```dockerfile
FROM base AS fetch-starship
COPY upstream/manifest.json /tmp/upstream-manifest.json
RUN set -eu; \
    tag="$(jq -r '.upstreams[] | select(.name == "starship") | .pinned.version' ...)"; \
    asset="starship-x86_64-unknown-linux-gnu.tar.gz"; \
    curl -fsSL "https://github.com/starship/starship/releases/download/${tag}/${asset}" -o /tmp/${asset}; \
    curl -fsSL "https://github.com/starship/starship/releases/download/${tag}/${asset}.sha256" -o /tmp/${asset}.sha256; \
    expected="$(cat /tmp/${asset}.sha256)"; \
    echo "${expected}  /tmp/${asset}" | sha256sum -c -; \
    tar -xzf /tmp/${asset} -C /usr/bin starship; \
    chmod 0755 /usr/bin/starship
```

#### After (1 meaningful line)

```dockerfile
FROM base AS fetch-starship
COPY --from=tools /bkt-build /usr/local/bin/bkt-build
COPY upstream/manifest.json /tmp/upstream-manifest.json
RUN bkt-build fetch starship
```

#### Commands

| Command                          | What it does                                                     | Replaces                                                              |
| -------------------------------- | ---------------------------------------------------------------- | --------------------------------------------------------------------- |
| `bkt-build fetch <name>`         | Download, verify, and install an upstream entry                  | fetch-starship, fetch-lazygit, fetch-getnf, fetch-bibata, fetch-fonts |
| `bkt-build setup-repos`          | Import GPG keys and write `.repo` files from external-repos.json | 30-line `printf` block in base stage                                  |
| `bkt-build download-rpms <repo>` | `dnf download` packages for a named external repo                | dl-vscode, dl-edge, dl-1password stage bodies                         |

#### How `bkt-build fetch` Works

1. Read `/tmp/upstream-manifest.json` (COPY'd into the build context)
2. Find the entry by `name`
3. Based on `source.type` and `install.type`, dispatch to the right
   fetch/verify/install strategy:

| source.type                        | install.type       | Strategy                                                         |
| ---------------------------------- | ------------------ | ---------------------------------------------------------------- |
| `github` + `asset_pattern`         | `binary`           | Download release asset → sha256 verify → chmod to `install_path` |
| `github` + `asset_pattern`         | `archive`          | Download release asset → sha256 verify → extract to `extract_to` |
| `github` + `release_type: release` | (none/script)      | Download raw file by commit → sha256 verify → chmod              |
| `github` + `release_type: tag`     | `script`           | Git clone at tag → run install script (see bespoke stages below) |
| `url`                              | `binary`/`archive` | Direct URL download → sha256 verify → install                    |

#### How `bkt-build setup-repos` Works

1. Read `/tmp/external-repos.json` (COPY'd from manifests/)
2. For each repo: import GPG key, write `/etc/yum.repos.d/{name}.repo`
3. All configuration derived from manifest — zero hand-written shell

#### How `bkt-build download-rpms` Works

1. Read `/tmp/external-repos.json`
2. Find repo by name, run `dnf download --destdir=/rpms {packages}`
3. Wrapper is thin but ensures the manifest is the single source of truth

#### Delivery Mechanism

`bkt-build` is compiled as a static musl binary and made available as a
separate build stage:

```dockerfile
FROM scratch AS tools
COPY bkt-build /bkt-build
```

Each stage that needs it does `COPY --from=tools /bkt-build /usr/local/bin/bkt-build`.
CI builds `bkt-build` before `docker build` (same pattern as `rpmcheck`)
and injects it into the build context.

#### Relationship to `bkt-common` and `fetchbin`

Core download/verify/extract logic lives in a shared `bkt-common/`
crate, extracted from `fetchbin`. Both `bkt-build` and `fetchbin`
depend on `bkt-common` as a path dependency. The shared crate uses
`ureq` (pure Rust, synchronous) with `rustls` for HTTP — replacing
`reqwest` to keep the dependency tree small (~30 deps vs ~100+) and
avoid OpenSSL complications with musl static linking.

`bkt-common` provides:

- HTTP download (`ureq` + rustls)
- SHA256 verification (`sha2`)
- Archive extraction: tar.gz (`flate2` + `tar`), tar.xz (`lzma-rs` +
  `tar`), zip (`zip`)
- Upstream manifest types (`UpstreamManifest`, `Upstream`,
  `UpstreamSource`, `PinnedVersion`, `InstallConfig`)

`bkt-build` adds the Containerfile-specific commands (`setup-repos`,
`download-rpms`) and the `fetch` dispatch logic on top.

### 2. Fully Managed Containerfile Generation

The second tool extends bkt's existing `bkt containerfile` command to
generate the **entire** Containerfile, not just the marker sections.

#### `bkt containerfile generate`

Produces a complete Containerfile from manifests:

- `manifests/external-repos.json` → base stage repos + dl-\* stages + COPY --from lines
- `upstream/manifest.json` → fetch-_/build-_ stages + COPY --from lines
- `manifests/system-packages.json` → SYSTEM_PACKAGES install section
- `manifests/host-shims.json` → HOST_SHIMS section
- System config manifests → KERNEL_ARGUMENTS, SYSTEMD_UNITS, COPR_REPOS sections
- `Containerfile.d/` fragments → bespoke stages and system configuration (see below)

#### `bkt containerfile check` (extended)

Today this checks marker sections for drift. Extended to diff the entire
generated Containerfile against the committed one and fail CI if they
diverge.

#### Bespoke Stages: Manifest + Fragment Middle Ground

Two current stages have genuinely bespoke build processes:

- **build-keyd**: git clone + `make` + `make install` + manual systemd unit copy
- **fetch-whitesur**: git clone + vendor `./install.sh` script

These should not block full generation. The approach is a **middle
ground** between pure manifest-driven generation and opaque fragments:

1. **The manifest tracks the _what_** — name, version, source, and
   `install.outputs` (what files to `COPY --from` into the final image).
   The generator uses this to emit `COPY --from=build-keyd` lines
   automatically.

2. **The fragment carries only the _build recipe_** — the `RUN` body
   that builds/installs. Not a complete stage definition, just the
   commands. The generator wraps it with the `FROM`, `COPY --from=tools`,
   and manifest-derived context.

This means the generator knows every stage's inputs and outputs from
the manifest. Fragments are minimal — just the build commands.

##### Fragment Structure

```
Containerfile.d/
  build-keyd.run           # Just the RUN body for the build stage
  fetch-whitesur.run       # Just the RUN body for the fetch stage
  90-system-config.tail    # Post-install system configuration
```

##### Manifest `install.outputs` Field

New field on `InstallConfig` entries that describes what files a stage
produces, so the generator can emit `COPY --from` lines:

```json
{
  "name": "keyd",
  "install": {
    "type": "script",
    "command": "make && make PREFIX=/usr FORCE_SYSTEMD=1 install",
    "outputs": [
      "/usr/bin/keyd",
      "/usr/bin/keyd-application-mapper",
      "/usr/lib/systemd/system/keyd.service",
      "/usr/share/keyd/",
      "/usr/share/man/man1/keyd.1.gz",
      "/usr/share/man/man1/keyd-application-mapper.1.gz",
      "/usr/share/doc/keyd/"
    ]
  }
}
```

Each string is a path. For files, it's an exact path; for directories
(trailing `/`), the generator emits `COPY --from=stage /path/ /path/`.
The `src` and `dest` are always identical — the stage installs to the
same paths the final image uses.

##### Generation Order

The generator emits stages in order:

1. `base` stage (repos from external-repos.json)
2. `tools` stage (bkt-build binary)
3. `dl-*` stages (from external-repos.json)
4. `fetch-*` stages (from upstream/manifest.json, `install.type` is `binary` or `archive`)
5. `build-*`/`fetch-*` stages with fragments (from manifest entries with `.run` files in `Containerfile.d/`)
6. `image` stage assembly:
   - COPR repos, kernel args sections
   - COPY --from for RPM downloads
   - SYSTEM_PACKAGES install (Fedora packages + `/tmp/rpms/*.rpm` for externals)
   - SYSTEMD_UNITS section
   - /opt relocation for ostree
   - COPY --from for all upstream outputs (derived from manifest `install` config)
   - System configuration from `Containerfile.d/90-system-config.tail`
   - HOST_SHIMS section
   - RPM version snapshot

#### Example Generated Containerfile (abbreviated)

```dockerfile
# Auto-generated by bkt containerfile generate — DO NOT EDIT
# Re-generate with: bkt containerfile sync
# Source manifests: external-repos.json, upstream/manifest.json,
#                   system-packages.json, host-shims.json

FROM ghcr.io/ublue-os/bazzite-gnome:stable AS base
COPY --from=tools /bkt-build /usr/local/bin/bkt-build
COPY manifests/external-repos.json /tmp/external-repos.json
RUN bkt-build setup-repos

FROM scratch AS tools
COPY bkt-build /bkt-build

FROM base AS dl-vscode
ARG DNF_CACHE_EPOCH=0
COPY --from=tools /bkt-build /usr/local/bin/bkt-build
COPY manifests/external-repos.json /tmp/external-repos.json
RUN bkt-build download-rpms vscode

FROM base AS dl-edge
ARG DNF_CACHE_EPOCH=0
COPY --from=tools /bkt-build /usr/local/bin/bkt-build
COPY manifests/external-repos.json /tmp/external-repos.json
RUN bkt-build download-rpms edge

FROM base AS dl-1password
ARG DNF_CACHE_EPOCH=0
COPY --from=tools /bkt-build /usr/local/bin/bkt-build
COPY manifests/external-repos.json /tmp/external-repos.json
RUN bkt-build download-rpms 1password

FROM base AS fetch-starship
COPY --from=tools /bkt-build /usr/local/bin/bkt-build
COPY upstream/manifest.json /tmp/upstream-manifest.json
RUN bkt-build fetch starship

FROM base AS fetch-lazygit
COPY --from=tools /bkt-build /usr/local/bin/bkt-build
COPY upstream/manifest.json /tmp/upstream-manifest.json
RUN bkt-build fetch lazygit

# ... remaining fetch stages ...

# --- Fragment: Containerfile.d/50-build-keyd.dockerfile ---
FROM base AS build-keyd
# (hand-maintained fragment, spliced verbatim)

# --- Fragment: Containerfile.d/50-fetch-whitesur.dockerfile ---
FROM base AS fetch-whitesur
# (hand-maintained fragment, spliced verbatim)

FROM base AS image
# === COPR_REPOS (managed by bkt) ===
# ...
# === END COPR_REPOS ===

COPY --from=dl-vscode /rpms/ /tmp/rpms/
COPY --from=dl-edge /rpms/ /tmp/rpms/
COPY --from=dl-1password /rpms/ /tmp/rpms/

# === SYSTEM_PACKAGES (managed by bkt) ===
RUN dnf install -y \
    /tmp/rpms/*.rpm \
    curl distrobox fontconfig ...
# === END SYSTEM_PACKAGES ===

COPY --from=fetch-starship /usr/bin/starship /usr/bin/starship
COPY --from=fetch-lazygit /usr/bin/lazygit /usr/bin/lazygit
# ... remaining COPY --from lines (derived from install config) ...

# --- Fragment: Containerfile.d/90-system-config.dockerfile ---
# (system configuration, spliced verbatim)

# === HOST_SHIMS (managed by bkt) ===
# ...
# === END HOST_SHIMS ===

RUN rpm -qa --qf '%{NAME}\t%{EVR}\n' | sort > /usr/share/bootc/rpm-versions.txt
```

## Manifest Changes Required

### `external-repos.json` (enrichment)

Currently (on rpmcheck branch):

```json
{
  "repos": [
    {
      "name": "vscode",
      "baseurl": "https://packages.microsoft.com/yumrepos/vscode",
      "packages": ["code", "code-insiders"]
    }
  ]
}
```

Needs GPG key info for `bkt-build setup-repos`:

```json
{
  "repos": [
    {
      "name": "vscode",
      "baseurl": "https://packages.microsoft.com/yumrepos/vscode",
      "gpg_key": "https://packages.microsoft.com/keys/microsoft.asc",
      "display_name": "Visual Studio Code",
      "packages": ["code", "code-insiders"]
    }
  ]
}
```

### `upstream/manifest.json` (gap filling)

Several entries need changes:

| Entry                    | Current                                 | Needed                                                                                 |
| ------------------------ | --------------------------------------- | -------------------------------------------------------------------------------------- |
| starship                 | `install_path: /usr/local/bin/starship` | Fix to `/usr/bin/starship` (matches actual Containerfile)                              |
| lazygit                  | `install_path: /usr/local/bin/lazygit`  | Fix to `/usr/bin/lazygit` (matches actual Containerfile)                               |
| getnf                    | no `install` block                      | Add `{ "type": "binary", "install_path": "/usr/bin/getnf" }`                           |
| keyd                     | no `install` block                      | Add `{ "type": "script", "outputs": [...] }` — build recipe in fragment                |
| whitesur-icons           | `install.type: archive`                 | Change to `{ "type": "script", "outputs": [...] }` — vendor install script in fragment |
| JetBrains Mono Nerd Font | **no entry**                            | Add full entry with pinned version + sha256 (see below)                                |

#### JetBrains Mono Nerd Font

The `fetch-fonts` stage currently downloads a zip from GitHub with no
version pin, no checksum, and no manifest tracking. This font needs the
**patched** Nerd Font variant (not the symbol-only fallback font or the
base `jetbrains-mono-fonts` Fedora package) because:

- Patched fonts adjust symbol glyph widths to fit the monospace grid
- VTE-based terminals (GNOME Console/Terminal) have unreliable PUA
  codepoint fallback
- starship and lazygit UIs depend on correctly-sized Nerd Font glyphs

Add a new upstream entry:

```json
{
  "name": "jetbrains-mono-nerd-font",
  "description": "JetBrains Mono patched with Nerd Fonts glyphs",
  "source": {
    "type": "github",
    "repo": "ryanoasis/nerd-fonts",
    "asset_pattern": "JetBrainsMono.zip"
  },
  "pinned": {
    "version": "v3.4.0",
    "sha256": "<actual hash>",
    "pinned_at": "2026-02-11T00:00:00Z"
  },
  "install": {
    "type": "archive",
    "extract_to": "/usr/share/fonts/nerd-fonts/JetBrainsMono",
    "strip_components": 0
  }
}
```

This lets `bkt-build fetch jetbrains-mono-nerd-font` handle the download
and `bkt upstream` manage version bumps.

For the bespoke stages (keyd, whitesur), the manifest tracks version,
source, and outputs. The fragment files carry only the build recipe.
The manifest still owns the version pins for `bkt upstream` commands.

### `system-packages.json` (split)

Today all packages are in one list. With the download/install split,
we need to distinguish:

- **External repo packages** — already enumerated in `external-repos.json`
  as `packages` per repo
- **Fedora repo packages** — everything else in `system-packages.json`

The SYSTEM_PACKAGES section generator should:

1. Read both manifests
2. Emit `dnf install -y /tmp/rpms/*.rpm` (for COPY'd external RPMs) +
   the Fedora packages
3. External packages don't need to appear in `system-packages.json` at
   all — they're fully described by `external-repos.json`

This is a data cleanup, not a new manifest. The packages that today
appear in both `system-packages.json` and `external-repos.json.repos[].packages`
should only appear in `external-repos.json`.

## Implementation Plan

Work is organized into PER (Prepare → Execute → Review) cycles. Each
cycle is independently landable as a PR with its own verification.

### PER A: Manifest Foundations

Enriched `external-repos.json` + schema on main. Fix upstream manifest
data (install paths, missing install blocks, JetBrains Mono Nerd Font
entry with pinned version + sha256). Pure JSON/schema work, no Rust.

- **Difficulty**: Easy
- **Dependencies**: None
- **Codebase touches**: `manifests/external-repos.json` (new),
  `schemas/external-repos.schema.json` (new), `upstream/manifest.json`,
  `schemas/upstream-manifest.schema.json` (`outputs` field)
- **Verification**: Schemas validate; install paths match Containerfile;
  `bkt upstream` loads without error

### PER B: Fragment Extraction

Create `Containerfile.d/` with `build-keyd.run`, `fetch-whitesur.run`,
`90-system-config.tail`. Extract from current Containerfile, verify
equivalence. Each contains only the build commands, not full stage defs.

- **Difficulty**: Easy
- **Dependencies**: None
- **Codebase touches**: `Containerfile.d/` (new directory + 3 files)
- **Verification**: Fragment content matches current Containerfile sections

### PER C: SYSTEM_PACKAGES Bug Fix _(folded into PER G1)_

> **Note**: PER C was folded into PER G1 because the `/tmp/rpms/*.rpm`
> install pattern requires multi-stage `download-rpms` stages to exist
> first. Landing PER C independently would remove external packages
> from the `dnf install` line with nothing to replace them.

### PER D: Shared Crate + `bkt-build fetch`

Extract `bkt-common/` shared crate from `fetchbin` (HTTP download,
sha256 verify, archive extraction, manifest types). Create `bkt-build/`
crate depending on `bkt-common`. Implement `bkt-build fetch <name>` —
manifest-driven download from `pinned.url`, sha256 verify, install.
Covers: starship, lazygit, getnf, bibata, JetBrains Mono Nerd Font.

- **Difficulty**: Hard
- **Dependencies**: PER A
- **Codebase touches**: `bkt-common/` (new shared crate extracted from
  fetchbin), `bkt-build/` (new crate), `fetchbin/` (refactored to
  depend on bkt-common), `bkt/` (add bkt-common dependency for
  manifest types)
- **Verification**: `bkt-build fetch starship` inside a container
  downloads, verifies, and installs to `/usr/bin/starship`;
  `fetchbin` tests still pass after refactor

### PER E: `bkt-build` Repo Commands

Add `bkt-build setup-repos` (GPG import + `.repo` file generation from
`external-repos.json`). Add `bkt-build download-rpms <repo>` (thin
wrapper around `dnf download` for a named repo).

- **Difficulty**: Moderate
- **Dependencies**: PER A, PER D
- **Codebase touches**: `bkt-build/src/` (new modules)
- **Verification**: `bkt-build setup-repos` writes correct `.repo`
  files; `bkt-build download-rpms vscode` downloads RPMs

### PER F: CI + Containerfile Rewrite

CI builds musl `bkt-build`, injects into Docker build context as
`FROM scratch AS tools` stage. Rewrite all fetch/download/setup stages
to use `bkt-build` commands (8-15 lines → 2-3 per stage). Verify image
equivalence.

- **Difficulty**: Moderate
- **Dependencies**: PER E
- **Codebase touches**: `.github/workflows/build.yml`, `Containerfile`
- **Verification**: Docker build produces identical image (verify via
  `rpm -qa` diff and binary checksums)

### PER G1: Multi-Stage Containerfile + RPM Fix

Rewrite Containerfile to multi-stage structure: `FROM scratch AS tools`
for bkt-build, `FROM tools AS fetch-*` for upstream fetches,
`FROM tools AS dl-*` for `bkt-build download-rpms`, and `FROM base AS
image` with `COPY --from=` assembly. Fold PER C: update
`generate_system_packages` to emit `/tmp/rpms/*.rpm` + Fedora-only
packages, remove 5 external packages from `system-packages.json`.
Splice `Containerfile.d/` fragments into their respective stages.

- **Difficulty**: Hard
- **Dependencies**: PER B, PER F
- **Codebase touches**: `Containerfile` (full rewrite to multi-stage),
  `manifests/system-packages.json` (remove external packages),
  `bkt/src/containerfile.rs` (`generate_system_packages` fix)
- **Verification**: Docker build produces equivalent image (verify via
  `rpm -qa` diff and binary checksums); external packages installed
  from `/tmp/rpms/*.rpm`; `bkt containerfile check` passes

### PER G2: Full Containerfile Generation

Extend `bkt containerfile generate` to produce the entire file from
manifests + fragments. `bkt containerfile check` diffs generated output
vs. committed file. CI enforces no divergence.

- **Difficulty**: Hard
- **Dependencies**: PER G1
- **Codebase touches**: `bkt/src/containerfile.rs` (major extension),
  `bkt/src/commands/containerfile.rs`, `.github/workflows/build.yml`
- **Verification**: `bkt containerfile generate` output is byte-identical
  to committed Containerfile; CI check exits 0

### PER H: Per-Repo Cache Busting

Generator emits per-repo ARGs (`VSCODE_CACHE_EPOCH`, etc.). CI uses
rpmcheck for per-repo hashes and passes them as build-args. Each dl-\*
stage busts independently.

- **Difficulty**: Moderate
- **Dependencies**: PER G2, rpmcheck (PR #106)
- **Codebase touches**: `bkt/src/containerfile.rs`,
  `.github/workflows/build.yml`
- **Verification**: Changing one repo's version only invalidates that
  repo's dl-\* stage

### Dependency Graph

```
PER A ──→ PER D ──→ PER E ──→ PER F ──→ PER G1 ──→ PER G2 ──→ PER H
                                          ↑                      ↑
PER B ────────────────────────────────────┘                  rpmcheck
                                                            (PR #106)

PER C folded into PER G1
```

### Critical Path

PER A → PER D → PER E → PER F → PER G1 → PER G2 (6 serial cycles, two hard)

### Parallelizable Work

- **PER A + PER B** can run simultaneously (no dependencies)
- **PER B** is fully independent and can land at any time

## Design Decisions

### Static Musl Binary

`bkt-build` is statically linked via `x86_64-unknown-linux-musl`. This
provides portability across build environments without depending on the
container's glibc. Same pattern as `rpmcheck`.

### Shared `bkt-common` Crate

Core download/verify/extract logic and manifest types are extracted
into `bkt-common/`, a shared library crate. Both `fetchbin` and
`bkt-build` depend on it. This avoids duplicating the ~100 lines of
manifest types and the archive/checksum utilities, while keeping each
binary's own dependency footprint minimal.

### `ureq` HTTP Client (Pure Rust)

`bkt-common` uses `ureq` with `rustls` instead of `reqwest` with
OpenSSL. This cuts the dependency tree from ~100+ crates to ~30 and
eliminates the C library complications that make musl static linking
painful. `fetchbin` will migrate from `reqwest` to `ureq` via
`bkt-common` as part of the shared crate extraction.

### `lzma-rs` for tar.xz (Pure Rust)

bibata-cursor ships as `.tar.xz`. `fetchbin` currently doesn't support
this format. `bkt-common` adds tar.xz support via `lzma-rs`, a pure
Rust LZMA implementation that works out of the box with musl (no C
toolchain or `liblzma-dev` headers needed). Acceptable performance
tradeoff for a build tool extracting single archives.

### `pinned.url` for All Entries

`bkt upstream pin` resolves the actual download URL for every entry
and stores it in `pinned.url`. This means `bkt-build fetch` never
calls the GitHub API — it downloads from the stored URL, verifies
sha256, and installs. The glob pattern in `asset_pattern` (e.g.,
`lazygit_*_Linux_x86_64.tar.gz`) is preserved as the _update intent_
for future `bkt upstream pin` runs. `pinned.url` is the _resolved
build input_ for the currently pinned version. Both persist, serving
different audiences.

### Manifest as Single Source of Truth

Every piece of information in the Containerfile must trace back to a
manifest file. If it's not in a manifest, it's in a fragment. Nothing
is "just in the Containerfile." This is what makes full generation
possible and what prevents the hand-edit bugs we keep hitting.

### Manifest + Fragment Middle Ground for Bespoke Stages

Bespoke stages (keyd, whitesur) are not fully opaque fragments. The
manifest tracks their name, version, source, and `install.outputs` —
everything the generator needs to emit `FROM`, `COPY --from`, and
stage wiring. The fragment file carries **only the build recipe** (the
`RUN` body). This means the generator knows every stage's inputs and
outputs from structured data, even for bespoke builds.

### Backward-Compatible Migration

Each unit produces a working Containerfile. The generated output should
be diff-equivalent to the hand-maintained version at each step. This
lets us migrate incrementally and verify each unit independently.

### Fragment Minimality

Fragments contain only the commands that resist schematization — the
build recipe itself. Stage structure (`FROM`, `COPY --from=tools`, ARGs)
and output wiring (`COPY --from` in the image stage) are always
generated from manifest data. A fragment is never a complete stage
definition.

## Relationship to Other RFCs

- **RFC 0041 (Multi-Stage Parallel Downloads)**: This RFC builds on
  0041's architecture. The multi-stage structure is what makes `bkt-build`
  useful — each stage is a self-contained unit that `bkt-build` can
  drive.
- **rpmcheck (PR #106)**: Phase 4 connects the per-repo hashes from
  rpmcheck to the per-repo ARGs in the generated Containerfile.
- **RFC 0006 (Upstream Management)**: `bkt-build fetch` is the build-time
  counterpart to `bkt upstream pin` — one tracks versions, the other
  consumes them during the build.
