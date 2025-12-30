# Ostree Filesystem Patterns

On ostree-based systems (Fedora Atomic, Bazzite, etc.), certain directories behave differently than traditional Linux.

## Key Insight

The rootfs (`/usr`) is **immutable** and replaced on each update. But some paths are **persistent** — they survive across updates and are NOT replaced by image contents.

## Persistent Paths (Don't Install Here)

| Path | Actually Points To | Behavior |
|------|-------------------|----------|
| `/opt` | `/var/opt` | Persistent, NOT updated |
| `/usr/local` | `/var/usrlocal` | Persistent, NOT updated |
| `/home` | `/var/home` | Persistent (user data) |
| `/srv` | `/var/srv` | Persistent |
| `/mnt` | `/var/mnt` | Persistent |
| `/root` | `/var/roothome` | Persistent |

**Problem:** If your Containerfile installs binaries to `/usr/local/bin` or packages install to `/opt`, those files exist in the image but are **invisible** at runtime because the symlink points to the persistent `/var` location.

## Solutions

### For `/usr/local/*`

**Don't use it.** Install to `/usr` instead:
- `/usr/local/bin` → `/usr/bin`
- `/usr/local/share` → `/usr/share`
- `/usr/local/lib` → `/usr/lib`

### For `/opt/*` (Third-party packages like Edge, 1Password)

1. **Copy to `/usr/lib/opt/`** during image build
2. **Use `tmpfiles.d`** to create symlinks at boot

```dockerfile
# After installing packages that use /opt
RUN set -eu; \
    if [ -d /opt ] && [ "$(ls -A /opt 2>/dev/null)" ]; then \
        mkdir -p /usr/lib/opt; \
        cp -a /opt/. /usr/lib/opt/; \
        rm -rf /opt/*; \
    fi

# Create symlinks at boot time
RUN printf '%s\n' \
    'L+ /var/opt/1Password - - - - /usr/lib/opt/1Password' \
    'L+ /var/opt/microsoft - - - - /usr/lib/opt/microsoft' \
    >/usr/lib/tmpfiles.d/bootc-opt.conf
```

### Alternative: Per-package relocation (BlueBuild pattern)

```dockerfile
# Move specific app and symlink
RUN mv /opt/1Password /usr/lib/1Password && \
    ln -s /usr/lib/1Password /usr/bin/1password

# tmpfiles.d for the /opt path apps expect
RUN echo 'L /opt/1Password - - - - /usr/lib/1Password' \
    >/usr/lib/tmpfiles.d/onepassword.conf
```

## How to Detect Issues

Run the CI check: `scripts/check-ostree-paths`

Or manually after a build:
```bash
# Check if anything installed to problematic paths
podman run --rm <image> ls -la /opt /usr/local/bin 2>/dev/null
```

## References

- [Fedora bootc filesystem docs](https://docs.fedoraproject.org/en-US/bootc/filesystem/)
- [BlueBuild optfix module](https://github.com/blue-build/modules/blob/main/modules/rpm-ostree/optfix.sh)
- [BlueBuild 1Password installer](https://github.com/blue-build/modules/blob/main/modules/bling/installers/1password.sh)
