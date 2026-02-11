# RFC 0022: Homebrew Package Management

One-line summary: Manage Linuxbrew/Homebrew packages declaratively through system and user manifests with `bkt homebrew`.

## Motivation

The system needs a declarative way to track Homebrew formulae, similar to other manifest-driven subsystems. A merged system+user manifest allows baseline packages to be baked into images while still enabling personal additions. bkt provides commands that manipulate these manifests and apply changes to the host.

## Design

Homebrew is managed via `homebrew.json` manifests and a dedicated command surface.

Manifest format:

- `homebrew.json` contains `formulae` and optional `taps`.
- Each formula entry can be a simple string or a full object:
  - Simple: `"lefthook"`
  - Full: `{ "name": "valkyrie00/bbrew/bbrew", "tap": "valkyrie00/bbrew" }`
- System manifest path: `/usr/share/bootc-bootstrap/homebrew.json`.
- User manifest path: `~/.config/bootc/homebrew.json`.
- Merged view: user formulae override system formulae by name, and taps are merged and deduplicated.

Command surface:

- `bkt homebrew add <formula>` adds a formula to the user manifest.
- `bkt homebrew remove <formula>` removes a formula from the user manifest.
- `bkt homebrew list [--format table|json]` lists the merged view.
- `bkt homebrew sync` installs missing formulae and adds missing taps based on the merged manifest.
- `bkt homebrew capture` records explicitly installed formulae (leaves) into the user manifest.

Runtime behavior:

- Sync uses `brew list --formula -1` to detect installed formulae and `brew tap` to add missing taps.
- Capture uses `brew leaves -r` to gather explicit installs (excluding dependencies).
- The command domain is host-only.

## Implementation Notes

- Manifest types and merge logic live in [bkt/src/manifest/homebrew.rs](bkt/src/manifest/homebrew.rs).
- The command implementation is in [bkt/src/commands/homebrew.rs](bkt/src/commands/homebrew.rs).
- `HomebrewManifest::merged` sorts formulae and deduplicates taps for consistent output.
- `BrewFormula::tap()` infers a tap from `user/repo/formula` when provided as a simple string.
- Sync and capture are implemented as `Plan` executions with a summarized action list and a report of successes/failures.

## Known Gaps

None.
