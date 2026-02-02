# Backlog

**Last Updated:** 2026-01-30

Prioritized work not in current epoch. See [CURRENT.md](CURRENT.md) for active work.

---

## P1: Phase 4 Completion

Complete remaining Phase 4 items before advancing.

| Item                         | Size | RFC      | Notes                                     |
| ---------------------------- | ---- | -------- | ----------------------------------------- |
| GSettings Auto-Discovery     | M    | RFC-0016 | Reduce guesswork for settings capture     |
| Drift Resolution             | M    | RFC-0007 | Currently check-only; add apply workflow  |
| Upstream Dependency Tracking | XL   | RFC-0006 | Themes, icons, fonts with pinned versions |

---

## P2: Documentation Alignment

Fix docs to match code reality.

| Task                      | Files                                            | Notes                                  |
| ------------------------- | ------------------------------------------------ | -------------------------------------- |
| `bkt dnf` → `bkt system`  | WORKFLOW, ARCHITECTURE, README, manifests/README | Global rename                          |
| Remove delegation claims  | ARCHITECTURE, WORKFLOW, README                   | "Works from host and toolbox" is false |
| PATH policy alignment     | RFC-0017                                         | Remove `$PATH` inheritance examples    |
| Mark implemented features | ARCHITECTURE                                     | `bkt status`, distrobox capture        |
| Update export location    | ARCHITECTURE                                     | `/usr/bin` → `~/.local/bin/distrobox`  |

---

## P3: Schema & Code Alignment

| Task              | Notes                                      |
| ----------------- | ------------------------------------------ |
| Host-only shims   | Implement RFC-0018 OR retire it            |
| Schema generation | Add missing manifests to `bkt schema list` |
| Host-shims schema | Align with actual code structure           |
| Repo config path  | Resolve `repos.json` vs `repo.json`        |

---

## P4: Testing & Quality

| Task                      | Notes                              |
| ------------------------- | ---------------------------------- |
| CLI integration tests     | Cover all commands                 |
| PR workflow unit tests    | Verify git argument construction   |
| Manifest validation tests | Schema compliance                  |
| Doctor health checks      | PATH validation, shim verification |

---

## P5: Future Phases (Deferred)

### Phase 5+: Developer Experience

| Item              | Size | RFC      | Notes                        |
| ----------------- | ---- | -------- | ---------------------------- |
| `bkt init`        | L    | —        | New distribution scaffolding |
| Interactive TUI   | L    | —        | `--select` flag for capture  |
| Plugin subsystems | XL   | RFC-0028 | Extensibility                |

### Phase 5+: Automation

| Item                      | Size | RFC      | Notes                    |
| ------------------------- | ---- | -------- | ------------------------ |
| Changelog enforcement     | M    | RFC-0005 | CI checks for PR entries |
| MOTD integration          | S    | —        | First-boot "What's New"  |
| Scheduled upstream checks | M    | RFC-0006 | Auto-PR for updates      |

### Phase 5+: Multi-Environment

| Item                | Size | RFC      | Notes                          |
| ------------------- | ---- | -------- | ------------------------------ |
| Multi-machine sync  | XL   | —        | Sync manifests across machines |
| VM management       | L    | RFC-0030 | Windows VM workflow            |
| Windows VM workflow | L    | RFC-0031 | Steam Remote Play              |

---

## Migrated from HANDOFF.md

These items were in the archived HANDOFF.md and remain relevant:

| Item                              | Status | Notes                           |
| --------------------------------- | ------ | ------------------------------- |
| Ship `bkt` binary to host         | Done   | Available via distrobox exports |
| Toolchain policy decision         | Open   | Image-level vs home-managed     |
| `bkt status` distrobox visibility | Open   | Show distrobox manifest status  |

---

## Decision Queue

Items needing explicit decision before implementation:

| Decision           | Options                               | Leaning                |
| ------------------ | ------------------------------------- | ---------------------- |
| Host-only shims    | Implement / Defer / Retire RFC-0018   | Defer                  |
| Toolchain location | Image-baked / Home-managed            | Home-managed (current) |
| GSettings baseline | Ship with image / Create on first run | First run              |
| Override format    | Flatpak native / Normalize to JSON    | Flatpak native         |
