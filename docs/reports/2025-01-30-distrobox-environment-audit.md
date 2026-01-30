# Distrobox Environment Audit Report

**Date:** January 30, 2026  
**Status:** Investigation Complete  
**Branch:** feat/fetchbin-and-distrobox-capture

## Executive Summary

A comprehensive audit of the distrobox environment revealed significant gaps between documented expectations and runtime reality. The investigation identified **6 major discrepancies** that cause friction and confusion.

### Key Findings

1. **PATH policy conflict** — VISION.md says "no PATH inheritance," but RFC-0017 and HANDOFF.md still recommend `PATH="...:$PATH"`
2. **Export location mismatch** — Docs say exports land in `/usr/bin`, reality is `~/.local/bin/distrobox`
3. **Delegation model outdated** — Docs claim universal host/toolbox delegation, but code is host-only
4. **Missing host shims** — Bootstrap script calls non-existent `shim` command, so shims never generate
5. **Manifest schema drift** — RFC examples use `exported_bins`, actual schema uses `bins.from`/`bins.to`
6. **Distrobox capture status** — Docs say "planned," but it's already implemented

---

## Part 1: Runtime Environment State

### Current Context
- **Host:** bazzite (Fedora-based bootc)
- **Container:** `bootc-dev` (Up 22+ hours, image: `ghcr.io/wycats/bootc-toolbox:latest`)
- **Terminal:** Running on host (no `CONTAINER_ID`, no `/run/.containerenv`)

### PATH Analysis (Host)

**Expected (from environment.d):**
~/.local/bin/distrobox
~/.local/bin
/usr/local/sbin
/usr/local/bin
/usr/bin
```

**Actual (VS Code terminal):**
```
~/.config/Code/.../debugCommand    (VS Code injected)
~/.config/Code/.../copilotCli      (VS Code injected)
~/.local/share/pnpm                (unexpected)
~/.local/bin/distrobox             ✓
~/.local/bin                       ✓
/usr/local/sbin                    ✓
/usr/local/bin                     ✓
/usr/bin                           ✓
/home/linuxbrew/.linuxbrew/bin     (×3 duplicates!)
/home/linuxbrew/.linuxbrew/sbin    (×3 duplicates!)
```

**Issues:**
- Linuxbrew paths appear 3 times each
- pnpm path not in expected set
- VS Code injects paths (acceptable but not documented)

### Shim Analysis

**Distrobox export shims (`~/.local/bin/distrobox/`):** ✅ Present
- cargo, rustc, node, pnpm, nu, rust-analyzer, etc.
- Correctly use `distrobox-enter` for host→container execution

**Host shims (`~/.local/bin/shims/`):** ❌ Empty
- Expected: systemctl, journalctl, flatpak, podman, bootc, rpm-ostree
- Reality: Directory doesn't exist or is empty

**Container shims (`/usr/local/bin/shims/`):** ❌ Missing
- The toolbox container PATH includes this directory
- But no shims are generated there

---

## Part 2: Documentation vs Reality

### 2.1 PATH Policy Conflict

| Document | Says | Should Say |
|----------|------|------------|
| [docs/VISION.md](../VISION.md) | ✅ "Define complete PATH, no inheritance" | (Correct) |
| [docs/rfcs/0017-distrobox-integration.md](../rfcs/0017-distrobox-integration.md) | ❌ `PATH="$HOME/.local/bin:$PATH"` | Complete PATH |
| [HANDOFF.md](../../HANDOFF.md) | ❌ `PATH="$HOME/.local/bin:$PATH"` | Complete PATH |

### 2.2 Export Location Mismatch

| Document | Claims | Reality |
|----------|--------|---------|
| [docs/ARCHITECTURE.md](../ARCHITECTURE.md) | `/usr/bin` | `~/.local/bin/distrobox` |
| [manifests/distrobox.json](../../manifests/distrobox.json) | `"to": "~/.local/bin/distrobox"` | ✅ Correct |

### 2.3 Delegation Model Outdated

| Document | Claims | Reality |
|----------|--------|---------|
| [docs/ARCHITECTURE.md](../ARCHITECTURE.md) | All commands delegate via `flatpak-spawn --host` | `bkt` is host-only |
| [docs/WORKFLOW.md](../WORKFLOW.md) | "All bkt commands work from both host and toolbox" | Host-only execution |
| [README.md](../../README.md) | Same claim | Same issue |

### 2.4 Manifest Schema Drift

| RFC Example | Actual Schema |
|-------------|---------------|
| `exported_bins` | `bins.from` (directories) |
| `exported_bins_path` | `bins.to` |
| (none) | `bins.also` (individual files) |

---

## Part 3: Root Cause Analysis

### Why Are Host Shims Missing?

**Primary cause:** `scripts/bootc-bootstrap` calls `shim sync`, but the command is `bkt shim sync`.

```bash
# scripts/bootc-bootstrap (line ~45)
if need_cmd shim; then
  log "Syncing host shims"
  shim sync || log "shim sync failed (non-fatal)"
else
  log "shim command not found; skipping host shims"  # ← This happens!
fi
```

**Fix:** Change `shim` to `bkt shim` or add a `shim` alias.

### Why Is PATH Polluted?

1. **Linuxbrew:** `eval "$(brew shellenv)"` is sourced multiple times (likely in `.bashrc` and `.profile`)
2. **VS Code:** Injects its own paths for Copilot CLI integration
3. **pnpm:** Added by pnpm setup script

**Fix:** Remove PATH modifications from shell rc files; rely solely on `environment.d`.

---

## Part 4: Proposed Fixes

### 4.1 Documentation Patch Plan (14 edits)

**Priority 1 (Critical):**
1. Fix ARCHITECTURE.md export location (`/usr/bin` → `~/.local/bin/distrobox`)
2. Fix ARCHITECTURE.md delegation table (remove flatpak-spawn claims)
3. Fix WORKFLOW.md "where to run bkt" section
4. Fix README.md delegation claim

**Priority 2 (High):**
5. Update RFC-0017 PATH examples (use complete PATH)
6. Update RFC-0017 manifest examples (use current schema)
7. Update HANDOFF.md PATH instructions

**Priority 3 (Medium):**
8. Mark distrobox capture as implemented (not "planned")
9. Add troubleshooting section to WORKFLOW.md
10. Document VS Code PATH injection behavior

### 4.2 Bootstrap Script Fix

```bash
# Change from:
if need_cmd shim; then
  shim sync

# To:
if need_cmd bkt; then
  bkt shim sync
```

### 4.3 PATH Health Check for `bkt doctor`

Add comprehensive PATH validation:
- Detect duplicates
- Check for forbidden paths (~/.cargo/bin on host)
- Verify ordering (distrobox shims first)
- Report unexpected entries

**Proposed checks:**
- `PATH (duplicates)` — fail if any entry appears more than once
- `PATH (forbidden)` — fail if toolchain paths appear on host
- `PATH (missing)` — fail if required entries are absent
- `PATH (ordering)` — fail if distrobox shims aren't first

---

## Part 5: Friction Points Summary

### Incoherencies That Introduce Unnecessary Friction

1. **Shim generation never runs** — Users get empty shim directories and wonder why host commands don't work in container

2. **PATH accumulates instead of being set** — Users end up with duplicate entries and shadowed binaries

3. **Docs describe a world that doesn't exist** — New users following docs will be confused when behavior doesn't match

4. **Multiple sources of truth** — PATH policy in VISION.md vs RFC-0017 vs HANDOFF.md all say different things

5. **RFC describes unimplemented features** — `host_only` shim flag is documented but not in schema or code

---

## Appendix A: Files Referenced

### Documentation
- [docs/VISION.md](../VISION.md) — Canonical PATH policy
- [docs/ARCHITECTURE.md](../ARCHITECTURE.md) — Runtime context, delegation
- [docs/WORKFLOW.md](../WORKFLOW.md) — User-facing how-to
- [docs/rfcs/0017-distrobox-integration.md](../rfcs/0017-distrobox-integration.md) — Distrobox design
- [docs/rfcs/0018-host-only-shims.md](../rfcs/0018-host-only-shims.md) — Host shim design

### Configuration
- [manifests/distrobox.json](../../manifests/distrobox.json) — Container definition
- [manifests/host-shims.json](../../manifests/host-shims.json) — Host shim manifest
- [~/.config/environment.d/10-distrobox-exports.conf] — Host PATH config

### Code
- [scripts/bootc-bootstrap](../../scripts/bootc-bootstrap) — First-login setup
- [bkt/src/commands/shim.rs](../../bkt/src/commands/shim.rs) — Shim generation
- [bkt/src/commands/doctor.rs](../../bkt/src/commands/doctor.rs) — Health checks

---

## Appendix B: Recommended Next Steps

1. **Immediate:** Fix bootstrap script to use `bkt shim sync`
2. **Short-term:** Apply documentation patches (Priority 1 first)
3. **Medium-term:** Implement PATH health check in `bkt doctor`
4. **Long-term:** Decide whether to implement `host_only` shims or remove from RFC
