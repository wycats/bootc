# Current State

_Last updated: 2026-02-26_

One-sentence: this repo extends atomic Linux (bootc/ostree) into the user layer —
making user environment (Flatpaks, extensions, gsettings, dev tools) declarative,
reproducible, and recoverable.

---

## What Just Happened (Feb 23–26, 2026)

Major architectural changes landed. Read these to understand the current state:

### Manifest Architecture (RFC-0052, canon)

The repo is now the **single source of truth** for all subsystems. The old
system+user manifest merge layer (`~/.config/bootc/`) was eliminated. Every
subsystem reads from and writes to `manifests/` in the repo. ~2,700 lines of
shadow-state code removed. See [RFC-0052](docs/rfcs/canon/0052-manifest-lifecycle.md).

### Change Workflow (RFC-0054)

The `--local` flag and ephemeral manifest are gone. Changes write directly to
the repo's `manifests/` directory. `git diff manifests/` shows what changed.
The default PR mode is now "no automatic PR" — there's a known gap where
`bkt pr` (batch PR submission) doesn't exist yet. See [RFC-0054](docs/rfcs/0054-change-workflow.md).

### Bootstrap & Repo Discovery (RFC-0053)

`find_repo_path()` now caches the repo path at `~/.local/state/bkt/repo-path`.
The bootstrap script clones the repo on first login. See [RFC-0053](docs/rfcs/0053-bootstrap-and-repo-discovery.md).

### Containerfile / Layer Architecture

Per-package RPM install stages (RFC-0045) with a `collect-outputs` stage that
merges upstream fetches, config, and wrappers into a single OCI layer. External
RPM install stages use `COPY` (not `COPY --link`) to avoid a btrfs xattrs
hardlink limit. See [investigation](docs/investigations/2026-02-26-btrfs-xattrs-hardlink-limit.md)
for the debugging details.

### Other

- `bkt doctor --fix` — auto-remediates distrobox shim issues
- `bkt tune memory` — RAM/swap/GPU analysis
- `bkt migrate manifests` — migrates legacy `~/.config/bootc/` files into repo

### Current Phase

Run `exo status` for the current phase, goals, and tasks. Run `exo idea list`
for captured future work including the Mac VM multi-machine support (semi-urgent).

---

> **If you're an agent reading this**: update this "What Just Happened" section
> when you complete significant work. Keep it short — just enough for the next
> agent to understand the current momentum. Delete entries older than ~2 weeks.
