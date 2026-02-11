# RFC 0021: Local Change Management

One-line summary: Track local-only changes in an ephemeral manifest and provide `bkt local` commands to inspect, promote, or clear them.

## Motivation

Users often make quick, local adjustments without immediately creating a PR. These changes need a safe holding area that survives across commands but is not treated as durable system configuration. bkt also needs a way to promote those changes into the normal manifest and PR workflow when the user is ready.

## Design

Local changes are recorded in an ephemeral manifest when users run commands with `--local`. The `bkt local` command family exposes this tracking state and lets users promote or clear it.

Ephemeral manifest behavior:

- Stored at `~/.local/share/bkt/ephemeral.json`.
- Tied to the current boot ID, and automatically cleared when the boot ID changes (local changes do not survive into the next boot/image).
- Tracks changes by domain and identifier and deduplicates by `domain:identifier`.

Tracked domains and actions:

- Domains: `flatpak`, `extension`, `gsetting`, `shim`, `dnf`, `appimage`.
- Actions: `add`, `remove`, `update`.
- Each change stores a timestamp and optional domain-specific metadata (e.g., flatpak remote/scope, gsetting value, appimage repo/asset).

Command surface:

- `bkt local list [--format table|json] [--domain <name>]` shows tracked changes grouped by domain.
- `bkt local commit [--message <title>] [--domain <name>] [--select]` applies tracked changes to manifests, creates a commit, and opens a PR.
- `bkt local clear [--force]` deletes the ephemeral manifest entries but leaves installed items untouched.
- `bkt local path` prints the manifest file path.

Promotion to PR:

- Changes are grouped by domain and applied to the corresponding manifest files:
  - Flatpak apps, GNOME extensions, gsettings, shims, system packages, and appimage apps.
- bkt creates a branch, commits the manifest edits, pushes, and opens a PR via `gh`.
- After a successful PR, the committed changes are removed from the ephemeral manifest.

## Implementation Notes

- The ephemeral manifest is defined in [bkt/src/manifest/ephemeral.rs](bkt/src/manifest/ephemeral.rs).
- Boot ID validation reads `/proc/sys/kernel/random/boot_id` on Linux and clears stale manifests automatically.
- `EphemeralManifest::record` collapses multiple edits on the same item, and cancels inverse add/remove pairs to avoid no-op changes.
- `bkt local commit` uses a generated branch name (`bkt/local-commit-<timestamp>`), commits with `feat(manifests): ...`, pushes, and creates a PR with `gh pr create`.
- Manifest updates reuse existing manifest loaders and serializers and write JSON back to the repo manifest files.

## Known Gaps

- `bkt local commit --select` is parsed but not yet implemented; selection is currently ignored.
