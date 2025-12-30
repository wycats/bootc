# Workflow Guide

This guide explains the day-to-day workflow for managing your workstation using this repo.

## The Big Picture

Your workstation has three layers:

| Layer | What | Where | How to change |
|-------|------|-------|---------------|
| **Host** | Immutable OS image | `/` (read-only) | Edit this repo, push, `bootc upgrade` |
| **Toolbox** | Ephemeral dev container | `toolbox enter` | Recreate from updated image |
| **Home** | Your files, configs, toolchains | `~` | Direct editing |

The host is your "baked" system — packages, fonts, system configs. Changes require rebuilding the image.

The toolbox is where you do development. It mounts your home directory, so your code and dotfiles persist.

## Daily Development

### Entering the toolbox

```bash
toolbox enter
```

You're now in an ephemeral container with your dev tools. Your home directory is bind-mounted, so:
- Your code is there (`~/Code/...`)
- Your dotfiles work (`~/.config/...`)
- Your toolchains work (`~/.cargo/bin`, `~/.nix-profile/bin`, etc.)

### Running host commands from toolbox

Certain commands need to run on the host (system management, containers, etc.). These are available as **shims**:

```bash
bootc status      # Actually runs on host
systemctl status  # Actually runs on host
podman ps         # Actually runs on host
```

The shims are transparent — they look like regular commands but delegate to the host via `flatpak-spawn`.

### Managing shims

```bash
# See what shims exist
shim list

# Add a shim for a host command
shim add nmcli              # shim 'nmcli' -> host 'nmcli'
shim add dc docker-compose  # shim 'dc' -> host 'docker-compose'

# Remove a shim
shim remove nmcli

# Regenerate shims from manifest
shim sync
```

**Where shims come from:**

| Manifest | Location | Purpose |
|----------|----------|---------|
| System | `/usr/local/share/bootc-bootstrap/host-shims.json` | Baked defaults |
| User | `~/.config/bootc/host-shims.json` | Your additions |

User shims override system shims with the same name.

## Making Permanent Changes

### Change types

| Change | Where to make it |
|--------|-----------------|
| Add a system package | `Containerfile` (dnf install) |
| Add a Flatpak | `manifests/flatpak-apps.json` |
| Add a GNOME extension | `manifests/gnome-extensions.json` |
| Add a default shim | `manifests/host-shims.json` |
| Change system config | `system/...` + `Containerfile` |
| Change dotfiles | `skel/...` (for new users) or just edit `~` |

### The gitops workflow

1. **Edit the repo** (you can do this from your toolbox):
   ```bash
   cd ~/Code/Config/bootc
   # Make your changes
   git add -A && git commit -m "feat: add thing"
   git push
   ```

2. **CI builds new image** (automatic, takes ~10 min)

3. **Upgrade your machine**:
   ```bash
   sudo bootc upgrade  # or: ujust update
   systemctl reboot
   ```

### Quick iteration (local testing)

If you want to test before pushing:

```bash
# Build locally
podman build -t localhost/bootc-test .

# Inspect it
podman run --rm -it localhost/bootc-test bash
```

Note: You can't `bootc switch` to a local image easily, but you can verify packages/configs are correct.

## Checking for Drift

"Drift" means your running system differs from what the manifests declare.

```bash
check-drift --repo-root /usr/local/share/bootc-bootstrap
```

This compares:
- Installed Flatpaks vs manifest
- Enabled GNOME extensions vs manifest

If you find drift you want to keep, update the manifests and push.

## Updating the Toolbox

When the toolbox image is updated:

```bash
# Pull new image
podman pull ghcr.io/wycats/bootc-toolbox:latest

# Recreate your toolbox
toolbox rm -f fedora-toolbox-42
toolbox create --image ghcr.io/wycats/bootc-toolbox:latest
```

Your home directory persists — only the container environment is recreated.

## Common Tasks

### Add a CLI tool to the host

```dockerfile
# In Containerfile, find the dnf install section
RUN dnf install -y \
    ... \
    new-package \  # Add here
    ...
```

### Add a CLI tool to the toolbox

```dockerfile
# In toolbox/Containerfile
RUN dnf install -y \
    ... \
    new-package \
    ...
```

### Add a Flatpak app

```json
// In manifests/flatpak-apps.json, add to "apps" array:
{
  "id": "org.example.App",
  "remote": "flathub",
  "scope": "user"
}
```

### Add a host shim (baked default)

```json
// In manifests/host-shims.json, add to "shims" array:
{ "name": "nmcli", "host": "nmcli" }
```

### Add a personal shim (not baked)

```bash
shim add nmcli
```

This edits `~/.config/bootc/host-shims.json` and regenerates shims.

## Understanding the PATH

In your toolbox, PATH is ordered:

```
~/.local/bin               # Your scripts (shared with host)
~/.local/toolbox/bin       # Toolbox-specific scripts
~/.local/toolbox/shims     # Host command delegates
~/.nix-profile/bin         # Nix packages
~/.cargo/bin               # Rust toolchain
~/.proto/bin               # Proto toolchains
/usr/local/bin             # Container binaries
...
```

This means:
- Your scripts override everything
- Shims make host commands available
- Toolchains from your home directory work

## Emergency Recovery

If something breaks after upgrading:

1. **Reboot** and select the previous deployment in GRUB
2. You're back to the working state
3. Fix the issue in the repo, push, upgrade again

bootc always keeps the previous deployment. You're never stuck.
