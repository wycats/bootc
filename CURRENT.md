# Current State

**Epoch:** Phase 4 — Manifest Fidelity & Workflow Gaps  
**Last Updated:** 2026-01-30

---

## Active PR

**[PR #88](https://github.com/wycats/bootc/pull/88):** Fetchbin + Distrobox Live Capture

| Component              | Status                 |
| ---------------------- | ---------------------- |
| fetchbin crate         | ✅ Complete (52 tests) |
| Distrobox live capture | ✅ Complete            |
| Review comments        | ✅ Addressed           |

---

## Critical Fixes (P0) ✅ Complete

All P0 issues have been fixed.

| Issue                       | File                      | Status | Commit |
| --------------------------- | ------------------------- | ------ | ------ |
| ✅ `bootc-apply` fixed      | `scripts/bootc-apply`     | Done   | 90571b3 |
| ✅ `bootc-bootstrap` fixed  | `scripts/bootc-bootstrap` | Done   | 90571b3 |
| ✅ Export location fixed    | `docs/ARCHITECTURE.md`    | Done   | df8e4b8 |

**Evidence:** [2025-01-30-roadmap.md](docs/reports/2025-01-30-roadmap.md)

---

## Context

| Aspect              | Value                                                   |
| ------------------- | ------------------------------------------------------- |
| **Runtime**         | Bazzite (bootc/ostree), GNOME Shell 49.1                |
| **Development**     | `bootc-dev` distrobox container                         |
| **Execution Model** | Host-first: `bkt` runs on host only                     |
| **PATH Policy**     | Complete PATH via environment.d, no `$PATH` inheritance |

---

## Phase 4 Status

### Completed ✅

| Item                     | Description                              | PR  |
| ------------------------ | ---------------------------------------- | --- |
| Distrobox Live Capture   | `bkt distrobox capture --live`           | #88 |
| Extension Enabled State  | Capture/apply enabled/disabled           | #58 |
| Flatpak Override Capture | Capture Flatseal changes                 | #58 |
| Fetchbin Crate           | Binary acquisition from npm/cargo/github | #88 |
| Subsystem Phases         | Execution ordering (RFC-0029)            | #86 |
| Dev/System Split         | `bkt dev` + `bkt system` (RFC-0020)      | #86 |

### Remaining

| Item                         | Size | Status      | Notes                           |
| ---------------------------- | ---- | ----------- | ------------------------------- |
| GSettings Auto-Discovery     | M    | Not Started | RFC-0016                        |
| Drift Resolution             | M    | Not Started | RFC-0007 (check only, no apply) |
| Upstream Dependency Tracking | XL   | Not Started | RFC-0006                        |

---

## Documentation Debt (P1)

From [codebase audit](docs/reports/2025-01-30-distrobox-environment-audit.md):

| Issue                 | Locations                               | Fix                                  |
| --------------------- | --------------------------------------- | ------------------------------------ |
| `bkt dnf` references  | WORKFLOW, ARCHITECTURE, README, CURRENT | Change to `bkt system`               |
| Delegation model      | ARCHITECTURE, WORKFLOW, README          | Remove "works from host and toolbox" |
| PATH policy           | RFC-0017, HANDOFF (archived)            | Align with VISION.md                 |
| Planned → Implemented | ARCHITECTURE                            | Mark `bkt status`, distrobox capture |

---

## Blockers

None currently.

---

## References

| Document                                                                 | Purpose                          |
| ------------------------------------------------------------------------ | -------------------------------- |
| [docs/VISION.md](docs/VISION.md)                                         | Long-term architecture goals     |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)                             | Technical design (needs updates) |
| [docs/WORKFLOW.md](docs/WORKFLOW.md)                                     | User workflows (needs updates)   |
| [QUALITY.md](QUALITY.md)                                                 | Quality backlog                  |
| [docs/reports/2025-01-30-roadmap.md](docs/reports/2025-01-30-roadmap.md) | Full issue breakdown             |

---

## History

| Phase               | Focus                   | Archive                                                                                     |
| ------------------- | ----------------------- | ------------------------------------------------------------------------------------------- |
| Phase 1             | Bootstrap               | [001-bootstrap.md](docs/history/001-bootstrap.md)                                           |
| Phase 2             | Distribution Management | [002-phase2-distribution-management.md](docs/history/002-phase2-distribution-management.md) |
| Phase 3             | Complete the Loop       | [003-phase3-complete-the-loop.md](docs/history/003-phase3-complete-the-loop.md)             |
| Distrobox Migration | Host-first workflow     | [2026-01-20-distrobox-handoff.md](docs/history/2026-01-20-distrobox-handoff.md)             |
