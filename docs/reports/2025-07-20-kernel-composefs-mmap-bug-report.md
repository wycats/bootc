# Disproven hypothesis: composefs mmap bug

## Summary

This report documents a disproven hypothesis. The original theory was that
composefs or overlayfs mmap behavior caused Chromium/Electron to render system
fonts as empty output. The actual root cause is stale fontconfig user cache
files in `~/.cache/fontconfig/` persisting across OS deployments.

No kernel or composefs bug exists here. No upstream kernel report is needed.

## Environment

- OS: Bazzite 43 (Fedora 43 based), bootc/ostree with composefs
- Browser: Microsoft Edge 144.0.3719.115 (Chromium 144)
- fontconfig: 2.17.0
- Root FS: composefs overlay (metacopy=on, datadir+=/sysroot/ostree/repo/objects)
- Home FS: btrfs

## Evidence That composefs mmap Works

1. **Byte-identical data**

Python mmap tests on the composefs font file showed identical SHA-256 hashes
between `read()` and `mmap()` output. No data corruption or zeroed pages were
observed.

2. **Pango + FreeType renders from composefs**

Cairo + Pango (FreeType backend) rendered Kannada from the same composefs font
file with 829 non-zero pixels. This proves the underlying font data is readable
and correct on composefs.

3. **Definitive fix is fontconfig cache clear**

```
rm -rf ~/.cache/fontconfig/*
fc-cache -f
```

After clearing the cache, all non-Latin fonts rendered correctly in Chromium.
No filesystem or kernel changes were required.

## Final Diagnosis

- Root cause: stale fontconfig user cache files retained across OS deployments
- `fc-cache -f` alone did not remove older-format caches (e.g. `cache-reindex1-10`)
- Clearing the cache directory resolves the issue completely

## Outcome

Close this hypothesis. Do not file or pursue kernel/composefs bug reports for
this issue.# Kernel/composefs Bug Report: mmap'd data from data-only overlay layers inaccessible in sandboxed namespaces

> **Filed against:** kernel overlayfs / composefs
> **Subsystem:** fs/overlayfs
> **Severity:** Major — breaks any sandboxed application that mmap's files from composefs overlays
> **Kernel:** 6.14+ (reproducible on Fedora 43 / Bazzite with composefs as ostree backend)

## Summary

Applications running inside a restricted sandbox (user namespace + seccomp +
chroot, as used by Chromium/Electron) cannot correctly read mmap'd data from
files on a composefs overlay mount. The data appears valid from unsandboxed
processes, but glyph rasterization and other byte-level reads from the mmap'd
region fail within the sandbox.

This is NOT a simple "mmap returns zeros" — a Python test from an unsandboxed
process confirms byte-for-byte identical data between `mmap()` and `read()`.
The failure is specific to the combination of overlayfs's metacopy + data-only
layer resolution and the namespace/seccomp restrictions of the consuming process.

## Mount Configuration

```
composefs on / type overlay (ro,relatime,seclabel,
  lowerdir+=/run/ostree/.private/cfsroot-lower,
  datadir+=/sysroot/ostree/repo/objects,
  redirect_dir=on,metacopy=on)
```

Key features:

- **`metacopy=on`**: Metadata-only copy-up. File metadata (permissions, xattrs)
  is in the lowerdir. File data is indirected via `trusted.overlay.redirect`
  (or similar) to the data-only layer.
- **`datadir+=/sysroot/ostree/repo/objects`**: Data-only lower layer. File
  content is stored in a content-addressed object store, addressed by digest.
  The kernel resolves the indirection transparently.
- The lowerdir (`/run/ostree/.private/cfsroot-lower`) is an erofs mount
  containing only metadata and directory structure.

## Affected Files

All regular files on the overlay. Confirmed with font files:

```
$ stat /usr/share/fonts/google-noto-sans-kannada-fonts/NotoSansKannada-Regular.ttf
  Size: 3215988   Inode: 186264   Device: 0,38
  Access: 2024-01-01 00:00:00.000000000 +0000  (epoch — erofs metadata layer)
```

Note the epoch timestamps — these come from the erofs metadata layer
(`lowerdir`). The actual data is served from the object store (`datadir`).

## Reproduction

### What Works (unsandboxed)

From a normal process, `mmap()` and `read()` return identical data:

```python
import mmap, hashlib

with open('/usr/share/fonts/.../NotoSansKannada-Regular.ttf', 'rb') as f:
    data_read = f.read()
    f.seek(0)
    mm = mmap.mmap(f.fileno(), 0, access=mmap.ACCESS_READ)
    data_mmap = mm[:]
    mm.close()

print(hashlib.sha256(data_read).hexdigest())
print(hashlib.sha256(data_mmap).hexdigest())
# Both: 7fe1a2ee3177afd1... (identical)
```

### What Fails (sandboxed)

Chromium's renderer process, which runs inside:

- `clone(CLONE_NEWUSER | CLONE_NEWNS | CLONE_NEWPID | CLONE_NEWNET)`
- seccomp-bpf filter restricting syscalls
- `chroot()` into empty directory
- Pre-opened file descriptors only (no new filesystem access)

Within this sandbox, mmap'd font data from composefs files produces zero pixels
when used for glyph rasterization (via Skia/Fontations). The font file's
`SkTypeface` is created in the browser process (less sandboxed), and the mmap'd
`SkData` region is shared with the renderer. But when the renderer accesses glyph
table offsets in the mmap'd region, the expected data is not there.

**Evidence that the sandbox is the differentiating factor:**

1. fontconfig (browser process) correctly locates and reads font metadata
2. `document.fonts.check()` returns `true` (SkTypeface created successfully)
3. Web fonts loaded via HTTP (not from the filesystem) render perfectly
4. Copying the exact same font bytes to btrfs (`~/.local/share/fonts/`) fixes rendering
5. The ONLY difference is the filesystem: composefs overlay vs. btrfs

## Hypothesis: Data-Only Layer Resolution Across Namespace Boundaries

When overlayfs serves a file with `metacopy=on` and `datadir`, the kernel must
resolve the data indirection:

1. Open the metadata inode in `lowerdir` (erofs)
2. Read the redirect xattr to find the content digest
3. Look up the corresponding object in `datadir`
4. Serve data from that object's inode

If step 3 or 4 fails when the consuming process is in a different namespace —
specifically a user namespace without privileges to traverse the `datadir` mount
— the kernel may return zeros or SIGBUS instead of the expected data.

This is consistent with overlayfs's documented non-standard behaviors:

> **(b)** If a file residing on a lower layer is opened for read-only and then
> memory mapped with `MAP_SHARED`, then subsequent changes to the file are not
> reflected in the memory mapping.

While this specific note is about change propagation (not relevant here since
the data is static), it demonstrates that overlayfs's mmap behavior has known
divergences from POSIX expectations. A similar divergence for data-only layers
across namespace boundaries would explain this bug.

## Affected Applications

Any application that:

1. Opens a file on a composefs overlay
2. Creates an mmap'd region from that file
3. Accesses the mmap'd data from within a sandboxed child process (particularly
   one in a different user namespace)

Confirmed affected:

- **Microsoft Edge** (Chromium 144, RPM install) — system fonts invisible
- **VS Code / VS Code Insiders** (Electron/Chromium) — system fonts invisible
- **Any Chromium-based browser** on composefs would be affected

Likely affected:

- Any application using user namespaces + mmap on composefs files
- Container runtimes that mmap host files from overlayfs (though most use
  separate mounts)

## Workaround

Copy the affected files to a regular (non-overlay) filesystem:

```bash
rsync -a /usr/share/fonts/ ~/.local/share/fonts/composefs-mirror/
fc-cache ~/.local/share/fonts/composefs-mirror/
```

Chromium then uses the btrfs copies (via fontconfig's user-dir priority) and
mmap works correctly.

## Bisection Opportunity

This bug can be isolated by testing two axes:

### Axis 1: Chromium Sandbox

Run Chromium with `--no-sandbox` on a composefs system. If system fonts render
correctly, the issue is confirmed as a namespace × composefs interaction.

### Axis 2: overlayfs Configuration

Test the same Chromium version on:

1. ✅ ext4 or btrfs root — works (traditional Fedora Workstation)
2. ✅ ostree WITHOUT composefs (ostree hardlink farm) — likely works
3. ❌ ostree WITH composefs (overlayfs + metacopy + datadir) — fails
4. ❓ overlayfs with metacopy=on but WITHOUT data-only layers — unknown
5. ❓ overlayfs with data-only layers but WITHOUT metacopy — unknown

Tests 4 and 5 would isolate whether the issue is in metacopy, data-only layers,
or their combination.

## Distribution Impact

composefs is now the default deployment backend for ostree-based distributions
including:

- Fedora CoreOS
- Bazzite (gaming desktop, based on Universal Blue)
- Bluefin / Aurora (Universal Blue desktop variants)
- Any custom bootc image

All Chromium/Electron applications on these distributions are affected.
As Fontations became the only font backend in Chromium 131+ (flag removed May
2025), there is no configuration-level workaround on the Chromium side.

## Related

- Companion Chromium bug report (filed separately against Blink > Fonts)
- overlayfs documentation: `Documentation/filesystems/overlayfs.rst` sections
  "Metadata only copy up" and "Data-only lower layers"
- composefs: https://github.com/containers/composefs
- ostree composefs integration: https://github.com/ostreedev/ostree
