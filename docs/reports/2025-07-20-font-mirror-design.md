# Design update: composefs font workaround (cache clear, not mirror)

## Summary

The original workaround mirrored fonts from composefs to btrfs to bypass a
suspected mmap/sandbox bug. The root cause is now confirmed as **stale
fontconfig user caches**, so the mirror is unnecessary. The correct and much
lighter fix is to clear the user cache and rebuild it on each OS deployment
change.

## Old Approach (No Longer Needed)

- Copy ~100MB (or more) of system fonts from composefs to btrfs
- `rsync /usr/share/fonts/ -> ~/.local/share/fonts/composefs-mirror/`
- `fc-cache` to prioritize the user fonts

This worked only because the mirror created fresh cache entries with new paths.
It does not address the real problem and adds persistent storage overhead.

## New Approach (Correct Fix)

Clear fontconfig user caches and rebuild them after each deployment change:

```
rm -rf ~/.cache/fontconfig/*
fc-cache -f
```

This resolves the issue directly by removing stale, cross-version cache files
that fontconfig still reads (e.g. `cache-reindex1-10` alongside `cache-9` and
`cache-11`). Once the cache is rebuilt, Chromium/Electron render non-Latin fonts
correctly from the composefs system fonts.

## Implementation Applied in bootc-bootstrap

The current implementation lives in [scripts/bootc-bootstrap](scripts/bootc-bootstrap)
under `apply_composefs_font_workaround()` and is active in the runtime login
path. Behavior summary:

1. Detect composefs root by `stat -f -c '%T' /` (overlay/overlayfs)
2. Gate by ostree deployment checksum (`rpm-ostree status --json`)
3. On deployment change:
   - `rm -rf ~/.cache/fontconfig`
   - `mkdir -p ~/.cache/fontconfig`
   - `fc-cache -f`
4. Remove the legacy mirror if present (`~/.local/share/fonts/composefs-mirror`)
5. Persist the deployment checksum marker to avoid repeat work

This approach is lightweight, correct, and does not copy any font data.

## Evidence Summary

- Same composefs font file rendered ASCII glyphs but zero pixels for Kannada
- Pango + FreeType rendered Kannada from composefs correctly (829 pixels)
- Clearing the fontconfig cache fixed all scripts and emoji in Chromium
- `fc-cache -f` alone did not fix it because old-format caches persisted

## Recommendation

- Keep the cache-clear workaround as the standard fix for composefs systems
- Do not reintroduce the font mirror unless new evidence emerges
- Track this as a deployment-time maintenance step, not a build-time action# Design: composefs Font Mirror — Placement and bkt Integration

## Decision: Runtime, Not Build-Time

The font mirror **must** run at runtime (first login after image update). Build-time
is ruled out for three independent reasons:

| Approach                     | Why It Fails                                                                                       |
| ---------------------------- | -------------------------------------------------------------------------------------------------- |
| `RUN rsync` in Containerfile | Destination is still on composefs — same overlay, same bug                                         |
| Seed `/var` in Containerfile | ostree: `/var` content from the image only seeds first deployment; never updated on image upgrades |
| `tmpfiles.d` copy rule       | `C` directive only copies if absent; doesn't re-sync on image update; no deployment gating         |

**The workaround only works when font data lives on a different filesystem** (btrfs,
ext4 — anything that isn't the composefs overlay). The user's home directory
(`~/.local/share/fonts/`, on btrfs) is the natural target.

## Current Implementation (Already Correct)

`apply_composefs_font_workaround()` in [scripts/bootc-bootstrap](../../scripts/bootc-bootstrap)
is well-designed:

```
                       ┌──────────────────┐
                       │ bootc-bootstrap  │
                       │   (systemd user  │
                       │    oneshot)       │
                       └────────┬─────────┘
                                │
                    ┌───────────┴───────────┐
                    │ apply_composefs_      │
                    │ font_workaround()     │
                    └───────────┬───────────┘
                                │
                    ┌───────────┴───────────┐
              ┌─────┤ Is root composefs?    │
              │     └───────────┬───────────┘
              │ no              │ yes
              │     ┌───────────┴───────────┐
              │     │ Has deployment changed│
              │     │ since last sync?      │
              │     └───────────┬───────────┘
              │           │ no  │ yes
              │           │     │
              │           │  ┌──┴───────────┐
              │           │  │ rsync + fc-  │
              │           │  │ cache        │
              │           │  └──────────────┘
              │           │
              └───────────┘ (skip)
```

**Key design properties:**

- **Filesystem detection**: `stat -f -c '%T' /` → "overlay"
- **Deployment gating**: `rpm-ostree status --json | jq .deployments[0].checksum`
- **Idempotency**: Marker file at `~/.local/state/bootc-bootstrap/font-mirror-deployment`
- **Placement**: Runs in `apply_all()` _outside_ the manifest hash gate (because
  it's gated on deployment, not manifest content)

## Should This Be a bkt Subsystem?

**No.** The font mirror doesn't fit the `Subsystem` trait:

| Subsystem Concept                      | Font Mirror Reality                    |
| -------------------------------------- | -------------------------------------- |
| `load_manifest()` → declarative intent | No manifest — it's a blanket `rsync`   |
| `capture()` → system → manifest        | Nothing to capture (no manifest)       |
| `sync()` → manifest → system           | Not manifest-driven; deployment-driven |
| `drift()` → diff manifest vs system    | No meaningful drift concept            |

The bkt subsystem model is for _declarative configuration management_: "I declare
what I want (in JSON), bkt makes it so." The font mirror is a _platform
workaround_: "detect a broken ecosystem interaction, compensate." These are
architecturally different.

## What bkt SHOULD Know About

### 1. `bkt doctor` check

`bkt doctor` should detect and report the composefs font issue:

```
$ bkt doctor
✓ system packages: 42 installed, 0 missing
✓ flatpak apps: 15 installed, 0 missing
⚠ composefs font mirror: stale (deployment changed since last sync)
✓ composefs font mirror: synced (deployment abc123def456)
✗ composefs font mirror: not present (composefs detected, run bootc-bootstrap apply)
```

This would be a **diagnostic check**, not a subsystem. It reads:

- `stat -f -c '%T' /` to detect composefs
- `~/.local/state/bootc-bootstrap/font-mirror-deployment` marker
- `rpm-ostree status` for current deployment

### 2. `bkt status` reporting (optional)

If desired, the font mirror status could appear in `bkt status` output alongside
subsystem statuses, as a special "platform workaround" section:

```
$ bkt status
Subsystems:
  extension   12 synced, 0 pending
  flatpak     15 synced, 0 pending
  gsetting     8 synced, 0 pending
  shim         7 synced, 0 pending

Platform:
  composefs-font-mirror  ✓ synced (deployment abc123...)
```

## Future Enhancement: System-Level Service

The current implementation runs per-user on login. A cleaner long-term approach
would be a system-level oneshot service that:

1. Creates `/var/cache/bootc-fonts/` (on the real filesystem, not composefs)
2. rsyncs `/usr/share/fonts/` → `/var/cache/bootc-fonts/`
3. Ships a fontconfig snippet (`/etc/fonts/conf.d/01-composefs-font-mirror.conf`)
   that adds `/var/cache/bootc-fonts/` with high priority

**Advantages:**

- Runs at boot, before any user session (fonts ready immediately)
- Single copy for all users (saves ~542MB per additional user)
- System-level is semantically correct for system font workaround

**Trade-offs:**

- Requires a fontconfig snippet in the image (Containerfile change)
- Requires a new systemd system service (Containerfile + systemd/ change)
- More complex gating (system service can't use `rpm-ostree status --json`
  directly — needs to read `/ostree/repo` or `/sysroot` deployment info)

**Recommendation:** Keep the current per-user approach for now. It works, it's
already implemented, and the bug reports may produce an upstream fix that
eliminates the need entirely. If this workaround persists long-term (>6 months
without upstream fix), migrate to the system-level approach.

## Summary of Recommendations

| Item                 | Action                                                 | Priority |
| -------------------- | ------------------------------------------------------ | -------- |
| Font mirror function | ✅ Already implemented correctly in bootc-bootstrap    | Done     |
| Placement (runtime)  | ✅ Correct — build-time is impossible                  | Done     |
| bkt subsystem        | ❌ Don't create one — wrong abstraction                | N/A      |
| `bkt doctor` check   | 🟡 Add a composefs font mirror diagnostic              | Medium   |
| `bkt status` line    | 🟡 Optional; add if doctor check proves useful         | Low      |
| System-level service | 🔵 Defer unless workaround needed long-term            | Future   |
| Bug reports          | 🔴 File both (Chromium + kernel) to drive upstream fix | High     |
