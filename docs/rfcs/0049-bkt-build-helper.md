# RFC 0049: `bkt-build` Helper Binary

## Status

Draft

## Summary

A standalone Rust binary that runs **inside** the Containerfile during
`docker build`. It reads manifests COPY'd into the build context and
replaces verbose shell pipelines with single commands.

> **Note**: This RFC was extracted from RFC-0042. The Containerfile
> generation portion of RFC-0042 is implemented; this helper binary
> is not yet implemented.

## Problem

Each upstream fetch stage in the Containerfile is 8-15 lines of shell
that constructs URLs, downloads, verifies checksums, and extracts —
all from `jq` queries against the manifest. This pattern repeats 7
times with slight variations. The base stage has 30 lines of `printf`
to write `.repo` files. Every hand-edit is a bug risk.

### Before (fetch-starship, 10 lines)

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

### After (1 meaningful line)

```dockerfile
FROM base AS fetch-starship
COPY --from=tools /bkt-build /usr/local/bin/bkt-build
COPY upstream/manifest.json /tmp/upstream-manifest.json
RUN bkt-build fetch starship
```

## Design

### Commands

| Command                          | What it does                              | Replaces                             |
| -------------------------------- | ----------------------------------------- | ------------------------------------ |
| `bkt-build fetch <name>`         | Download, verify, and install an upstream | fetch-starship, fetch-lazygit, etc.  |
| `bkt-build setup-repos`          | Import GPG keys and write `.repo` files   | 30-line `printf` block in base stage |
| `bkt-build download-rpms <repo>` | `dnf download` packages for a named repo  | dl-vscode, dl-edge, dl-1password     |
| `bkt-build lint [containerfile]` | Validate Containerfile for ostree issues  | `scripts/check-ostree-paths`         |

### `bkt-build lint`

Validates a Containerfile for common ostree filesystem mistakes:

```bash
bkt-build lint Containerfile
bkt-build lint --fix Containerfile  # Future: auto-fix simple issues
```

**Checks performed:**

| Check                  | Severity | Description                                                             |
| ---------------------- | -------- | ----------------------------------------------------------------------- |
| `/usr/local/bin` usage | Error    | Should use `/usr/bin` (ostree symlinks `/usr/local` to `/var/usrlocal`) |
| `/opt` usage           | Warning  | Consider `/usr/share` for read-only data                                |
| `/etc` direct writes   | Warning  | Prefer `/usr/etc` for defaults                                          |
| Mutable paths in COPY  | Error    | Paths like `/var/lib` won't persist correctly                           |

**Output:**

```
$ bkt-build lint Containerfile
ERROR: Line 45: /usr/local/bin found
       On ostree, /usr/local -> /var/usrlocal (persistent, not updated)
       Use /usr/bin instead

WARNING: Line 78: /opt/myapp found
         Consider /usr/share/myapp for read-only application data

1 error, 1 warning
```

**CI integration:**

```yaml
- name: Lint Containerfile
  run: bkt-build lint Containerfile
```

This replaces `scripts/check-ostree-paths` with a Rust implementation that can be extended with more checks and eventually auto-fix capabilities.

### How `bkt-build fetch` Works

1. Read `/tmp/upstream-manifest.json` (COPY'd into the build context)
2. Find the entry by `name`
3. Based on `source.type` and `install.type`, dispatch to the right
   fetch/verify/install strategy:

| source.type                        | install.type       | Strategy                                                         |
| ---------------------------------- | ------------------ | ---------------------------------------------------------------- |
| `github` + `asset_pattern`         | `binary`           | Download release asset → sha256 verify → chmod to `install_path` |
| `github` + `asset_pattern`         | `archive`          | Download release asset → sha256 verify → extract to `extract_to` |
| `github` + `release_type: release` | (none/script)      | Download raw file by commit → sha256 verify → chmod              |
| `github` + `release_type: tag`     | `script`           | Git clone at tag → run install script                            |
| `url`                              | `binary`/`archive` | Direct URL download → sha256 verify → install                    |

### Delivery Mechanism

`bkt-build` is compiled as a static musl binary and made available as
a separate build stage:

```dockerfile
FROM scratch AS tools
COPY bkt-build /bkt-build
```

Each stage that needs it does `COPY --from=tools /bkt-build /usr/local/bin/bkt-build`.
CI builds `bkt-build` before `docker build` and injects it into the
build context.

### Shared `bkt-common` Crate

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
- Upstream manifest types

## Implementation Plan

### PER D: Shared Crate + `bkt-build fetch`

Extract `bkt-common/` shared crate from `fetchbin`. Create `bkt-build/`
crate depending on `bkt-common`. Implement `bkt-build fetch <name>`.

- **Difficulty**: Hard
- **Codebase touches**: `bkt-common/` (new), `bkt-build/` (new),
  `fetchbin/` (refactored)

### PER E: `bkt-build` Repo Commands

Add `bkt-build setup-repos` and `bkt-build download-rpms <repo>`.

- **Difficulty**: Moderate
- **Dependencies**: PER D

### PER F: CI + Containerfile Integration

CI builds musl `bkt-build`, injects into Docker build context. Rewrite
fetch/download/setup stages to use `bkt-build` commands.

- **Difficulty**: Moderate
- **Dependencies**: PER E

## Design Decisions

### Static Musl Binary

`bkt-build` is statically linked via `x86_64-unknown-linux-musl`. This
provides portability across build environments without depending on the
container's glibc.

### `ureq` HTTP Client (Pure Rust)

`bkt-common` uses `ureq` with `rustls` instead of `reqwest` with
OpenSSL. This cuts the dependency tree from ~100+ crates to ~30 and
eliminates the C library complications that make musl static linking
painful.

### `lzma-rs` for tar.xz (Pure Rust)

bibata-cursor ships as `.tar.xz`. `bkt-common` adds tar.xz support via
`lzma-rs`, a pure Rust LZMA implementation that works out of the box
with musl.

### `pinned.url` for All Entries

`bkt upstream pin` resolves the actual download URL for every entry
and stores it in `pinned.url`. This means `bkt-build fetch` never
calls the GitHub API — it downloads from the stored URL, verifies
sha256, and installs.

## Relationship to Other RFCs

- **RFC 0042 (Managed Containerfile)**: This RFC was extracted from 0042. The Containerfile generator is implemented; this helper binary
  would further simplify the generated stages.
- **RFC 0006 (Upstream Management)**: `bkt-build fetch` is the build-time
  counterpart to `bkt upstream pin` — one tracks versions, the other
  consumes them during the build.
