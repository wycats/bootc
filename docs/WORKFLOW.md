# Daily Workflow

You maintain your own Linux distribution. This guide covers the day-to-day patterns.

## The Mental Model

You have two modes:

| Mode         | What you're doing               | Where   |
| ------------ | ------------------------------- | ------- |
| **Using**    | Running apps, gaming, browsing  | Host    |
| **Building** | Writing code, building software | Toolbox |

Both share your home directory. The toolbox is a container that mounts `~`.

## Where to Run `bkt`

**All `bkt` commands work from both host and toolbox.** Delegation is automatic.

| Command Category      | Target  | From Toolbox                             |
| --------------------- | ------- | ---------------------------------------- |
| `bkt flatpak ...`     | Host    | Auto-delegates via flatpak-spawn         |
| `bkt extension ...`   | Host    | Auto-delegates via flatpak-spawn         |
| `bkt gsetting ...`    | Host    | Auto-delegates via flatpak-spawn         |
| `bkt capture`         | Host    | Auto-delegates via flatpak-spawn         |
| `bkt apply`           | Host    | Auto-delegates via flatpak-spawn         |
| `bkt shim ...`        | Host    | Auto-delegates via flatpak-spawn         |
| `bkt admin bootc ...` | Host    | Auto-delegates via flatpak-spawn         |
| `bkt dev dnf ...`     | Toolbox | Auto-delegates from host via toolbox run |
| `bkt status`          | Either  | Runs in current context                  |

### Daemon Acceleration

When running `bkt` from the toolbox, delegation uses `flatpak-spawn` by default (~120ms overhead). The **bkt daemon** provides ~30x faster delegation (~4ms) via Unix socket.

**New installations**: The daemon starts automatically at login.

**Existing installations**: Enable once after upgrading:

```bash
# Check daemon status
bkt admin daemon status

# If not running, enable and start
systemctl --user enable --now bkt-daemon.service
```

The daemon is optional — all commands work without it, just slower from toolbox.

## Syncing System Changes to Manifests

After installing things via GUI (GNOME Software, Extension Manager, Settings):

```bash
# Preview what would be captured (dry-run)
bkt capture --dry-run

# Capture all changes to manifests
bkt capture --apply

# Review changes
cd ~/Code/Config/bootc
git diff manifests/

# Commit and push
git add -A && git commit -m "feat: capture system changes"
git push
```

## The Development Loop (Making Changes)

When you modify the system configuration (`Containerfile`, `manifests/`, `system/`):

1.  **Commit & Push**: Push your changes to `main` (or merge a PR).
    ```bash
    git push origin main
    ```
2.  **Wait for Build**: The GitHub Action must complete building the new image.
    ```bash
    gh run watch
    ```
3.  **Fetch & Stage**: Pull the new image to the host.
    ```bash
    sudo bootc upgrade
    ```
4.  **Reboot**: Apply the staged image.
    ```bash
    systemctl reboot
    ```


### What Gets Captured

| Subsystem   | What's captured                                   |
| ----------- | ------------------------------------------------- |
| `flatpak`   | User-installed Flatpak apps (with pinned commits) |
| `extension` | GNOME Shell extensions (enabled/disabled state)   |
| `dnf`       | rpm-ostree layered packages                       |

**Not auto-captured:**

- GSettings (use `bkt gsetting capture <schema>` for specific schemas)
- Shims (added intentionally, not discovered)

### Capture Individual Subsystems

```bash
# Capture only flatpaks
bkt flatpak capture --apply

# Capture only extensions
bkt extension capture --apply

# Capture only layered packages
bkt dnf capture --apply

# Capture specific gsettings schema
bkt gsetting capture org.gnome.desktop.interface
```

## Applying Manifests to System

To sync your system to match the manifests:

```bash
# Preview what would be applied
bkt apply --dry-run

# Apply all manifests
bkt apply

# Apply only specific subsystems
bkt apply --only flatpak
bkt apply --only extension,gsetting
bkt apply --exclude dnf
```

### What Gets Applied

| Subsystem   | What happens                                               |
| ----------- | ---------------------------------------------------------- |
| `flatpak`   | Installs missing Flatpak apps                              |
| `extension` | Enables extensions from manifest (must be installed first) |
| `gsetting`  | Applies GSettings values                                   |
| `dnf`       | Installs rpm-ostree layered packages                       |
| `shim`      | Creates host shim scripts                                  |

**Note:** Extension sync only _enables_ extensions. It doesn't install them. To install new extensions:

1. Add to manifest via `bkt extension add <uuid>`
2. Install via Extension Manager or wait for next image bootstrap

## Adding Things Manually

### Add a Flatpak

```bash
# Add and install
bkt flatpak add org.gnome.Boxes

# Just add to manifest (don't install yet)
bkt flatpak add org.gnome.Boxes --pr-only
```

### Add a GNOME Extension

```bash
# Add to manifest (will enable if already installed)
bkt extension add dash-to-dock@micxgx.gmail.com

# Verify it's in the manifest
bkt extension list
```

### Add a GSettings Value

```bash
# Set a value (validates schema exists)
bkt gsetting set org.gnome.desktop.interface color-scheme prefer-dark

# Skip validation
bkt gsetting set org.gnome.desktop.interface color-scheme prefer-dark --force
```

### Add a Host Shim

```bash
# Simple shim (name = command)
bkt shim add nmcli

# Aliased shim (different name)
bkt shim add dc docker-compose

# Sync shims to scripts
bkt shim sync
```

## Checking Status

```bash
# Overall status
bkt status

# Check for drift
bkt drift check
```

## For Things That Require Reboot

System packages, fonts, and configs baked into the image require editing the Containerfile:

```bash
cd ~/Code/Config/bootc

# Edit Containerfile to add a package
vim Containerfile
# Add: RUN rpm-ostree install your-package

# Commit and push
git add Containerfile && git commit -m "feat: add package"
git push

# Wait for CI to build (~10 min)

# Upgrade and reboot
bkt admin bootc upgrade --confirm
systemctl reboot
```

## Developing in the Toolbox

### Enter the toolbox

```bash
toolbox enter
```

### Toolbox Packages

```bash
# Install in toolbox (and capture to manifest)
bkt dev dnf install gcc

# See what's in the toolbox manifest
bkt dev dnf list
```

### Host Commands from Toolbox

These work transparently via shims:

```bash
bootc status      # Delegates to host
systemctl status  # Delegates to host
podman ps         # Delegates to host
```

If a command isn't available, add a shim:

```bash
# Works from both host and toolbox (auto-delegates)
bkt shim add nmcli
```

## Recovery

Something broke after upgrading?

1. Reboot
2. In GRUB, select the previous deployment
3. You're back to working state
4. Fix the issue in repo, push, try again

bootc always keeps the previous deployment.

## Quick Reference

### Common Tasks (work from anywhere)

| Task                    | Command                             |
| ----------------------- | ----------------------------------- |
| Capture all changes     | `bkt capture --apply`               |
| Apply all manifests     | `bkt apply`                         |
| Add a flatpak           | `bkt flatpak add <app-id>`          |
| Add an extension        | `bkt extension add <uuid>`          |
| Check status            | `bkt status`                        |
| Upgrade system          | `bkt admin bootc upgrade --confirm` |
| Install toolbox package | `bkt dev dnf install <pkg>`         |
