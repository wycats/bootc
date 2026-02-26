# btrfs xattrs hardlink limit during bootc upgrade

**Date**: 2026-02-25 to 2026-02-26
**Status**: Fixed (workaround), root cause analysis incomplete

## Symptom

`sudo bootc upgrade` fails with:

```
Hardlinking 40/a6050fa7c302b1ddd48335ab09bd70f106a0a91d7a011b24c6e4673e6a4a5f.file
to 5596856829e61c684d0f66f3d203e0197f1dd0ab81c7f6f821b66de70c0187.file-xattrs-link:
Too many links
```

The upgrade downloads layers successfully but fails during the ostree import
(checkout) phase.

## Environment

- **OS**: Bazzite (Fedora 43 based), btrfs root filesystem
- **bootc**: 1.12.1
- **ostree**: 2025.7
- **btrfs features**: EXTENDED_IREF enabled (hardlink limit: 65,535)
- **composefs**: enabled (`/usr/lib/ostree/prepare-root.conf` has `enabled = yes`)
- **Image build**: BuildKit via `docker/build-push-action` on GitHub Actions

## Observations

### strace data

Traced with `strace -ff -e trace=link,linkat -o /tmp/bootc-trace bootc upgrade`.

The failing process (PID 202914) made **452,120 `linkat` calls** total. Of these,
**65,535 were to the same source file** (`40/a6050fa7c302...file`):

- 65,534 succeeded (`= 0`)
- 1 failed (`= -1 EMLINK`)

The `linkat` pattern:
```
linkat(21, "40/a6050fa7c302...file", 39, "<hash>.file-xattrs-link", 0) = 0
```

The source file (FD 21, path `40/a6050fa7c302...`) is the **shared xattrs object**.
The target (FD 39) is a different file each time, with the `-xattrs-link` suffix.

### The xattrs object

The file `40/a6050fa7c302b1ddd48335ab09bd70f106a0a91d7a011b24c6e4673e6a4a5f.file`
is 46 bytes containing:

```
security.selinux\0system_u:object_r:usr_t:s0\0
```

This is the default SELinux label for files under `/usr`. ostree deduplicates
xattrs by creating hardlinks to a shared object for all files with identical
xattrs. When >65,535 files share the same xattrs, the btrfs hardlink limit
is hit.

### Manual hardlink test

Creating a hardlink to the same file manually with `ln` **succeeded**. The
file had only 5-7 hardlinks at the time of testing. The 65,535 limit is hit
during the import because ostree creates the links in a temporary checkout
directory, not in the permanent object store.

### Other observations

- `rpm-ostree reset` was necessary to clear stale staged state that caused
  `bootc upgrade` to retry a failed import instead of pulling fresh.
- `ostree admin cleanup` alone did not clear the stale state.
- `bootc rollback` does NOT discard staged imports (despite documentation
  suggesting it might).
- The system had 439,000+ open files (VS Code: 113k, Edge: 66k, Steam: 59k)
  and inotify watcher warnings, but reducing load did not fix the issue.

## What we changed

**PR #138**: Removed `--link` from `COPY --from=install-*` instructions in the
Containerfile generator. Changed:

```dockerfile
# Before
COPY --link --from=install-code / /
COPY --link --from=install-microsoft-edge / /
COPY --link --from=install-bundled / /

# After
COPY --from=install-code / /
COPY --from=install-microsoft-edge / /
COPY --from=install-bundled / /
```

After this change, `bootc upgrade` succeeded:
- 14 layers needed (1.8 GB)
- Downloaded in 64 seconds
- Deployed in 12 seconds

**PR #137** (earlier, insufficient): Consolidated upstream/config/wrapper COPY
instructions into a single `collect-outputs` stage. Reduced final image layers
from 22 to 14. Did not fix the issue by itself.

## What we don't know

1. **Why `COPY --link` vs `COPY` changes the xattrs hardlink count.** Both
   produce OCI layers. The difference in how BuildKit serializes them, and
   how ostree's importer processes them, is not understood.

2. **Whether the xattrs links accumulate across layers or within a single
   layer.** The strace showed 65,535 links in one process, but we don't know
   if that process handles one layer or multiple layers.

3. **Whether the pre-PR-134 image was close to the limit.** The last working
   image (Feb 22, PR #133) had a `dnf install` layer that included external
   RPMs (more files). It worked, but we don't know its xattrs link count.

4. **Whether this will recur** if the base image grows, if we add more
   packages, or if the base image's layer structure changes.

5. **The exact mechanism by which `COPY --link` increases xattrs hardlink
   pressure.** The BuildKit documentation describes `--link` as creating
   "independent layers" that can be reordered, but the implications for
   ostree's import path are not documented.

## References

- [bootc PR #1578](https://github.com/bootc-dev/bootc/pull/1578): Fix for
  xattrs hardlink explosion in ostree's tar export path (merged Sep 2025).
  This fix is in the export path, not the import path.
- [ostree issue #3489](https://github.com/ostreedev/ostree/issues/3489):
  Hardlink chain issues in tar serialization (buildah-related).
- [bootc issue #1126](https://github.com/bootc-dev/bootc/issues/1126):
  Hardlink swapping across layers causing import failures.
- [btrfs documentation](https://btrfs.readthedocs.io/en/latest/btrfs-man5.html#filesystem-limits):
  Maximum hardlinks per file is 65,536 with EXTENDED_IREF.
- Our PR #134: Layer Independence (RFC-0045) — the change that introduced
  `COPY --link --from=install-*` and coincided with the start of failures.
- Our PR #138: The workaround that removed `--link` and fixed the upgrade.
