# Daily Workflow

You maintain your own Linux distribution. This guide covers the day-to-day patterns.

## The Mental Model

You have two modes:

| Mode         | What you're doing               | Where   |
| ------------ | ------------------------------- | ------- |
| **Using**    | Running apps, gaming, browsing  | Host    |
| **Building** | Writing code, building software | Toolbox |

Both share your home directory. The toolbox is a container that mounts `~`.

## Developing in the Toolbox

### Enter the toolbox

```bash
toolbox enter
```

You're now in your dev environment. Everything in `~` is available — your code, dotfiles, and toolchains.

### Host commands work transparently

From inside the toolbox, these run on the host automatically:

```bash
bootc status      # System status
systemctl status  # System services
podman ps         # Containers (on host)
ujust update      # System update
```

These are **shims** — small scripts that delegate to the host. They're managed via the `shim` command.

### Managing shims

```bash
shim list                   # See all shims
shim add nmcli              # Add a shim
shim add dc docker-compose  # Alias: 'dc' runs 'docker-compose' on host
shim remove nmcli           # Remove a shim
```

Shims come from two places:

| Source | Location                                           | Purpose          |
| ------ | -------------------------------------------------- | ---------------- |
| System | `/usr/local/share/bootc-bootstrap/host-shims.json` | Baked into image |
| User   | `~/.config/bootc/host-shims.json`                  | Your additions   |

## Making Changes Permanent

Here's the key pattern: **apply locally for immediate effect, open a PR to make it permanent**.

### For things that don't require reboot

These apply immediately and can be synced to the repo:

| What             | Local command                 | To bake permanently                          |
| ---------------- | ----------------------------- | -------------------------------------------- |
| Add a shim       | `shim add nmcli`              | Edit `manifests/host-shims.json`, push       |
| Install Flatpak  | `flatpak install ...`         | Edit `manifests/flatpak-apps.json`, push     |
| Enable extension | `gnome-extensions enable ...` | Edit `manifests/gnome-extensions.json`, push |

**Coming soon:** `--pr` flag to do both at once:

```bash
shim add --pr nmcli   # Apply locally AND open PR
```

### For things that require reboot

These require editing the repo and rebuilding:

| What                 | Where to edit                  |
| -------------------- | ------------------------------ |
| Add system package   | `Containerfile`                |
| Add system font      | `Containerfile`                |
| Change system config | `system/...` + `Containerfile` |
| Add toolbox package  | `toolbox/Containerfile`        |

The workflow:

```bash
cd ~/Code/Config/bootc
# Edit the file
git add -A && git commit -m "feat: add thing"
git push
# Wait for CI (~10 min)
sudo bootc upgrade
systemctl reboot
```

## Checking for Drift

"Drift" = your running system differs from what the repo declares.

```bash
check-drift
```

If you find drift you want to keep, update the manifests and push. If you don't want it, the next bootstrap will fix it.

## Updating

### Host OS (requires reboot)

```bash
sudo bootc upgrade
systemctl reboot
```

Or let `uupd` handle it automatically — it checks hourly and stages updates.

### Toolbox (no reboot)

```bash
podman pull ghcr.io/wycats/bootc-toolbox:latest
toolbox rm -f dev
toolbox create --image ghcr.io/wycats/bootc-toolbox:latest dev
```

Your home directory persists. Only the container is recreated.

## The PATH in Toolbox

```
~/.local/bin               # Your scripts (works on host too)
~/.local/toolbox/bin       # Toolbox-only scripts
~/.local/toolbox/shims     # Host command delegates
~/.nix-profile/bin         # Nix
~/.cargo/bin               # Rust
~/.proto/bin               # Proto
/usr/local/bin             # Container packages
...
```

Your scripts take precedence over everything.

## Recovery

Something broke after upgrading?

1. Reboot
2. In GRUB, select the previous deployment
3. You're back to working state
4. Fix the issue in repo, push, try again

bootc always keeps the previous deployment. You're never stuck.

## Quick Reference

| Task          | Command                                  |
| ------------- | ---------------------------------------- |
| Enter toolbox | `toolbox enter`                          |
| Update host   | `sudo bootc upgrade && systemctl reboot` |
| Check drift   | `check-drift`                            |
| Add shim      | `shim add <name>`                        |
| List shims    | `shim list`                              |
| Host status   | `bootc status`                           |
