# RFC 0041: Multi-Stage Containerfile with Parallel Downloads

## Status

Draft — implementing

## Problem

Our Containerfile is a linear sequence of layers. Every step runs after the
previous one finishes:

```
base → repos → dnf install (all 22 packages) → starship → lazygit → keyd → getnf → fonts → bibata → whitesur → config → ...
```

This has two costs:

1. **No parallelism.** The three external RPM downloads (~400MB: VS Code,
   Edge, 1Password) happen one package at a time inside a single `dnf
   install`. Binary fetches (starship, lazygit, keyd, etc.) wait for the
   RPM install to finish even though they're completely independent.

2. **Monolithic cache key.** All 22 packages share one layer. When any
   external package updates, all packages are re-downloaded and
   reinstalled. `rpmcheck` (PR #106) gives us per-repo version tracking,
   but a linear Containerfile can't consume that granularity.

## Key Insight: Download ≠ Install

`dnf install` has three phases:

1. **Metadata refresh** — fetch `repomd.xml`, `primary.xml.gz` (~5-10s)
2. **Package download** — fetch `.rpm` files (~30-90s, dominated by Edge at ~170MB)
3. **Transaction** — resolve deps, extract files, run scriptlets, write RPM DB (~10-20s)

Only phase 3 needs the RPM database. Phases 1-2 are pure I/O with no
shared state. `dnf download` performs phases 1-2 without touching the
database.

## Proposal

Use multi-stage builds — a standard Dockerfile feature already supported
by BuildKit (which `docker/build-push-action` uses) — to parallelize
downloads and binary fetches while keeping a single sequential RPM
install.

### Architecture

```
         base (repos configured)
      /   |   \       \    \    \   \
dl-vscode dl-edge dl-1pw  starship lazygit keyd ...
      \   |   /       |    |    |   |
       image (dnf install + COPY binaries + config)
```

All branches from `base` run **concurrently**. The `image` stage waits
for all of them, then:

1. `COPY --from=dl-*` brings in pre-downloaded `.rpm` files
2. `dnf install /tmp/rpms/*.rpm curl distrobox ...` does a single
   transaction from local files + Fedora repos
3. `COPY --from=fetch-*` brings in binary artifacts

### Download Stages

```dockerfile
FROM base AS dl-vscode
ARG DNF_CACHE_EPOCH=0
RUN echo "cache-epoch: ${DNF_CACHE_EPOCH}" > /dev/null && \
    mkdir -p /rpms && \
    dnf download --destdir=/rpms code code-insiders

FROM base AS dl-edge
ARG DNF_CACHE_EPOCH=0
RUN echo "cache-epoch: ${DNF_CACHE_EPOCH}" > /dev/null && \
    mkdir -p /rpms && \
    dnf download --destdir=/rpms microsoft-edge-stable

FROM base AS dl-1password
ARG DNF_CACHE_EPOCH=0
RUN echo "cache-epoch: ${DNF_CACHE_EPOCH}" > /dev/null && \
    mkdir -p /rpms && \
    dnf download --destdir=/rpms 1password 1password-cli
```

Each produces a `/rpms/` directory containing just the named packages.
The `DNF_CACHE_EPOCH` ARG busts the cache when the CI detects version
changes.

### Install Stage

```dockerfile
FROM base AS image
COPY --from=dl-vscode /rpms/ /tmp/rpms/
COPY --from=dl-edge /rpms/ /tmp/rpms/
COPY --from=dl-1password /rpms/ /tmp/rpms/

ARG DNF_CACHE_EPOCH=0
RUN echo "cache-epoch: ${DNF_CACHE_EPOCH}" > /dev/null && \
    dnf install -y \
    /tmp/rpms/*.rpm \
    curl distrobox fontconfig gh jq ... \
    && rm -rf /tmp/rpms && dnf clean all
```

DNF resolves dependencies for both local RPMs and repo packages in a
single transaction. The big downloads are already local, so the
transaction is fast (~10-20s).

### Binary Fetch Stages

Same pattern, already implemented:

```dockerfile
FROM base AS fetch-starship
RUN ... curl + verify + extract → /usr/bin/starship

FROM base AS build-keyd
RUN ... git clone + make → /usr/bin/keyd + systemd unit

# In image stage:
COPY --from=fetch-starship /usr/bin/starship /usr/bin/starship
COPY --from=build-keyd /usr/bin/keyd /usr/bin/keyd
```

These are completely independent of the RPM stages and run in parallel
with them. `build-keyd` (git clone + gcc) is the slowest single step and
benefits the most from parallelism.

## Cache Busting

Currently: CI passes a single `DNF_CACHE_EPOCH` build-arg to all stages.
All download stages bust together — same behavior as the old monolithic
layer, but with parallel downloads.

Future: once rpmcheck emits per-repo hashes, each download stage gets its
own ARG (`VSCODE_CACHE_EPOCH`, `EDGE_CACHE_EPOCH`, etc.). VS Code
updating only busts the `dl-vscode` stage. The install stage remains
shared — it always runs after all downloads complete — but the
COPY steps bring in cached vs. fresh RPMs per-repo.

## Further Concurrency (Deferred)

The sequential `dnf install` transaction takes ~10-20s for 22 packages.
This is acceptable today. If it becomes a bottleneck, there are deeper
options:

- `rpm -ivh --nodb` to extract files in parallel, then `rpm --rebuilddb`
- Separate `dnf install` per repo group + RPM database merge

These are significantly more complex and fragile (scriptlet ordering,
database merge correctness). The download/install split captures ~80% of
the theoretical speedup with zero risk. Documenting the option here so
it's remembered, not pursued.

## CI Changes

Minimal — this builds on the existing `docker/build-push-action` + BuildKit
pipeline:

1. `target: image` added to `build-push-action` (explicit, was implicit)
2. `DNF_CACHE_EPOCH` build-arg already passed — now consumed by download
   stages AND the install stage
3. No new CI tools or actions required

## Benefits

1. **Parallel downloads** — VS Code, Edge, 1Password RPMs download
   concurrently (~30-90s → limited by slowest single download)
2. **Parallel binary fetches** — starship, lazygit, keyd, getnf, fonts,
   cursors, icons all run concurrently with each other and with RPM
   downloads
3. **No new tooling** — standard multi-stage Dockerfile, standard
   BuildKit, standard `docker/build-push-action`
4. **Per-repo cache busting ready** — ARG structure supports independent
   cache keys per download stage once rpmcheck per-repo hashes land
5. **Correct RPM database** — single `dnf install` transaction, no merge
   problems

## Risks

1. **`dnf download` without `--resolve`** — We download only the named
   packages, not their dependencies. The final `dnf install` pulls any
   missing deps from repos. If a dep is large and changes frequently,
   it won't benefit from the download parallelism. In practice, all
   large deps for our external packages are already in the base image.

2. **Duplicate deps across stages** — If two download stages both pull
   the same dep (unlikely without `--resolve`), the second `COPY` wins.
   Since they download from the same repos at the same time, versions
   will match. Non-issue in practice.

3. **bkt templating** — bkt manages `=== SYSTEM_PACKAGES ===` sections.
   The package list moved from a single `RUN` to separate download stages
   + a combined install. bkt needs to be aware of this structure. For
   now, bkt still templates the install stage's package list; the
   download stages are manual. A future bkt update can generate both.

## Alternatives Considered

### Earthly

Earthly adds Makefile-like targets on top of Dockerfile syntax. It
compiles to the same LLB DAG that BuildKit uses and provides nicer syntax
for parallel targets.

**Why not:** Multi-stage Dockerfile gives us the same parallelism without
adding a new tool. Earthly's extra syntax would help if we had a much
larger build graph, but for 10 parallel stages, standard multi-stage is
clear enough. Earthly is also VC-backed with uncertain future (layoffs
in 2024). If we outgrow multi-stage, Earthly is the natural next step.

### Go SDK / Dagger / Custom Frontend

Programmatic LLB generation. Maximum power, massive overkill for our
scale. Not considered seriously.

### DNF cache mounts (`RUN --mount=type=cache`)

BuildKit supports persistent cache mounts. We could cache the dnf
package directory across builds:

```dockerfile
RUN --mount=type=cache,target=/var/cache/dnf dnf install -y ...
```

This caches downloaded RPMs between builds but doesn't help with
parallelism or per-repo cache busting. Orthogonal optimization — could
be combined with this RFC's approach.

## PoC: RPM Database Merge Failure

Before arriving at the download/install split, we tested parallel `dnf
install` + `COPY --from`:

```dockerfile
FROM base AS vscode
RUN dnf install -y code && dnf clean all

FROM base AS edge
RUN dnf install -y microsoft-edge-stable && dnf clean all

FROM base AS merged
COPY --from=vscode / /
COPY --from=edge / /
# Result: only Edge in rpm -qa — VS Code's DB entries clobbered
```

This confirmed that RPM installs cannot be parallelized via COPY --from.
The download/install split avoids the problem entirely.
