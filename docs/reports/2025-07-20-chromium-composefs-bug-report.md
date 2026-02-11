# Chromium composefs font rendering investigation (resolved: fontconfig cache staleness)

## Summary

Chromium and Electron apps on composefs-based, image-updated systems (bootc/ostree)
can fail to render non-Latin system fonts (Kannada, Arabic, emoji). This is NOT
an upstream Chromium or kernel bug. The root cause is stale fontconfig user
cache files in `~/.cache/fontconfig/` that persist across OS deployments and
contain outdated metadata for fonts on the new composefs root.

Clearing the cache and rebuilding with `fc-cache -f` fixes all rendering issues.
No upstream bug reports are needed.

## Environment

- OS: Bazzite 43 (Fedora 43 based), bootc/ostree with composefs
- Browser: Microsoft Edge 144.0.3719.115 (Chromium 144)
- fontconfig: 2.17.0
- Root FS: composefs overlay (metacopy=on, datadir+=/sysroot/ostree/repo/objects)
- Home FS: btrfs

## Symptoms

- Non-Latin system fonts render as invisible or empty output in Chromium/Electron
- `document.fonts.check()` returns `true` for the font
- Web fonts loaded via `@font-face` render correctly
- Latin glyphs may render while complex scripts and emoji fail

## Root Cause

Fontconfig caches font metadata in `~/.cache/fontconfig/`. On image-based
systems, OS upgrades replace the entire root filesystem (new composefs inodes
and potentially new font builds). The user cache persists and can include stale
entries from previous deployments or older fontconfig versions.

The cache directory contained mixed-format cache files:

- `cache-9`
- `cache-11`
- `cache-reindex1-10`

`fc-cache -f` regenerates current-version caches but does not remove older
format files. The stale caches were still being read, causing fontconfig to
return outdated metadata. Chromium then rendered zero pixels for complex script
glyphs even though the font files were present and readable.

## Evidence Trail

### 1) Same font, different glyphs (composefs)

From the same composefs `NotoSansKannada-Regular.ttf` file:

- ASCII glyphs rendered (non-zero pixels):
  - period: 32
  - zero: 296
  - comma: 51
  - quoteleft: 55
- All Kannada glyphs rendered as **zero pixels**, including Kannada digits

This shows the file itself is readable but the complex-script coverage was
missing due to stale metadata, not file access failures.

### 2) Pango + FreeType renders from composefs

Python (Cairo + Pango, FreeType backend) rendered Kannada from the same
composefs font file with **829 non-zero pixels**, ruling out composefs data
access bugs.

### 3) Definitive fix: clear fontconfig cache

```
rm -rf ~/.cache/fontconfig/*
fc-cache -f
```

After clearing the cache and rebuilding:

- Kannada: 247
- Arabic: 212
- Emoji (Twemoji): 1412
- NotoSans: 797
- Inter: 826

All non-Latin fonts rendered correctly with non-zero pixels.

### 4) Why `fc-cache -f` alone did not fix it

`fc-cache -f` does not delete old-format caches. The remaining
`cache-reindex1-10` files likely overrode fresh entries with stale metadata.
Only deleting the entire cache directory resolved the issue.

### 5) Why the mirror/symlink workaround worked

Adding fonts at new paths (`~/.local/share/fonts/composefs-mirror/` or
`~/.local/share/fonts/test-symlink/`) forced new cache entries with no stale
metadata. This bypassed the old cache entries and rendered correctly, but the
mirror is unnecessary once the cache is cleared.

## Disproven Hypotheses

- Composefs mmap returns wrong data: byte-identical mmap/read data
- Chromium sandbox blocks composefs access: `--no-sandbox` showed same failures
- Fontations bug: Pango+FreeType renders from composefs fine; Fontations toggle
  did not change behavior
- Font format/table access issue: table hashes match between composefs and btrfs

## Fix and Recommendation

**Fix:** clear the user fontconfig cache and rebuild it.

- Command sequence: `rm -rf ~/.cache/fontconfig/* && fc-cache -f`
- Should run after each OS deployment upgrade on composefs-based systems
- The bootc bootstrap flow now performs this automatically (see
  [scripts/bootc-bootstrap](scripts/bootc-bootstrap))

**Recommendation:** Do not file upstream bugs against Chromium or the kernel.
This is a local cache invalidation issue on image-based systems.

## Resolution

Mark the original Chromium/composefs bug reports as resolved. Track the fix as
an OS-level maintenance step (fontconfig cache clear on deployment change).# Chromium Bug Report: System fonts fail to rasterize on composefs overlay (Fontations backend)

> **Filed against:** Chromium — Blink > Fonts
> **Severity:** S2 (renders all system-installed fonts invisible in canvas/paint operations)
> **Reproducible:** 100% on affected configurations
> **Affects:** All Chromium-based browsers AND Electron apps (Microsoft Edge, VS Code, VS Code Insiders, etc.)

## Summary

System-installed font glyphs render as invisible (zero pixels) on Linux systems
where the root filesystem is a composefs overlay (used by Fedora CoreOS, Bazzite,
and other bootc/ostree-based distributions). Fontconfig finds the fonts, the
SkTypeface is created, `document.fonts.check()` returns `true` — but all canvas
and paint operations produce empty output.

**Web fonts loaded via `@font-face` work perfectly.** The regression is
isolated to system font file access.

**This began with the Fontations migration for system fonts** (CL 6169919,
March 11, 2025). The previous FreeType backend rendered these same fonts
correctly on the same filesystem.

## Environment

- **OS:** Bazzite 43 (Fedora 43 Rawhide, bootc/ostree with composefs)
- **Kernel:** 6.14+ (composefs as default ostree deployment backend)
- **Browser:** Microsoft Edge 144.0.3719.115 (Chromium 144)
- **Also confirmed:** VS Code / VS Code Insiders (Electron, same Chromium engine)
- **Root filesystem mount:**
  ```
  composefs on / type overlay (ro,relatime,seclabel,
    lowerdir+=/run/ostree/.private/cfsroot-lower,
    datadir+=/sysroot/ostree/repo/objects,
    redirect_dir=on,metacopy=on)
  ```
- **Home filesystem:** btrfs on /dev/nvme0n1p3

## Steps to Reproduce

1. Install a bootc/ostree-based Linux distribution that uses composefs (e.g.,
   Bazzite 43, Fedora CoreOS with composefs enabled)
2. Ensure system fonts are installed (e.g., Noto sans, Twemoji, any TTF/OTF in
   `/usr/share/fonts/`)
3. Open Edge (or any Chromium ≥131 / Electron app built against it)
4. Navigate to any page that uses system fonts for emoji, Indic scripts
   (Kannada, Devanagari), or CJK characters
5. Observe: glyphs are invisible or render as `.notdef` boxes

**Verification that the fonts ARE installed and found:**

```bash
$ fc-match "Noto Sans Kannada"
NotoSansKannada-Regular.ttf: "Noto Sans Kannada" "Regular"

$ fc-list | grep -c ".ttf\|.otf"
742
```

**Verification via JavaScript:**

```javascript
// In Edge DevTools console:
document.fonts.check('16px "Noto Sans Kannada"');
// → true (fontconfig found it, SkTypeface was created)

// But canvas rendering produces zero pixels:
const canvas = document.createElement("canvas");
canvas.width = 200;
canvas.height = 50;
const ctx = canvas.getContext("2d");
ctx.font = '32px "Noto Sans Kannada"';
ctx.fillText("ಕನ್ನಡ", 10, 35);
const data = ctx.getImageData(0, 0, 200, 50).data;
const nonZero = data.filter((v) => v !== 0).length;
console.log("Non-zero pixels:", nonZero);
// → 0 (no glyph data painted)
```

## Expected Result

System fonts installed in `/usr/share/fonts/` should render correctly regardless
of the underlying filesystem mount type.

## Actual Result

All system-installed fonts produce zero pixels in canvas/paint operations. The
symptom appears as:

- Emoji render as monochrome `.notdef` boxes
- Indic scripts (Kannada, Devanagari) are completely invisible
- CJK characters may fall through to incorrect fallback fonts
- Any character whose glyph lives exclusively in a system-installed font fails

## Root Cause Analysis

### The Fontations Migration Changed System Font Access Patterns

| Timeline     | Change                                                                 | Impact                                                          |
| ------------ | ---------------------------------------------------------------------- | --------------------------------------------------------------- |
| Dec 4, 2024  | Fontations 100% for **web fonts** (stable launch)                      | No regression — web fonts use HTTP fetch, not local file access |
| Mar 11, 2025 | **CL 6169919**: "Instantiate Linux system fonts using Fontations"      | ← First point of failure on composefs                           |
| Mar 12, 2025 | CL 6346806: Use SkData constructor (mmap) for Fontations instantiation | Eliminated SkStream→SkData conversion                           |
| Mar 21, 2025 | CL 6383036: Avoid further SkStream/SkData conversions                  | Doubled down on mmap-first path                                 |
| May 8, 2025  | Remove Fontations flag — Fontations is the ONLY backend                | No fallback to FreeType possible                                |

**FreeType** (the previous backend) loaded font data using **stream-based
sequential `read()` calls** via `SkFontMgr_FreeType_Empty::makeFromStream()`.
These syscalls work correctly on composefs.

**Fontations** loads font data by constructing an **`SkData` from a memory-mapped
file** (`SkData::MakeFromFILE` → `mmap()`). The font's glyph table is then
accessed as a `&[u8]` byte slice backed by the mmap'd region.

### The composefs Filesystem

composefs is an **overlayfs mount with two non-standard features**:

1. **`metacopy=on`**: Only metadata (ownership, permissions, xattrs) is stored
   in the upper/lower layers. File data is redirected to a separate data layer.

2. **`datadir+=/sysroot/ostree/repo/objects`**: File content is stored in a
   content-addressed object store. When a file is opened, overlayfs resolves its
   content through an OCI digest stored as an extended attribute, then serves
   data from the corresponding object in the datadir.

This means font files on composefs are:

- Metadata in `/run/ostree/.private/cfsroot-lower` (erofs)
- Data in `/sysroot/ostree/repo/objects` (ext4/btrfs, content-addressed by SHA-256)
- Joined by the kernel's overlayfs implementation

### The Interaction: Fontations + composefs + Chromium Sandbox

From a normal (unsandboxed) process, `mmap()` on composefs files returns correct
data. A Python test confirmed:

```
mmap SHA-256: 7fe1a2ee3177afd1...
read SHA-256: 7fe1a2ee3177afd1...
All offsets match: True
```

However, Chromium's **renderer process** runs inside a severe sandbox:

- User namespace (unprivileged)
- seccomp-bpf filter (restricted syscalls)
- `chroot()` into an empty directory (`/proc/self/fdinfo/`-based)
- Restricted file descriptor set (no filesystem access post-sandbox)

The hypothesis is that this sandbox interacts pathologically with composefs's
multi-layer file resolution. When the renderer attempts to access mmap'd font
data, the kernel's overlayfs path from metacopy metadata → datadir content may
fail or return zeros within the sandboxed context. This would explain why:

1. **fontconfig succeeds** — it runs in the browser process (less sandboxed)
2. **SkTypeface creation succeeds** — also in the browser process
3. **Glyph rasterization fails** — happens in the renderer process (heavily sandboxed)
4. **Web fonts work** — they're fetched via HTTP (IPC to the network process), not loaded from the local filesystem
5. **Copying fonts to btrfs fixes it** — removes the composefs layer, ordinary `mmap()` works

### Why This Is New

This bug requires the intersection of three relatively recent developments:

- **composefs** became the default deployment backend for ostree ~2024
- **Fontations for system fonts** landed March 2025
- **Fontations flag removal** (no FreeType fallback) May 2025

Prior to Fontations, FreeType used `read()` (not `mmap()`) for font data, which
works correctly on composefs regardless of sandbox state.

## Workaround

Copying system fonts to a regular (non-overlayfs) filesystem restores rendering:

```bash
rsync -a /usr/share/fonts/ ~/.local/share/fonts/composefs-mirror/
fc-cache ~/.local/share/fonts/composefs-mirror/
```

This causes fontconfig's user-dir to shadow the composefs originals. The fonts
are now on btrfs, and mmap works correctly even in the sandboxed renderer.

## Suggested Investigation

1. **Verify the sandbox hypothesis**: Run Edge with `--no-sandbox` on a composefs
   system and test whether system fonts render correctly. If they do, the issue
   is confirmed as a sandbox × composefs interaction.

2. **Check `SkData::MakeFromFILE` behavior**: On composefs, does the mmap'd
   region return valid data when accessed from within the renderer sandbox?
   Adding logging or a debug flag to fall back to `read()` would isolate the
   layer.

3. **Consider a `read()` fallback**: If `mmap()` fails or returns suspect data,
   falling back to stream-based loading (as FreeType did) would restore
   compatibility with overlayfs/composefs. The performance difference for font
   loading is negligible compared to font rasterization.

4. **Coordinate with kernel/overlayfs maintainers**: This may also be a kernel
   bug (see companion report). The correct long-term fix may be in the kernel
   or in Chromium — or both.

## Related Links

- Chromium issue 40045339: Meta — Rust-based Fontations font backend
- Chromium issue 346918516: Move Linux system fonts to Fontations
- CL 6169919: "Instantiate Linux (or CrOS) system fonts using Fontations"
- CL 6346806: "[Fontations] Use SkData constructor for Fontations instantiation"
- CL 6383036: "[Fontations] Avoid further SkStream/SkData conversions"
- overlayfs docs: `Documentation/filesystems/overlayfs.rst`, "Metadata only copy up" and "Data-only lower layers" sections
