# RFC 0001: Command Punning Philosophy (Current)

- **Status**: Foundational
- Feature Name: `command_punning`
- Start Date: 2025-12-31
- RFC PR: (leave this empty until PR is opened)
- Tracking Issue: (leave this empty)

## Summary

`bkt` uses command punning: commands mirror familiar tools but do two things at once.

1. Apply the change immediately (host or toolbox)
2. Record the change in manifests and open a PR (unless `--local` or `--pr-only` is used)

The goal is to keep the fast feedback loop of imperative commands while still keeping the distribution declarative.

## Guide-level Explanation

### Command Shape

```
bkt [--context <host|dev|image>] <domain> <action> [options] [args...]
```

- **Context** controls where the command executes immediately.
- **Domain + action** maps to a manifest and a concrete system operation.

### Execution Contexts

| Context | How to select                    | Immediate effect       | Notes                                            |
| ------- | -------------------------------- | ---------------------- | ------------------------------------------------ |
| Host    | default, or `--context host`     | Applies to the host OS | Flatpak, GSettings, extensions, shims, AppImages |
| Dev     | `bkt dev ...` or `--context dev` | Runs in the toolbox    | `dnf` installs and toolbox-specific changes      |
| Image   | `--context image` or `--pr-only` | No immediate effect    | Manifest-only updates for next build             |

### PR Modes

| Flag        | Behavior                                              |
| ----------- | ----------------------------------------------------- |
| (none)      | Execute locally and create PR                         |
| `--local`   | Execute locally only, track in the ephemeral manifest |
| `--pr-only` | Update manifests and create PR only                   |

Local-only changes are tracked in `~/.local/share/bkt/ephemeral.json` and can be promoted later with `bkt local commit`.

### Concrete Examples

#### Host (default)

```bash
# Install a Flatpak now + update manifests + PR
bkt flatpak add org.gnome.Calculator

# Apply a GSetting now + update manifests + PR
bkt gsetting set org.gnome.desktop.interface color-scheme "'prefer-dark'"

# Enable a GNOME extension now + update manifests + PR
bkt extension enable dash-to-dock@micxgx.gmail.com
```

#### Dev toolbox

```bash
# Install in the toolbox now + update toolbox manifest
bkt dev install gcc

# Update manifest only (no dnf)
bkt dev install --manifest-only clang
```

#### Image-only (manifest updates, no immediate effect)

```bash
# Prepare a PR without local changes
bkt flatpak add --pr-only org.gnome.TextEditor

# Same intent with explicit image context
bkt --context image gsetting set org.gnome.desktop.interface clock-show-seconds true
```

### Where Punning Is Deferred

Not every domain can apply immediately:

- `bkt system add/remove` updates `manifests/system-packages.json` and the Containerfile, but packages only exist after the image rebuild.
- `bkt admin kargs` updates `manifests/system-config.json` only (no immediate kernel change).

These commands still follow the same manifest + PR workflow but are intentionally deferred.

## Reference-level Notes

### Domains and Typical Actions

- `flatpak`: `add`, `remove`, `list`, `sync`, `capture`
- `extension`: `add`, `remove`, `enable`, `disable`, `list`, `sync`, `capture`
- `gsetting`: `set`, `unset`, `list`, `apply`, `capture`
- `appimage`: `add`, `remove`, `enable`, `disable`, `list`, `sync`, `capture`
- `system`: `add`, `remove`, `list`, `capture` (deferred)
- `dev`: `install`, `remove`, `list`, `sync`, `capture`, `enter`
- `distrobox`: `apply`, `capture`

### Local Change Promotion

```bash
bkt flatpak add --local org.gnome.Calculator
bkt gsetting set --local org.gnome.desktop.interface color-scheme "'prefer-dark'"

bkt local list
bkt local commit
```

## Gaps

- Image-only and deferred domains do not apply changes immediately by design.
- Some domains have specialized workflows (`bkt apply`, `bkt capture`) instead of a single punning command.

## Unresolved Questions

- Should image-only workflows grow explicit subcommands, or remain as `--context image` plus existing commands?
