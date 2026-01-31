# Changelog

All notable changes to this distribution are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project uses [Calendar Versioning](https://calver.org/) in the format `YYYY.MM.DD.N`.

<!-- changelog entries start -->

## 2026.01.30 — Phase 4 Progress

### Added

- **fetchbin crate**: Binary acquisition from npm, cargo-binstall, and GitHub releases (52 tests)
- **Distrobox live capture**: `bkt distrobox capture --live` introspects running containers
- **Subsystem execution phases**: Ordered execution via RFC-0029
- **Dev/System command split**: `bkt dev` + `bkt system` per RFC-0020

### Fixed

- `bootc-apply`: Changed broken `bkt dnf sync` → `bkt system capture --apply`
- `bootc-bootstrap`: Changed broken `shim sync` → `bkt shim sync`
- `docs/ARCHITECTURE.md`: Fixed export location `/usr/bin` → `~/.local/bin/distrobox`

### Changed

- Consolidated planning docs: CURRENT.md + NEXT.md now single source of truth
- Archived HANDOFF.md to `docs/history/2026-01-20-distrobox-handoff.md`
