# Manifests Directory

This directory contains declarative manifests for the bootc distribution.

## Manifest Types

### base-image-assumptions.json

**Purpose**: Documents what the upstream Bazzite image provides (packages, services, paths).

**Design Intent**: This is a **reference document** that tracks upstream state, NOT a list of what we need to install. It enables:

- **Drift detection** when Bazzite changes
- **CI verification** that Bazzite still provides expected packages
- **Documentation** of our dependencies on the base image

**Important**: This manifest does NOT control what gets installed. Packages listed here are expected to already exist in the Bazzite base image.

**Commands**:

- `bkt base list` - Show what we expect Bazzite to provide
- `bkt base verify` - Verify Bazzite still provides expected packages
- `bkt base assume <pkg>` - Record that Bazzite provides a package
- `bkt base unassume <pkg>` - Remove a package from tracking

### system-packages.json

**Purpose**: Packages explicitly installed by the user on the host (layered via rpm-ostree).

**Design Intent**: YOUR additions beyond what Bazzite provides. These get added to the Containerfile's `RUN dnf install` section.

**Commands**:

- `bkt dnf install <pkg>` - Add a package
- `bkt dnf remove <pkg>` - Remove a package
- `bkt packages list` - List packages

### toolbox-packages.json

**Purpose**: Packages explicitly installed by the user in the development toolbox.

**Design Intent**: Development tools that should be available in the toolbox container.

**Commands**:

- `bkt dev install <pkg>` - Add a package to the toolbox manifest

### flatpak-apps.json

**Purpose**: Flatpak applications to install on first boot.

**Commands**:

- `bkt flatpak install <app>` - Add an application
- `bkt flatpak remove <app>` - Remove an application
- `bkt flatpak list` - List applications

### flatpak-remotes.json

**Purpose**: Flatpak remotes (repositories) to configure.

### gnome-extensions.json

**Purpose**: GNOME Shell extensions to enable.

**Commands**:

- `bkt extensions enable <ext>` - Enable an extension
- `bkt extensions disable <ext>` - Disable an extension
- `bkt extensions list` - List extensions

### gsettings.json

**Purpose**: GSettings (dconf) entries to apply.

**Commands**:

- `bkt gsettings set <path> <key> <value>` - Set a value
- `bkt gsettings list` - List managed settings

### host-shims.json

**Purpose**: Host commands to make available in the toolbox via shims.

**Design Intent**: Commands that should delegate to the host system (e.g., `flatpak`, `rpm-ostree`, `bootc`).

## Manifest Separation Philosophy

The manifests are organized to maintain a clear separation of concerns:

```
manifests/
├── base-image-assumptions.json  # What Bazzite provides (upstream reference)
├── system-packages.json         # Host packages YOU added (managed by bkt)
├── toolbox-packages.json        # Toolbox packages YOU added (managed by bkt)
├── flatpak-apps.json            # Apps to install at first login
├── gnome-extensions.json        # Extensions to enable
├── host-shims.json              # Commands to delegate to host
└── gsettings.json               # GNOME settings to apply
```

This separation ensures:

1. **Drift detection** only flags changes to user-managed packages
2. **Base image changes** are detected separately via `bkt base verify`
3. **Clear ownership** - we know what came from Bazzite vs. what we added
