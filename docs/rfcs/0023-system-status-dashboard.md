# RFC 0023: System Status Dashboard

One-line summary: Provide a unified `bkt status` report for OS state, manifest drift, and suggested next actions.

## Motivation

Users need a single, fast command that summarizes system state and the configuration drift between manifests and the live system. This becomes the daily entry point for deciding whether to apply, capture, or update.

## Design

`bkt status` aggregates four categories of information and presents them in table or JSON format.

Command surface:

- `bkt status [--format table|json] [--verbose] [--skip-os] [--no-changelog]`.
- Table output is optimized for human scanning; JSON output is structured for scripts.

Report structure:

- **OS status**: current image reference, version, checksum, staged update, and layered packages.
- **Manifest status**:
  - Flatpaks: total, installed, pending, untracked.
  - Extensions: total, installed, enabled, and items to enable/disable/install-disabled plus untracked.
  - GSettings: total, applied, drifted.
  - Shims: total, synced.
  - Skel: total, differing files.
- **Drift summary**:
  - `pending_sync`: items that need to be applied from manifest to system.
  - `pending_capture`: untracked items present on the system.
- **Next actions**: prioritized suggestions such as applying manifests, capturing drift, releasing changelog entries, or booting into a staged update.

Changelog integration:

- When enabled, status reports pending changelog entries and notes whether drafts block release.

## Implementation Notes

- The implementation lives in [bkt/src/commands/status.rs](bkt/src/commands/status.rs).
- OS status is derived from `rpm-ostree status --json` and reads booted and staged deployments.
- Flatpak drift uses the installed list from `flatpak list --app` and compares against the merged manifest.
- Extension drift uses `gnome-extensions list --enabled` plus per-extension install checks.
- GSettings drift uses `gsettings get <schema> <key>` and compares to manifest values.
- Shims are considered synced if the shim file exists in the configured shims directory.
- Skel drift compares repository `skel/` files to `$HOME` and lists differing file names.
- Next actions are sorted by priority and omit sections that do not apply.

## Known Gaps

None.
