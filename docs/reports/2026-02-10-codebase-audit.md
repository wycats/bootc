# Codebase Audit: Vision Alignment and Steel Thread Analysis

**Date:** 2026-02-10

---

## Context

This audit originated from a fontconfig cache investigation that revealed a
lifecycle gap in atomic Linux — no user-level post-deployment hooks exist. That
finding prompted a broader question: what does this repository add to atomic
Linux, and are we building the right things?

## Key Insight: Why This Repo Exists

Atomic Linux made the OS base declarative, reproducible, and recoverable. But it
drew the line at `/usr`. Everything above — Flatpaks, GNOME extensions,
gsettings, dev environments — is unmanaged mutable state. This repo extends
atomic Linux's principle upward into the user layer.

Two gaps:

1. **The user environment is not reproducible.** No Containerfile-equivalent for
   the user layer. This repo provides manifests + capture + apply.
2. **No lifecycle bridge between deployments and user state.** When the image
   changes, nobody tells the user session. `bootc-bootstrap` fills this gap.

Gap #2 is solved (the shell script works). Gap #1 is mostly solved but missing
one critical piece: **verification** (drift detection).

## Priority Analysis

Atomic Linux gives the OS base three properties: reproducibility, recovery, and
verification (the deployed image matches a known checksum). This repo has the
first two for the user layer. Drift detection is the third — the feature that
makes the vision actually true.

**Priority order, derived from the vision:**

1. **Drift detection** — closes the reproducibility loop
2. **GSettings auto-discovery** — capture completeness
3. **Upstream tracking** — keeping the baked tier current
4. **Doc alignment** — docs should match reality
5. **bkt bootstrap migration** — nice-to-have, not a capability gap

## Drift Detection: Not Missing, Just Unassembled

The recon found that drift detection infrastructure is largely built but
scattered. The project docs (CURRENT.md, RFC 0007, `bkt drift check` stub)
all say "not started," which misled both agents and humans.

### What exists

| Subsystem  |  List installed?  | Load manifest? | Compute diff? | Where                        |
| ---------- | :---------------: | :------------: | :-----------: | ---------------------------- |
| Flatpak    |        ✅         |       ✅       |      ✅       | `flatpak.rs`, `profile.rs`   |
| Extensions |        ✅         |       ✅       |      ✅       | `extension.rs`, `profile.rs` |
| GSettings  |        ✅         |       ✅       |      ✅       | `gsetting.rs`, `profile.rs`  |
| Homebrew   |        ✅         |       ✅       |      ✅       | `homebrew.rs`                |
| AppImage   |        ✅         |       ✅       |      ✅       | `appimage.rs`                |
| Fetchbin   |        ✅         |       ✅       |      ✅       | `fetchbin.rs`                |
| Shims      |   FS check only   |       ✅       |      ❌       | `status.rs`                  |
| Distrobox  | Container vs base |       ✅       |      ❌       | `distrobox.rs`               |

Additionally:

- `bkt profile diff` prints missing/extra for Flatpaks, extensions, gsettings
- `bkt status` computes `pending_sync`/`pending_capture` and "Drift Detected"
- `manifest/diff.rs` has generic `Diffable` trait and collection diff utilities
- Containerfile sync has its own drift detection (`is_drift`)

### What's missing

1. `bkt drift check` is a stub — no composition of subsystem comparisons
2. No unified drift report (RFC 0007 designed a JSON format, never built)
3. Shims and distrobox lack manifest-vs-live comparison
4. `bkt status`, `bkt profile diff`, and `bkt drift` don't share code

### Proposed command surface

- `bkt drift check` becomes the detailed report (composing existing subsystem
  comparisons via a shared engine)
- `bkt status` keeps its summary view (using the same engine)
- `bkt profile diff` either folds into `bkt drift` or narrows scope

### Why the docs were wrong

CURRENT.md says "Drift Resolution: Not Started." The `bkt drift check` stub
says "not implemented." RFC 0007 says "Implementation Deferred." All three
signal "nothing exists" — but the subsystem-level comparison code is built
across capture, profile, and status. The project tracks the _command_, not the
_capability_.

---

## RFC Triage

### Summary

| Category      | Count | Action                         |
| ------------- | :---: | ------------------------------ |
| CANON         |  10   | Move to `docs/rfcs/canon/`     |
| WITHDRAW      |   3   | Remove (superseded/obsolete)   |
| UPDATE        |  14   | Rewrite to match reality       |
| FUTURE        |   8   | Keep as-is                     |
| FUTURE-UPDATE |   3   | Update framing, keep as future |

### CANON — Move to `docs/rfcs/canon/`

Fully implemented and accurately describe current behavior.

| RFC  | Title                          |
| ---- | ------------------------------ |
| 0008 | Command Infrastructure         |
| 0009 | Runtime Privileged Operations  |
| 0010 | Transparent Command Delegation |
| 0011 | Testing Strategy and DI        |
| 0012 | GearLever/AppImage Integration |
| 0014 | Extension State Management     |
| 0020 | Dev and System Commands        |
| 0032 | Binary Acquisition (fetchbin)  |
| 0039 | keyd Mac Muscle Memory         |
| 0040 | Workstation Design Manifesto   |

### WITHDRAW

Superseded or obsolete. Remove or mark withdrawn.

| RFC  | Title               | Reason                                          |
| ---- | ------------------- | ----------------------------------------------- |
| 0002 | `bkt dnf`           | Superseded by RFC 0020 (`bkt dev`/`bkt system`) |
| 0018 | Host-Only Shims     | Superseded by RFC 0010 (delegation)             |
| 0025 | AppImage Management | Superseded by RFC 0012 (GearLever)              |

### UPDATE — Rewrite or withdraw-and-redraft

Implemented but RFC text doesn't match reality. Goal: output reads as if it were
written today, with no lingering outdated framing.

| RFC   | Title                   | Issue                                                 | Effort                    |
| ----- | ----------------------- | ----------------------------------------------------- | ------------------------- |
| 0001  | Command Punning         | Outdated command examples                             | Low                       |
| 0003  | `bkt dev`               | Large unimplemented sections, merge conflicts in file | High — withdraw & redraft |
| 0004  | `bkt admin`             | Missing udev/SELinux/firmware                         | Medium                    |
| 0005  | Changelog               | PR parsing/release not implemented                    | Medium                    |
| 0006  | Upstream Management     | Mirrors/GPG not implemented                           | Medium                    |
| 0013  | Build Descriptions      | Publish/rollup not implemented                        | Low                       |
| 0015  | Flatpak Overrides       | Show/edit commands missing                            | Low                       |
| 0017  | Distrobox Integration   | Helper subcommands missing                            | Low                       |
| 0019b | Environment Abstraction | RFC file empty, code exists                           | Low — write from code     |
| 0021  | Local Change Management | RFC file empty, code exists                           | Low — write from code     |
| 0022  | Homebrew Management     | RFC file empty, code exists                           | Low — write from code     |
| 0023  | Status Dashboard        | RFC file empty, code exists                           | Low — write from code     |
| 0024  | Doctor Command          | RFC file empty, code exists                           | Low — write from code     |
| 0029  | Subsystem Dependencies  | Phase model not wired into apply/capture              | Medium                    |
| 0036  | Kernel Arguments        | Only manifest-only, no kargs.d                        | Low                       |

### FUTURE — Keep as-is

Not implemented, but accurately describe desired future work.

| RFC  | Title                 |
| ---- | --------------------- |
| 0016 | GSettings Discovery   |
| 0028 | Plugin Subsystems     |
| 0030 | VM Management         |
| 0031 | Windows VM Workflow   |
| 0033 | Fetchbin Enhancements |
| 0035 | `bkt admin update`    |
| 0037 | `bkt upgrade`         |
| 0038 | RPM-Aware Rebuild     |

### FUTURE-UPDATE — Update framing, keep as future

| RFC   | Title                  | Issue                                                 |
| ----- | ---------------------- | ----------------------------------------------------- |
| 0007  | Drift Detection        | Doesn't account for existing subsystem infrastructure |
| 0019a | Cron-able Sync         | `bkt sync` doesn't exist; apply/capture remain        |
| 0034  | usroverlay Integration | No matching commands exist                            |

---

## Steel Thread Assessment

| Thread                                                  | Status                                        | Gap                         |
| ------------------------------------------------------- | --------------------------------------------- | --------------------------- |
| **Capture loop** (live → manifests → git)               | Mostly complete                               | Timer-driven, working       |
| **Bootstrap loop** (login → apply manifests)            | Mostly complete                               | Shell script, working       |
| **Image build loop** (manifests → Containerfile → CI)   | Mostly complete                               | Working                     |
| **Distrobox dev env** (manifest → assemble)             | Mostly complete                               | Working                     |
| **Drift detection** (verify: system matches manifests?) | **Infrastructure built, composition missing** | `bkt drift check` is a stub |
| **Status dashboard**                                    | Implemented                                   | Has partial drift data      |
| **Local change tracking**                               | Implemented                                   | Working                     |
| **Upstream tracking**                                   | Partial                                       | No automation               |

---

## Architectural Tensions

1. **bootc-bootstrap vs bkt apply**: Shell script runs first on login, applies
   remotes/apps/extensions/gsettings/shims/distrobox/fontconfig. bkt apply does
   a subset in different order. Neither knows about the other's state. This is
   a code org question, not a capability gap — the shell script works.

2. **Manifest location split**: Shell reads from `/usr/share/bootc-bootstrap`
   (baked). bkt reads from repo working directory. Could diverge at login.

3. **Ordering**: Shell does remotes→apps→extensions→gsettings→shims. bkt apply
   does shim→distrobox→gsetting→extension→flatpak→appimage. RFC 0029 defines
   phases neither follows exactly.

4. **Drift code duplication**: status, profile diff, and subsystem capture plans
   all compute manifest-vs-live comparisons independently.

---

## Execution Plan

### Phase 1: RFC Triage (documentation)

1. Move 10 CANON RFCs to `docs/rfcs/canon/`
2. Mark 3 WITHDRAW RFCs as withdrawn
3. Update/rewrite 14 UPDATE RFCs (prioritize low-effort ones)
4. Update 3 FUTURE-UPDATE RFCs

### Phase 2: CURRENT.md Rebuild

Rewrite CURRENT.md as a codebase state document reflecting actual build status
of each capability, not a phase tracker.

### Phase 3: Drift Detection (implementation)

1. Build shared drift engine composing existing subsystem comparisons
2. Wire into `bkt drift check` with RFC 0007 JSON output
3. Fill gaps: shim and distrobox manifest-vs-live comparison
4. Integrate into `bkt status` summary view
5. Decide `bkt profile diff` fate (fold in or narrow)
