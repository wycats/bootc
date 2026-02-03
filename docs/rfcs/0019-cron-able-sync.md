# RFC 0019: Cron-able Sync Command

- **Status**: Draft
- Feature Name: `sync_command`
- Start Date: 2026-02-03
- RFC PR: (leave this empty until PR is opened)
- Tracking Issue: (leave this empty)

## Summary

Rename `bkt apply` to `bkt sync` and establish performance axioms that make it safe to run on a 60-second cron. The command must be idempotent, near-instantaneous when nothing has changed, and fast when changes are needed.

## Motivation

### User Mental Model

Users think: "I changed something. I want it to work now."

They don't think in terms of "tier-1 vs tier-2" or "manifest vs system state." They want a single command that aligns manifest and reality:

```bash
bkt sync   # "Make it work"
```

### The Cron Requirement

For `bkt sync` to run automatically (e.g., every 60 seconds), it must satisfy:

| Property              | Requirement                    |
| --------------------- | ------------------------------ |
| **Idempotent**        | Running twice = running once   |
| **Fast no-op**        | < 100ms if nothing changed     |
| **Fast with work**    | < 2s for typical changes       |
| **Silent no-op**      | No output unless action taken  |
| **Safe to interrupt** | Ctrl-C leaves consistent state |

### Current State

The current `bkt apply` implementation:

- Shells out per-item for state checks (flatpak info, gnome-extensions info, gsettings get)
- No caching of system state between runs
- Always rewrites shims even if unchanged
- Always runs `distrobox assemble` even if container unchanged

This makes it too slow for cron (~5-10s even when nothing changed).

## Guide-level Explanation

### Basic Usage

```bash
# Make system match manifests (the common case)
bkt sync

# See what would change without doing it
bkt sync --check

# Force full sync even if cache says nothing changed
bkt sync --force

# Cron mode: silent unless changes made
bkt sync --quiet
```

### Cron Setup

```bash
# User cron (runs every minute)
* * * * * bkt sync --quiet

# Or via systemd user timer
systemctl --user enable bkt-sync.timer
```

### What Sync Does

`bkt sync` bidirectionally synchronizes all tier-2 domains:

| Domain     | Action                                |
| ---------- | ------------------------------------- |
| Flatpak    | Install/remove apps to match manifest |
| Extensions | Enable/disable GNOME extensions       |
| GSettings  | Apply settings values                 |
| Shims      | Generate host shims for toolbox       |
| Distrobox  | Export binaries, sync packages        |
| AppImage   | Install via GearLever                 |
| Homebrew   | Install/remove brew packages          |
| Skel       | Sync dotfiles                         |

Tier-1 changes (system packages) are **not** applied by sync—they require an image rebuild. Sync will report pending tier-1 changes:

```
bkt sync
  ✓ Flatpak: 2 apps installed
  ✓ Extensions: 1 enabled
  ⏳ System: 3 packages pending (run `bkt admin update` to rebuild)
```

### Terminology Changes

| Old                  | New                 | Rationale                            |
| -------------------- | ------------------- | ------------------------------------ |
| `bkt apply`          | `bkt sync`          | "Sync" implies ongoing bidirectional |
| `bkt capture`        | `bkt save`          | Explicit capture-only (no apply)     |
| `bkt <domain> apply` | `bkt <domain> sync` | Consistency                          |

### Bidirectional Sync: The Tier-2 Axiom

For tier-2 domains, **reality wins**. Unlike tier-1 (where the manifest is authoritative and requires a reboot), tier-2 sync is bidirectional:

| Direction          | Trigger                          | Action               |
| ------------------ | -------------------------------- | -------------------- |
| Reality → Manifest | Reality has item manifest lacks  | Capture to manifest  |
| Reality → Manifest | Reality removed item in manifest | Remove from manifest |
| Manifest → Reality | Manifest has item reality lacks  | Install/enable       |

#### Fixed-Point Guarantee

Sync must reach a **fixed point** where both directions agree. The algorithm:

```
1. Capture: Reality → Manifest (snapshot current system state)
2. Apply: Manifest → Reality (install any manifest-only items)
3. Result: Manifest = Reality (fixed point)
```

**Why capture first?**

- Respects "reality wins" — user's direct actions take precedence
- Prevents reinstalling something user just uninstalled
- After capture, apply only adds genuinely new manifest items

**No infinite loop because:**

- Capture only _adds_ to manifest (from reality) or _removes_ (if reality removed)
- Apply only _adds_ to reality (from manifest)
- One cycle guarantees: manifest ⊇ reality ∧ reality ⊇ manifest → manifest = reality

#### Three-Way Comparison

To distinguish "user removed from reality" from "user added to manifest", capture uses a **three-way comparison** against the last sync snapshot:

| Last Sync | Manifest | Reality | Interpretation         | Action                                |
| --------- | -------- | ------- | ---------------------- | ------------------------------------- |
| Had X     | Has X    | No X    | User removed X         | Remove from manifest                  |
| No X      | Has X    | No X    | User added to manifest | Keep in manifest (apply will install) |
| No X      | No X     | Has X   | User installed X       | Add to manifest                       |
| Had X     | Has X    | Has X   | No change              | Nothing                               |

The sync state cache stores the "last sync snapshot" for each domain, enabling this disambiguation.

#### Structural Intent (Distrobox Example)

Some domains use **structural intent** to distinguish "export everything here" from "export exactly this":

| Field         | Type           | Intent                              |
| ------------- | -------------- | ----------------------------------- |
| `export_path` | Directories    | "Export all binaries in these dirs" |
| `export_bins` | Explicit paths | "Export exactly these binaries"     |

This avoids glob-vs-explicit ambiguity:

- Capture preserves `export_path` directories (never expands to explicit list)
- Capture adds newly-discovered binaries to `export_bins`
- Apply exports union of (directory contents + explicit bins)

#### Examples

```bash
# User installs via GNOME Software
flatpak install org.gnome.Boxes
bkt sync  # Captures org.gnome.Boxes to manifest, nothing to apply

# User uninstalls via GNOME Software
flatpak uninstall org.gnome.Calculator
bkt sync  # Removes org.gnome.Calculator from manifest

# User adds to manifest manually
vim manifests/flatpak-apps.json  # adds org.gnome.Maps
bkt sync  # Installs org.gnome.Maps

# User installs cargo binary in toolbox
cargo install ripgrep  # in ~/.cargo/bin/
bkt sync  # Captures to export_bins (if not covered by export_path)
```

#### Conflict Detection

If both manifest and reality changed since last sync (rare edge case), sync reports the conflict rather than silently choosing:

```
bkt sync
  ⚠️ Conflict: org.gnome.Maps
     Manifest: added (commit abc123)
     Reality: not installed
     Resolution needed: --keep-manifest or --keep-reality
```

## Reference-level Explanation

### Performance Architecture

#### State Cache

```
~/.cache/bkt/
  sync-state.json     # Per-domain state hashes
  last-sync           # Timestamp of last successful sync
```

Example `sync-state.json`:

```json
{
  "flatpak": {
    "manifest_hash": "abc123",
    "system_hash": "def456",
    "last_check": "2026-02-03T12:00:00Z"
  },
  "shim": {
    "manifest_hash": "ghi789",
    "shims_dir_mtime": 1706961600
  }
}
```

#### Change Detection Tiers

| Tier  | Check                           | Cost   | When Used                 |
| ----- | ------------------------------- | ------ | ------------------------- |
| **0** | Manifest file mtime             | ~1ms   | Always first              |
| **1** | Manifest content hash vs cached | ~5ms   | If mtime changed          |
| **2** | System state sample             | ~50ms  | If manifest hash changed  |
| **3** | Full system state diff          | ~500ms | If sample indicates drift |

**Cron path** (typical): Tier 0 → cache hit → exit (~10ms)

**After manifest edit**: Tier 0 → Tier 1 → Tier 2 → sync (~500ms)

#### Domain-Specific Optimizations

##### Flatpak

- **Current**: `flatpak info <id>` per app (~100ms each)
- **Optimized**: Single `flatpak list --app --columns=application` → set compare (~200ms total)
- **Cache**: Store installed set hash, invalidate on `/var/lib/flatpak` mtime change

##### Extensions

- **Current**: `gnome-extensions info <id>` per extension
- **Optimized**: Single `gnome-extensions list --enabled` → set compare
- **Cache**: Store enabled set, invalidate on extension dir mtime

##### Shims

- **Current**: Delete all, rewrite all
- **Optimized**: Compare generated content hash per shim, skip unchanged
- **Cache**: Store `{name: content_hash}` map

##### Distrobox

- **Current**: Always run `distrobox assemble` + export all bins
- **Optimized**:
  - Skip assemble if INI unchanged AND container exists
  - Skip export if wrapper already exists with correct content
- **Cache**: Store INI hash + exported bins list

##### GSettings

- **Current**: `gsettings get` per setting
- **Optimized**: Single `dconf dump /` → parse and compare
- **Cache**: Store settings snapshot hash

#### Domain Ordering

Domains sync in dependency order:

```
distrobox assemble → distrobox export → shim sync
flatpak remotes → flatpak apps
extensions install → extensions enable
gsettings schemas (via extensions) → gsettings values
```

| Domain           | Depends On         | Reason                              |
| ---------------- | ------------------ | ----------------------------------- |
| Distrobox export | Distrobox assemble | Container must exist to export from |
| Shims            | Distrobox exports  | Shims wrap exported binaries        |
| Flatpak apps     | Flatpak remotes    | Apps install from remotes           |
| GSettings values | Extensions         | Extensions provide schemas          |
| Extension enable | Extension install  | Can't enable uninstalled extension  |

Parallel sync is only safe within a dependency tier.

### Sync Algorithm

```
fn sync(force: bool, quiet: bool) -> Result<Report>:
  report = Report::new()

  for domain in dependency_order([distrobox, shim, flatpak, extension, gsetting, ...]):
    if !force && domain.cache_valid():
      continue  # Fast path: nothing changed

    # Phase 1: Capture (Reality → Manifest)
    captured = domain.capture()  # Returns items added/removed from manifest
    if !captured.is_empty():
      report.add_captured(domain, captured)

    # Phase 2: Apply (Manifest → Reality)
    to_apply = domain.diff()  # Items in manifest but not reality
    if !to_apply.is_empty():
      if !quiet:
        report.add_applying(domain, to_apply)
      domain.apply(to_apply)

    domain.update_cache()

  if report.has_errors() && quiet:
    notify_error(report.errors())

  return report
```

### Exit Codes

| Code | Meaning                                    |
| ---- | ------------------------------------------ |
| 0    | Success (changes applied or nothing to do) |
| 1    | Error during sync                          |
| 2    | Dry-run: changes would be made             |

### Flags

| Flag             | Effect                            |
| ---------------- | --------------------------------- |
| `--check` / `-c` | Dry-run, exit 2 if changes needed |
| `--force` / `-f` | Skip cache, full state check      |
| `--quiet` / `-q` | No output unless error            |
| `--domain <d>`   | Sync only specified domain        |

### Error Surfacing Strategy

Cron mode (`--quiet`) requires layered error visibility:

| Layer            | Mechanism                 | Trigger   | Behavior               |
| ---------------- | ------------------------- | --------- | ---------------------- |
| **Journal**      | systemd-journald          | Always    | Logged automatically   |
| **Notification** | `notify-send`             | On error  | Desktop notification   |
| **State file**   | `~/.cache/bkt/last-error` | On error  | Persists until success |
| **Status**       | `bkt status`              | On demand | Shows last sync result |
| **Doctor**       | `bkt doctor`              | On demand | Includes sync health   |

**Quiet mode semantics:**

| Condition            | Output                     | Exit | Notification         |
| -------------------- | -------------------------- | ---- | -------------------- |
| No-op                | Silent                     | 0    | None                 |
| Success with changes | Silent                     | 0    | None                 |
| Error                | Silent (stderr to journal) | 1    | Desktop notification |

**Error state file:**

```json
{
  "timestamp": "2026-02-03T12:00:00Z",
  "domain": "flatpak",
  "error": "Failed to install org.gnome.Maps: remote not configured",
  "context": { "remote": "flathub", "app": "org.gnome.Maps" }
}
```

Successful sync clears `last-error`. `bkt status` reports "Last sync: OK" or "Last sync: FAILED (domain: error)".

## Drawbacks

1. **Cache invalidation complexity**: Caches can become stale if system is modified outside `bkt`
2. **Migration effort**: Renaming `apply` → `sync` requires updating docs, scripts, muscle memory
3. **Performance work**: Significant refactoring needed for domain optimizations

## Rationale and Alternatives

### Why "sync" not "apply"?

- `sync` implies bidirectional awareness (check + apply)
- `sync` implies idempotency (sync again = same result)
- `sync` is the standard term (rsync, Dropbox, cloud sync)
- `apply` implies one-time action (apply a patch)

### Why not separate check and apply?

We considered:

```bash
bkt check   # See what's different
bkt apply   # Make changes
```

But this doubles the work for the common case. `bkt sync` combines both with smart caching.

### Why not per-domain crons?

We considered separate timers per domain, but:

- More complexity to configure
- Domains have dependencies (distrobox before shims)
- Single sync is easier to reason about

## Prior Art

- **Nix/Home Manager**: `home-manager switch` is the unified sync command
- **Ansible**: `ansible-playbook` with `--check` for dry-run
- **Terraform**: `terraform apply` with plan caching
- **rsync**: Fast no-op via mtime/size checks

## Unresolved Questions

1. **Cache TTL**: Should caches expire after N hours even if mtime unchanged?
2. **Parallel sync**: Can domains sync in parallel, or must they be sequential?

## Appendix: Systemd Timer Units

### bkt-sync.service

```ini
[Unit]
Description=Sync bkt manifests to system state
Documentation=man:bkt(1)

[Service]
Type=oneshot
ExecStart=/usr/local/bin/bkt sync --quiet
# Don't fail the unit if nothing to do
SuccessExitStatus=0 2
```

### bkt-sync.timer

```ini
[Unit]
Description=Run bkt sync every minute
Documentation=man:bkt(1)

[Timer]
OnBootSec=30s
OnUnitActiveSec=60s
# Randomize to avoid thundering herd on multi-user systems
RandomizedDelaySec=5s

[Install]
WantedBy=timers.target
```

### Installation

```bash
# Install units
mkdir -p ~/.config/systemd/user
cp bkt-sync.{service,timer} ~/.config/systemd/user/

# Enable and start
systemctl --user daemon-reload
systemctl --user enable --now bkt-sync.timer

# Check status
systemctl --user status bkt-sync.timer
journalctl --user -u bkt-sync.service -f
```

## Future Possibilities

1. **`bkt watch`**: Filesystem watcher that syncs on manifest change
2. **`bkt sync --daemon`**: Long-running process with inotify
3. **Conflict detection**: Warn if system state diverged from both manifest AND cache
4. **Rollback**: `bkt sync --to <commit>` to sync to historical manifest state
