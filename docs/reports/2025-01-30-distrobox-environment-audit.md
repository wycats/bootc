# Distrobox Environment Audit Report# Distrobox Environment Audit Report


























































































































































































































- `docs/rfcs/0018-host-only-shims.md` — Host-only shims RFC- `docs/rfcs/0017-distrobox-integration.md` — Distrobox integration RFC- `docs/WORKFLOW.md` — User workflow- `docs/ARCHITECTURE.md` — Runtime context, delegation- `docs/VISION.md` — Canonical PATH policy### Documentation- `scripts/bootc-bootstrap` — First-login bootstrap### Scripts- `bkt/src/containerfile.rs` — Containerfile generation- `bkt/src/subsystem/shim.rs` — Shim subsystem- `bkt/src/commands/doctor.rs` — Doctor checks- `bkt/src/commands/shim.rs` — Shim generation### Code- `manifests/host-shims.json` — Host shim manifest- `manifests/distrobox.json` — Distrobox container manifest- `~/.config/environment.d/10-distrobox-exports.conf` — Host PATH policy### Configuration## Appendix: Files Involved---5. [ ] Decide on `host_only` RFC implementation4. [ ] Implement PATH health check (see Prepare Agent 2 design)3. [ ] Apply documentation patches (see Prepare Agent 1 report)2. [ ] Decide canonical shim path and align code/docs1. [ ] Fix `scripts/bootc-bootstrap` to use `bkt shim sync`## Next Steps---**RFC mismatch:** `host_only` documented but not implemented.**Path mismatch:** Code writes to `~/.local/toolbox/shims/`, not `~/.local/bin/shims/`.**Primary cause:** `scripts/bootc-bootstrap` calls `shim sync` but only `bkt shim sync` exists.**Status:** Root cause identified### Recon Agent: Missing Host Shims- `PathHealthReport` — duplicates, unexpected, forbidden, missing, out_of_order- `PathPolicy` — expected, forbidden, allowed_transient pathsKey data structures:- Context detection (host vs container)- Ordering validation- Missing required path detection- Forbidden path detection (toolchain paths)- Duplicate detectionDesigned comprehensive PATH validation:**Status:** Ready with caveats### Prepare Agent 2: Doctor PATH Health Check- `HANDOFF.md` (PATH example)- `README.md` (delegation claim)- `docs/rfcs/0017-distrobox-integration.md` (PATH examples, manifest schema)- `docs/WORKFLOW.md` (where to run bkt)- `docs/ARCHITECTURE.md` (export location, delegation model)Identified 6 critical fixes and 4 high-priority fixes across:**Status:** Ready with caveats### Prepare Agent 1: Documentation Patch Plan## Agent Reports Summary---10. **Document VS Code PATH behavior** — Explain why terminal PATH differs from environment.d9. **Add PATH health check to `bkt doctor`** — Detect duplicates, forbidden paths, missing entries8. **Implement or remove `host_only`** — Either add to schema/code or remove from RFC### Medium Priority (Technical Debt)7. **Fix RFC-0017** — Update PATH examples and manifest schema examples6. **Fix HANDOFF.md** — Use complete PATH, not `$PATH` inheritance5. **Fix README.md** — Same4. **Fix WORKFLOW.md** — Remove "all commands work from both" claim3. **Fix ARCHITECTURE.md** — Update export location and delegation model### High Priority (Causes Confusion)2. **Align shim path** — Either update code to use `~/.local/bin/shims/` or update docs/expectations1. **Fix bootstrap script** — Change `shim sync` to `bkt shim sync`### Immediate (Blocking)## Recommended Fixes---| Export destination | `/usr/bin` in docs | `~/.local/bin/distrobox` in manifest || `source` field | Documented in RFC-0018 | Not in schema or code || `host_only` flag | Documented in RFC-0018 | Not in schema or code ||---------|-----|-------------|| Feature | RFC | Schema/Code |### 4. Schema/RFC Mismatch| `docs/rfcs/0017-distrobox-integration.md` | Same PATH inheritance pattern || `HANDOFF.md` | Uses `PATH="$HOME/.local/bin:$PATH"` (violates VISION policy) || `README.md` | Same delegation claim || `docs/WORKFLOW.md` | "All bkt commands work from both host and toolbox" (wrong: host-only) || `docs/ARCHITECTURE.md` | Delegation table shows `flatpak-spawn --host` (deprecated) || `docs/ARCHITECTURE.md` | Claims exports land in `/usr/bin` (wrong: `~/.local/bin/distrobox`) ||----------|-------|| Document | Issue |### 3. Documentation Drift**Secondary cause:** Shell rc files or Homebrew's `shellenv` being sourced multiple times.**Primary cause:** VS Code terminal doesn't use environment.d PATH; it inherits from the graphical session and prepends its own paths.### 2. PATH Pollution**Secondary cause:** Path mismatch — code writes to `~/.local/toolbox/shims/`, not `~/.local/bin/shims/`.```fi  bkt shim syncif need_cmd bkt; then# Should be:fi  shim syncif need_cmd shim; then# Current (broken):```bash**Primary cause:** `scripts/bootc-bootstrap` calls `shim sync`, but only `bkt shim sync` exists.### 1. Missing Host Shims## Root Cause Analysis---| `~/.local/toolbox/shims/` | Actual code output path | ❓ Not checked || `/usr/local/bin/shims/` (container) | Host→container shims | ❌ Does not exist || `~/.local/bin/shims/` | Host shims | ❌ Empty || `~/.local/bin/distrobox/` | Distrobox export shims | ✅ Present (30+ shims) ||----------|----------|--------|| Location | Expected | Actual |### Shim Status- VS Code paths inherited from host- Multiple distrobox shim duplicates- Multiple linuxbrew duplicatesContainer PATH is severely polluted with ~30 entries including:### PATH Analysis (Container)- pnpm path not in expected set- VS Code injects paths not in policy- Linuxbrew appears 3 times**Issues:**```/home/linuxbrew/.linuxbrew/sbin   ← DUPLICATE (3×)/home/linuxbrew/.linuxbrew/bin    ← DUPLICATE (3×)/usr/bin/usr/local/bin/usr/local/sbin/home/wycats/.local/bin/home/wycats/.local/bin/distrobox/home/wycats/.local/share/pnpm/home/wycats/.config/Code/User/globalStorage/github.copilot-chat/copilotCli/home/wycats/.config/Code/User/globalStorage/github.copilot-chat/debugCommand```**Actual** (VS Code terminal):```/usr/bin/usr/local/bin/usr/local/sbin$HOME/.local/bin$HOME/.local/bin/distrobox```**Expected** (from `~/.config/environment.d/10-distrobox-exports.conf`):### PATH Analysis (Host)| Distrobox container | `bootc-dev` (Up 22 hours) || /run/.containerenv | Not present || CONTAINER_ID | (empty — on host) || Hostname | `bazzite` ||-------|--------|| Check | Result |### Environment Detection## Runtime Findings---4. **Schema/RFC mismatch** — RFC documents `host_only` shim flag that isn't implemented3. **Documentation drift** — Multiple docs describe outdated delegation model and wrong export locations2. **Missing host shims** — `~/.local/bin/shims` is empty; bootstrap calls non-existent `shim` command1. **PATH pollution** — Host and container PATH have duplicate entries (linuxbrew 3×)A comprehensive audit of the distrobox environment revealed significant gaps between documented expectations and runtime reality. The core issues are:## Executive Summary**Status:** Investigation Complete**Branch:** `feat/fetchbin-and-distrobox-capture`  **Date:** January 30, 2026  
**Date:** January 30, 2026  
**Branch:** `feat/fetchbin-and-distrobox-capture`  
**Status:** Investigation Complete

## Executive Summary

A comprehensive audit of the distrobox environment revealed significant gaps between documented expectations and runtime reality. The core issues are:

1. **PATH pollution** — Host and container PATH have duplicate entries (linuxbrew 3×)
2. **Missing host shims** — `~/.local/bin/shims` is empty; bootstrap calls non-existent `shim` command
3. **Documentation drift** — Multiple docs describe outdated delegation model and wrong export locations
4. **Schema/RFC mismatch** — RFC documents `host_only` shim flag that isn't implemented

---

## Runtime Findings

### Environment Detection

| Check | Result |
|-------|--------|
| Hostname | `bazzite` |
| CONTAINER_ID | (empty — on host) |
| /run/.containerenv | Not present |
| Distrobox container | `bootc-dev` (Up 22 hours) |

### PATH Analysis (Host)

**Expected** (from `~/.config/environment.d/10-distrobox-exports.conf`):
```
$HOME/.local/bin/distrobox
$HOME/.local/bin
/usr/local/sbin
/usr/local/bin
/usr/bin
```

**Actual** (VS Code terminal):
```
/home/wycats/.config/Code/User/globalStorage/github.copilot-chat/debugCommand
/home/wycats/.config/Code/User/globalStorage/github.copilot-chat/copilotCli
/home/wycats/.local/share/pnpm
/home/wycats/.local/bin/distrobox
/home/wycats/.local/bin
/usr/local/sbin
/usr/local/bin
/usr/bin
/home/linuxbrew/.linuxbrew/bin    ← DUPLICATE (3×)
/home/linuxbrew/.linuxbrew/sbin   ← DUPLICATE (3×)
```

**Issues:**
- Linuxbrew appears 3 times
- VS Code injects paths not in policy
- pnpm path not in expected set

### PATH Analysis (Container)

Container PATH is severely polluted with ~30 entries including:
- Multiple linuxbrew duplicates
- Multiple distrobox shim duplicates
- VS Code paths inherited from host

### Shim Status

| Location | Expected | Actual |
|----------|----------|--------|
| `~/.local/bin/distrobox/` | Distrobox export shims | ✅ Present (30+ shims) |
| `~/.local/bin/shims/` | Host shims | ❌ Empty |
| `/usr/local/bin/shims/` (container) | Host→container shims | ❌ Does not exist |
| `~/.local/toolbox/shims/` | Actual code output path | ❓ Not checked |

---

## Root Cause Analysis

### 1. Missing Host Shims

**Primary cause:** `scripts/bootc-bootstrap` calls `shim sync`, but only `bkt shim sync` exists.

```bash
# Current (broken):
if need_cmd shim; then
  shim sync
fi

# Should be:
if need_cmd bkt; then
  bkt shim sync
fi
```

**Secondary cause:** Path mismatch — code writes to `~/.local/toolbox/shims/`, not `~/.local/bin/shims/`.

### 2. PATH Pollution

**Primary cause:** VS Code terminal doesn't use environment.d PATH; it inherits from the graphical session and prepends its own paths.

**Secondary cause:** Shell rc files or Homebrew's `shellenv` being sourced multiple times.

### 3. Documentation Drift

| Document | Issue |
|----------|-------|
| `docs/ARCHITECTURE.md` | Claims exports land in `/usr/bin` (wrong: `~/.local/bin/distrobox`) |
| `docs/ARCHITECTURE.md` | Delegation table shows `flatpak-spawn --host` (deprecated) |
| `docs/WORKFLOW.md` | "All bkt commands work from both host and toolbox" (wrong: host-only) |
| `README.md` | Same delegation claim |
| `HANDOFF.md` | Uses `PATH="$HOME/.local/bin:$PATH"` (violates VISION policy) |
| `docs/rfcs/0017-distrobox-integration.md` | Same PATH inheritance pattern |

### 4. Schema/RFC Mismatch

| Feature | RFC | Schema/Code |
|---------|-----|-------------|
| `host_only` flag | Documented in RFC-0018 | Not in schema or code |
| `source` field | Documented in RFC-0018 | Not in schema or code |
| Export destination | `/usr/bin` in docs | `~/.local/bin/distrobox` in manifest |

---

## Recommended Fixes

### Immediate (Blocking)

1. **Fix bootstrap script** — Change `shim sync` to `bkt shim sync`
2. **Align shim path** — Either update code to use `~/.local/bin/shims/` or update docs/expectations

### High Priority (Causes Confusion)

3. **Fix ARCHITECTURE.md** — Update export location and delegation model
4. **Fix WORKFLOW.md** — Remove "all commands work from both" claim
5. **Fix README.md** — Same
6. **Fix HANDOFF.md** — Use complete PATH, not `$PATH` inheritance
7. **Fix RFC-0017** — Update PATH examples and manifest schema examples

### Medium Priority (Technical Debt)

8. **Implement or remove `host_only`** — Either add to schema/code or remove from RFC
9. **Add PATH health check to `bkt doctor`** — Detect duplicates, forbidden paths, missing entries
10. **Document VS Code PATH behavior** — Explain why terminal PATH differs from environment.d

---

## Agent Reports Summary

### Prepare Agent 1: Documentation Patch Plan

**Status:** Ready with caveats

Identified 6 critical fixes and 4 high-priority fixes across:
- `docs/ARCHITECTURE.md` (export location, delegation model)
- `docs/WORKFLOW.md` (where to run bkt)
- `docs/rfcs/0017-distrobox-integration.md` (PATH examples, manifest schema)
- `README.md` (delegation claim)
- `HANDOFF.md` (PATH example)

### Prepare Agent 2: Doctor PATH Health Check

**Status:** Ready with caveats

Designed comprehensive PATH validation:
- Duplicate detection
- Forbidden path detection (toolchain paths)
- Missing required path detection
- Ordering validation
- Context detection (host vs container)

Key data structures:
- `PathPolicy` — expected, forbidden, allowed_transient paths
- `PathHealthReport` — duplicates, unexpected, forbidden, missing, out_of_order

### Recon Agent: Missing Host Shims

**Status:** Root cause identified

**Primary cause:** `scripts/bootc-bootstrap` calls `shim sync` but only `bkt shim sync` exists.

**Path mismatch:** Code writes to `~/.local/toolbox/shims/`, not `~/.local/bin/shims/`.

**RFC mismatch:** `host_only` documented but not implemented.

---

## Next Steps

1. [ ] Fix `scripts/bootc-bootstrap` to use `bkt shim sync`
2. [ ] Decide canonical shim path and align code/docs
3. [ ] Apply documentation patches (see Prepare Agent 1 report)
4. [ ] Implement PATH health check (see Prepare Agent 2 design)
5. [ ] Decide on `host_only` RFC implementation

---

## Appendix: Files Involved

### Configuration
- `~/.config/environment.d/10-distrobox-exports.conf` — Host PATH policy
- `manifests/distrobox.json` — Distrobox container manifest
- `manifests/host-shims.json` — Host shim manifest

### Code
- `bkt/src/commands/shim.rs` — Shim generation
- `bkt/src/commands/doctor.rs` — Doctor checks
- `bkt/src/subsystem/shim.rs` — Shim subsystem
- `bkt/src/containerfile.rs` — Containerfile generation

### Scripts
- `scripts/bootc-bootstrap` — First-login bootstrap

### Documentation
- `docs/VISION.md` — Canonical PATH policy
- `docs/ARCHITECTURE.md` — Runtime context, delegation
- `docs/WORKFLOW.md` — User workflow
- `docs/rfcs/0017-distrobox-integration.md` — Distrobox integration RFC
- `docs/rfcs/0018-host-only-shims.md` — Host-only shims RFC
