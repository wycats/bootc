# Fontconfig User Cache Staleness on Atomic Linux

> **Status:** Resolved locally; upstream gap identified
> **Date:** 2025-07-20 (investigation), 2026-02-10 (web research update)
> **Observed on:** Bazzite 43 (Fedora 43, bootc/ostree with composefs)
> **Likely affects:** Other Fedora Atomic variants (Bluefin, Aurora, Silverblue, Kinoite) and ostree/bootc systems generally — downstream reports from Silverblue, Bluefin, and Bazzite confirm similar symptoms

## Summary

Chromium and Electron apps on image-based Linux systems fail to render non-Latin
system fonts (complex scripts, emoji). The root cause is **stale fontconfig user
cache files** in `~/.cache/fontconfig/` that persist across atomic OS
deployments. This is not a data-corruption bug in composefs, not specific to
Chromium, and not a kernel bug — it is a gap in the lifecycle management of
user-level caches on atomic Linux distributions. However, composefs/ostree's
mtime semantics are central to _why_ fontconfig's normal invalidation fails.

**Fix:** `rm -rf ~/.cache/fontconfig/* && fc-cache -f` after each deployment change.

## The Problem

### Symptom

After an OS upgrade on an atomic/ostree system, Chromium-based browsers and
Electron apps render Latin text normally but fail on:

- Complex scripts (Kannada, Arabic, Devanagari, CJK)
- Emoji
- Any glyph whose rendering depends on OpenType Layout (OTL) metadata

`document.fonts.check()` returns `true` (fontconfig finds the font), but canvas
rendering produces zero pixels for affected glyphs.

### Root Cause

Fontconfig caches font metadata in `~/.cache/fontconfig/`. The exact contents of
the cache include charset coverage and various font properties (the full schema
is internal to fontconfig and not well-documented externally). This cache uses
**mtime-based invalidation**: if a font directory's mtime hasn't changed, the
cache is considered valid.

On atomic/image-based systems, OS upgrades replace the entire `/usr` atomically.
Font files in `/usr/share/fonts/` are swapped wholesale, but:

1. **ostree/composefs sets mtime to epoch (0)** on all files and directories in
   the deployed image. Verified directly:

   ```
   $ stat /usr/share/fonts/
   Access: 1969-12-31 16:00:00.000000000 -0800
   Modify: 1969-12-31 16:00:00.000000000 -0800
   Change: 1969-12-31 16:00:00.000000000 -0800
   ```

   This is standard erofs/composefs behavior (see also fontconfig#44, which
   documents the same issue on squashfs). The previous deployment also had
   mtime=0. So fontconfig sees `0 == 0` and considers the cache fresh.

2. **The user cache persists across deployments** in `~/.cache/fontconfig/`
   (home directory is on a separate, mutable filesystem — typically btrfs).
   System-level caches in `/usr/lib/fontconfig/cache/` are baked into the image
   and expected to be correct (generated at image build time by `fc-cache`), but
   user caches are never invalidated.

3. **Cross-version cache files accumulate.** When fontconfig's cache format
   version changes (e.g., cache-9 → cache-11), `fc-cache -f` generates new
   files but **does not delete old-format files**. Verified directly — after
   clearing the cache and running fc-cache + Edge, the user cache accumulated
   **118 files** across 3 format variants for ~39 font directories:
   ```
   $ ls ~/.cache/fontconfig/ | grep -oP 'cache-\S+' | sort | uniq -c | sort -rn
        78 cache-9    # Edge's bundled fontconfig (39 le32d4 + 39 le64)
        39 cache-11   # System fontconfig 2.17.0 (le64 only)
   ```
   Meanwhile, the system cache baked into the image has only cache-9 (74 files),
   suggesting the image was built with an older fontconfig version. Edge's
   bundled fontconfig generates cache-9 files that coexist with the system's
   cache-11 files. Neither version cleans up the other's files.

### Why Latin Works But Complex Scripts Fail

**Hypothesis (not independently verified):** Latin glyphs use basic cmap lookups
that may work even with partially stale cache metadata. Complex scripts depend
on additional font metadata (shaping capabilities, script coverage) that
fontconfig caches. When these cached properties are stale or missing, the font
may be excluded from consideration for complex-script rendering.

What we _know_ empirically: the same font file, on the same filesystem, renders
ASCII glyphs but produces zero pixels for Kannada glyphs. Clearing the cache
fixes both. The exact mechanism by which stale cache metadata causes this
discrimination has not been traced through fontconfig/Pango/Chromium source code.

## Evidence Trail

### Local Testing (July 2025)

All tests performed on Bazzite 43 (Fedora 43), Edge 144.0.3719.115, fontconfig
2.17.0, composefs root.

1. **Same font, different glyphs:** From a single composefs
   `NotoSansKannada-Regular.ttf`, ASCII glyphs rendered (period=32px, zero=296px)
   while ALL Kannada glyphs rendered as zero pixels — including Kannada digits
   that are in the same font file.

2. **FreeType renders from composefs:** Python (Cairo + Pango, FreeType backend)
   rendered Kannada from the same composefs font at 829 non-zero pixels. The
   font data is accessible and correct on composefs.

3. **mmap data is correct:** Python mmap tests returned byte-identical SHA-256
   hashes between `read()` and `mmap()` on composefs fonts. No data corruption.

4. **Sandbox is not the issue:** `--no-sandbox` produced identical failures.

5. **Fontations flag is irrelevant:** `--disable-features=Fontations` had no
   effect (flag was already removed upstream).

6. **Cache clear fixes everything:**

   ```
   rm -rf ~/.cache/fontconfig/*
   fc-cache -f
   ```

   After clearing: Kannada=247, Arabic=212, Emoji=1412, NotoSans=797, Inter=826.
   All non-Latin fonts rendered correctly from composefs. No filesystem changes.

7. **Why mirror/symlink workaround worked:** Copying fonts to new paths created
   fresh cache entries with no stale data. The mirror was never necessary — it
   accidentally bypassed the stale cache, not a filesystem bug.

### Disproven Hypotheses

| Hypothesis                        | Test                                     | Result                  |
| --------------------------------- | ---------------------------------------- | ----------------------- |
| composefs mmap returns wrong data | Python mmap vs read comparison           | Byte-identical SHA-256  |
| Chromium sandbox blocks composefs | `--no-sandbox` flag                      | Same failures           |
| Fontations backend bug            | FreeType renders same font fine          | Not Fontations-specific |
| Font format/table access issue    | Table hash comparison btrfs vs composefs | All hashes match        |

## Upstream Landscape

### Fontconfig's Position

**[fontconfig#444](https://gitlab.freedesktop.org/fontconfig/fontconfig/-/issues/444)**
(Jan 2025) is the canonical upstream issue. Key statements from the maintainer
(Akira Tagoh, `@tagoh`):

> "We don't want to waste the runtime footprint by updating caches
> automatically. fc-cache is a separate tool to avoid such situation and **are
> supposed to be run before bringing up applications. We leave it to the
> distributions and packagers.**"

> "The most concern on bumping the cache version so often is that **we can't
> touch on cleaning up old caches.** The total size of the cache files isn't
> trivial. If we do clean up them to avoid the disk full, Applications which
> still use old version of fontconfig will be getting slower because they will
> need to create caches at the runtime. Also **we can't make a choice because
> there are no way to know at our side if old cache files are still in use or
> not.**"

> "there are some exception on it for flatpak/os-tree based environment
> (**which is 0-mtime on files/directories**)"

The maintainer explicitly acknowledges that ostree's 0-mtime breaks
fontconfig's invalidation model and considers it a distro-level responsibility.
The issue was closed.

Jan Alexander Steffens (Arch Linux maintainer, `@heftig`) identified the gap:

> "**User-level cache files are the problem.** In the case of Electron, it
> generates these for all system fonts as well."

> "This is commonly done for the system cache but **I don't think anyone
> handles user caches.**"

### Related Fontconfig Issues

| Issue                                                                     | Title                                                          | Relevance                                                                                                                                                                                                                                                                                                                                            |
| ------------------------------------------------------------------------- | -------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| [#444](https://gitlab.freedesktop.org/fontconfig/fontconfig/-/issues/444) | Cache version was not increased when font wrappers were added  | **Closely related.** Chromium's bundled fontconfig snapshot (cache version 9) generates cache files missing `FC_FONT_WRAPPER` that newer fontconfig (2.15+) still reads. Demonstrates the same class of cross-version cache corruption, though the specific triggering mechanism (Electron's bundled fontconfig vs. ostree mtime) differs from ours. |
| [#330](https://gitlab.freedesktop.org/fontconfig/fontconfig/-/issues/330) | Cache version bump breaks Flatpak apps                         | Cache version mismatch between host and Flatpak runtime. Flatpak mounts host caches.                                                                                                                                                                                                                                                                 |
| [#329](https://gitlab.freedesktop.org/fontconfig/fontconfig/-/issues/329) | fc-match crashes with refreshed toolbox system cache on ostree | Cache corruption from symlinked host fonts in toolbox on ostree.                                                                                                                                                                                                                                                                                     |
| [#44](https://gitlab.freedesktop.org/fontconfig/fontconfig/-/issues/44)   | fontconfig requires nano second precision for fontcache        | squashfs (like composefs/erofs) lacks nanosecond mtime; cache never invalidates. Ubuntu patched this for live ISOs in 2018.                                                                                                                                                                                                                          |
| [#45](https://gitlab.freedesktop.org/fontconfig/fontconfig/-/issues/45)   | X11 fc-cache and XQuartz fc-cache disagree                     | Two fontconfig versions fighting over shared cache directory. Same architectural flaw — no cross-version cache cleanup. Closed without fix.                                                                                                                                                                                                          |

### Downstream Reports (Same Root Cause)

Multiple ostree-based distros independently hit this and converged on
`rm -rf ~/.cache/fontconfig`:

| Issue                                                                           | Distro       | Symptom                                                                                                                           |
| ------------------------------------------------------------------------------- | ------------ | --------------------------------------------------------------------------------------------------------------------------------- |
| [silverblue#534](https://github.com/fedora-silverblue/issue-tracker/issues/534) | Silverblue   | Noto CJK broken in Flatpak. Notes mixed cache versions (cache-7/8/9).                                                             |
| [silverblue#540](https://github.com/fedora-silverblue/issue-tracker/issues/540) | Silverblue   | International fonts broken with Chrome + toolbox.                                                                                 |
| [bluefin#1558](https://github.com/ublue-os/bluefin/issues/1558)                 | Bluefin      | "deleted contents of `.cache/fontconfig` and my issue is resolved… not cleaned by fc-cache."                                      |
| [bazzite#864](https://github.com/ublue-os/bazzite/issues/864)                   | Bazzite      | Missing fonts. Fix: `rm ~/.cache/fontconfig`.                                                                                     |
| [distrobox#358](https://github.com/89luca89/distrobox/issues/358)               | Cross-distro | "fontconfig caches are not compatible across distributions… updating the cache inside the container will corrupt the host cache." |

**Chromium/Electron-specific reports:**

| Issue                                                                               | App              | Key detail                                                                               |
| ----------------------------------------------------------------------------------- | ---------------- | ---------------------------------------------------------------------------------------- |
| [flathub/chromium#280](https://github.com/flathub/org.chromium.Chromium/issues/280) | Chromium Flatpak | "fontconfig relies on mtime… on ostree-based systems… outdated caches still keep alive." |
| [flathub/chrome#202](https://github.com/flathub/com.google.Chrome/issues/202)       | Chrome Flatpak   | CJK failures on Silverblue.                                                              |
| [flathub/edge#299](https://github.com/flathub/com.microsoft.Edge/issues/299)        | Edge Flatpak     | Chinese fonts as boxes on Silverblue.                                                    |

### Existing Fixes (and Their Gaps)

| Fix                                                                                      | Scope                                                 | What it misses                                                                                                                                                                                                                                                                                              |
| ---------------------------------------------------------------------------------------- | ----------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| [Bluefin PR #1560](https://github.com/ublue-os/bluefin/pull/1560) (Aug 2024)             | Flatpak app caches (`~/.var/app/*/cache/fontconfig/`) | Host user cache for native (RPM) apps                                                                                                                                                                                                                                                                       |
| [freedesktop-sdk#1576](https://gitlab.com/freedesktop-sdk/freedesktop-sdk/-/issues/1576) | Flatpak runtime cache seed                            | Flatpak-internal; doesn't touch host                                                                                                                                                                                                                                                                        |
| [Bazzite #1178](https://github.com/ublue-os/bazzite/issues/1178) `bazzite-user-setup`    | Clears `~/.config/fontconfig`                         | **Appears to target the wrong directory.** Per the issue, the script clears `$HOME/.config/fontconfig` — the user config dir, not the cache dir (`~/.cache/fontconfig/`). If this reading is correct, it would not address cache staleness. (Verify by inspecting the current `bazzite-user-setup` script.) |

**Nobody handles the host user cache (`~/.cache/fontconfig/`) for native
(non-Flatpak) apps on ostree systems.** Every existing fix targets the Flatpak
sandbox's view of the cache, not the user's actual cache directory.

## The Composition Gap

### Why This Doesn't Happen on Traditional Fedora

On traditional (mutable) Fedora:

1. `dnf install google-noto-sans-kannada-fonts` runs RPM's `%post` scriptlet
2. The scriptlet calls `fc-cache` which regenerates the system cache
3. Font files get real timestamps from the package install time
4. The user cache naturally invalidates because font directory mtimes changed

The problem never surfaces because **font updates are incremental and produce
real timestamps.**

### Why Atomic Systems Break This

On Fedora Atomic (Bazzite, Bluefin, etc.):

1. The entire `/usr` is replaced atomically as a new deployment
2. **No RPM scriptlets run at deploy time** — the system cache baked into the
   image is correct, but no user-level cleanup happens
3. ostree/composefs uses **epoch (0) timestamps** on all files — fontconfig's
   mtime comparison sees no change
4. The user cache was generated against the _previous_ deployment's fonts and
   **nobody tells it things have changed**

### Who Owns User-Level Post-Deployment Lifecycle?

**Nobody.** This is the fundamental gap:

```
Image built (system fc-cache runs here — system cache correct)
  → ostree deploys image
  → User reboots
  → ??? nobody invalidates user-level caches ???
  → User logs in with stale derived data from the previous /usr
```

| Component     | What it provides                                | Gap                                                                       |
| ------------- | ----------------------------------------------- | ------------------------------------------------------------------------- |
| ostree/bootc  | `ostree-finalize-staged.service` (system-level) | No user-level post-deployment hooks                                       |
| Fedora Atomic | `rpm-ostreed-automatic.timer` (system updates)  | No user-level cache invalidation                                          |
| Bazzite       | `bazzite-user-setup`                            | Targets wrong directory (`~/.config/fontconfig` vs `~/.cache/fontconfig`) |
| fontconfig    | `fc-cache`                                      | Relies on mtime (broken on ostree); doesn't clean old-format files        |
| Flatpak       | Various cache fixes                             | Only covers Flatpak sandbox, not host user cache                          |

### Is This Just a Fontconfig Problem?

We surveyed other user-level caches derived from `/usr` to determine whether the
mtime-based invalidation failure extends beyond fontconfig:

| Cache                    | Location                                                  | Invalidation                                           | mtime=0 vulnerable?                                                      |
| ------------------------ | --------------------------------------------------------- | ------------------------------------------------------ | ------------------------------------------------------------------------ |
| **fontconfig**           | `~/.cache/fontconfig/`                                    | mtime comparison                                       | **YES — confirmed broken**                                               |
| **GTK icon-theme.cache** | `~/.local/share/icons/*/icon-theme.cache`                 | mtime comparison                                       | **Likely** — user-space icon caches exist, system icon dirs have mtime=0 |
| **Mesa shader cache**    | `~/.cache/mesa_shader_cache/`                             | Content hash (shader source + driver version + GPU ID) | **No** — not mtime-dependent                                             |
| **MIME cache**           | `/usr/share/mime/mime.cache` (mtime=0)                    | No user-level cache mirrors it                         | **N/A** — system-only                                                    |
| **GLib schemas**         | `/usr/share/glib-2.0/schemas/gschemas.compiled` (mtime=0) | System-only, no user mirror                            | **N/A**                                                                  |
| **GDK pixbuf loaders**   | `/usr/lib64/gdk-pixbuf-2.0/.../loaders.cache` (mtime=0)   | System-only, no user mirror                            | **N/A**                                                                  |
| **ldconfig**             | `/etc/ld.so.cache`                                        | Real mtime (mutable `/etc`)                            | **No** — `/etc` is writable                                              |

**Finding:** The "class of problem" is narrower than initially hypothesized.
Fontconfig is the clearest and most impactful case. GTK icon-theme.cache is a
plausible secondary case (user-space icon caches exist at
`~/.local/share/icons/` and system icon directories have mtime=0). Other caches
are either content-keyed (Mesa), system-only with no user mirror (MIME, GLib,
pixbuf), or on mutable filesystems (ldconfig on `/etc`).

The root issue — **user-level caches that use mtime to validate against
immutable system directories with epoch timestamps** — is real but currently
confirmed only for fontconfig, with icon caches as a plausible second case.

## Our Fix: bootc-bootstrap

`apply_composefs_font_workaround()` in
[scripts/bootc-bootstrap](../../scripts/bootc-bootstrap) handles this:

1. **Detect composefs root:** `stat -f -c '%T' /` → "overlay"/"overlayfs"
2. **Gate on deployment change:** Compare `rpm-ostree status --json |
.deployments[0].checksum` against a marker file
3. **On deployment change:**
   - `rm -rf ~/.cache/fontconfig`
   - `mkdir -p ~/.cache/fontconfig`
   - `fc-cache -f`
4. **Remove legacy font mirror** if present (from the previous rsync workaround)
5. **Write marker file** with current deployment checksum

This runs as part of the systemd user oneshot `bootc-bootstrap.service`, outside
the manifest hash gate (because it's gated on deployment checksum, not manifest
content).

## Broader Implications for bkt

The user-level post-deployment lifecycle gap is a potential area for `bkt` to
address. Today, `bootc-bootstrap` (a shell script) handles fontconfig as a
special case. RFC 0013 already proposes a `bkt bootstrap` command with a systemd
user unit, but it does not yet exist. The current `bkt apply` has no concept of
"deployment checksum changed" — that gating logic lives entirely in the shell
script.

The fontconfig investigation demonstrates a concrete need for
**deployment-gated user-level hooks**:

- **`bkt apply`** could incorporate deployment-change detection (comparing
  ostree deployment checksums) as a standard lifecycle phase, subsuming
  `bootc-bootstrap`'s current role
- **`bkt doctor`** could detect stale user caches (fontconfig confirmed,
  icon-theme.cache plausible)
- **A deployment hook registry** could let subsystems declare "run this when the
  deployment changes," replacing ad-hoc shell functions

The scope of the problem is narrower than initially hypothesized — fontconfig is
the confirmed case, with GTK icon-theme.cache as a plausible secondary case.
Mesa shader caches and most other system-derived caches use content-based keys
or have no user-level mirrors. But even with just fontconfig, the pattern argues
for formalizing deployment-gated hooks rather than keeping them as shell script
special cases.

This is a class of problem that ostree/bootc doesn't solve (system-level only),
Fedora Atomic doesn't solve (no user-level hooks), and individual apps don't
solve (they assume mtime-based invalidation works). `bkt` occupies the gap
between "system deployment" and "user session" where this cleanup belongs.

## References

### Fontconfig Upstream

- [fontconfig#444](https://gitlab.freedesktop.org/fontconfig/fontconfig/-/issues/444) — Cache version not bumped for font wrappers (Jan 2025, closed)
- [fontconfig#330](https://gitlab.freedesktop.org/fontconfig/fontconfig/-/issues/330) — Cache version bump breaks Flatpak (Sep 2022)
- [fontconfig#329](https://gitlab.freedesktop.org/fontconfig/fontconfig/-/issues/329) — fc-match crashes on ostree with toolbox (Sep 2022)
- [fontconfig#44](https://gitlab.freedesktop.org/fontconfig/fontconfig/-/issues/44) — Nanosecond mtime precision required (2017)
- [fontconfig#45](https://gitlab.freedesktop.org/fontconfig/fontconfig/-/issues/45) — Cross-version cache disagreement (2017, closed)

### Downstream (Fedora Atomic / UBlue)

- [silverblue#534](https://github.com/fedora-silverblue/issue-tracker/issues/534) — CJK fonts broken in Flatpak
- [silverblue#540](https://github.com/fedora-silverblue/issue-tracker/issues/540) — International fonts broken with Chrome
- [bluefin#1558](https://github.com/ublue-os/bluefin/issues/1558) — Font cache not cleaned by fc-cache
- [bluefin#1560](https://github.com/ublue-os/bluefin/pull/1560) — Fix: clear Flatpak font cache (merged Aug 2024)
- [bazzite#864](https://github.com/ublue-os/bazzite/issues/864) — Missing fonts
- [bazzite#1178](https://github.com/ublue-os/bazzite/issues/1178) — bazzite-user-setup clears wrong directory
- [distrobox#358](https://github.com/89luca89/distrobox/issues/358) — Cross-distro cache incompatibility

### Chromium / Flatpak

- [flathub/chromium#280](https://github.com/flathub/org.chromium.Chromium/issues/280) — CJK fonts, ostree mtime discussed
- [flathub/chrome#202](https://github.com/flathub/com.google.Chrome/issues/202) — CJK on Silverblue
- [flathub/edge#299](https://github.com/flathub/com.microsoft.Edge/issues/299) — Chinese fonts on Silverblue

### freedesktop SDK

- [freedesktop-sdk#1576](https://gitlab.com/freedesktop-sdk/freedesktop-sdk/-/issues/1576) — Cache seed mismatch in Flatpak runtimes (fixed)
