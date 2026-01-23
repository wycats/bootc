# RFC 0018: Host-Only Shims

- Feature Name: `host_only_shims`
- Start Date: 2026-01-22
- RFC PR: (leave this empty until PR is opened)
- Tracking Issue: (leave this empty)

## Summary

Add a `host_only` option to the shims manifest that creates direct symlinks instead of distrobox-enter wrappers. This supports the rare case of binaries built inside the container that must run directly on the host.

## Motivation

### The Distrobox Shim Model

When you run `cargo install ripgrep` inside the distrobox container, the binary lands in `~/.cargo/bin/rg`. To use it from the host, we create a shim in `~/.local/bin/distrobox/rg` that wraps the call:

```bash
#!/bin/sh
# distrobox_binary
# name: bootc-dev
if [ -z "${CONTAINER_ID}" ]; then
    exec "/usr/bin/distrobox-enter" -n bootc-dev -- '/home/user/.cargo/bin/rg' "$@"
elif [ -n "${CONTAINER_ID}" ] && [ "${CONTAINER_ID}" != "bootc-dev" ]; then
    exec distrobox-host-exec '/home/user/.local/bin/distrobox/rg' "$@"
else
    exec '/home/user/.cargo/bin/rg' "$@"
fi
```

This works transparently for most tools because distrobox shares:

- The home directory (bind-mounted)
- D-Bus session bus
- PID namespace (host's systemd is PID 1)
- Network namespace

For tools like `rg`, `fd`, `cargo`, `node` — the container is invisible.

### The Problem Case

Some tools have system integration requirements that don't work through distrobox:

| Tool     | Requirement                    | Why shim fails                         |
| -------- | ------------------------------ | -------------------------------------- |
| `locald` | Setuid shim, cgroup management | Needs host privileges directly         |
| `bkt`    | polkit integration, systemctl  | Works via delegation, but adds latency |

For these tools, we need to run the binary directly on the host, not through `distrobox-enter`.

### Current Workaround

Users manually symlink the binary to `~/.local/bin`:

```bash
ln -sf ~/Code/locald/target/release/locald ~/.local/bin/locald
```

This works but:

1. Isn't captured in the manifest
2. Requires manual intervention
3. Isn't reproducible

## Guide-level Explanation

### Basic Usage

Most shims are normal distrobox wrappers:

```bash
bkt shim add cargo                # Creates distrobox wrapper
bkt shim add rg                   # Creates distrobox wrapper
```

For tools that must run directly on the host:

```bash
bkt shim add locald --host-only   # Creates direct symlink
bkt shim add bkt --host-only      # Creates direct symlink
```

### Manifest Representation

```json
{
  "shims": [
    { "name": "cargo", "source": "~/.cargo/bin/cargo" },
    { "name": "rg", "source": "~/.cargo/bin/rg" },
    { "name": "locald", "source": "~/.cargo/bin/locald", "host_only": true },
    { "name": "bkt", "source": "~/.cargo/bin/bkt", "host_only": true }
  ]
}
```

### What Gets Created

**Normal shim** (`host_only: false` or omitted):

```
~/.local/bin/distrobox/cargo → shell script with distrobox-enter wrapper
```

**Host-only shim** (`host_only: true`):

```
~/.local/bin/cargo → symlink to ~/.cargo/bin/cargo
```

Note: Host-only shims go to `~/.local/bin`, not `~/.local/bin/distrobox`. This is because:

1. They run directly, not through distrobox
2. `~/.local/bin` comes after `~/.local/bin/distrobox` in PATH, so normal shims take precedence

### Listing Shims

```bash
$ bkt shim list
NAME      SOURCE                    MODE
cargo     ~/.cargo/bin/cargo        distrobox
rg        ~/.cargo/bin/rg           distrobox
node      ~/.proto/shims/node       distrobox
locald    ~/.cargo/bin/locald       host-only
bkt       ~/.cargo/bin/bkt          host-only
```

### Removing Host-Only Shims

```bash
bkt shim remove locald   # Removes symlink and manifest entry
```

## Reference-level Explanation

### Manifest Schema Update

```json
{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "type": "object",
  "properties": {
    "shims": {
      "type": "array",
      "items": {
        "type": "object",
        "properties": {
          "name": { "type": "string" },
          "source": { "type": "string" },
          "host_only": { "type": "boolean", "default": false }
        },
        "required": ["name", "source"]
      }
    }
  }
}
```

### Shim Generation Logic

```rust
pub fn generate_shim(shim: &ShimConfig) -> Result<()> {
    let target_dir = if shim.host_only {
        dirs::home_dir().unwrap().join(".local/bin")
    } else {
        dirs::home_dir().unwrap().join(".local/bin/distrobox")
    };

    let target_path = target_dir.join(&shim.name);
    let source_path = expand_home(&shim.source);

    if shim.host_only {
        // Create symlink
        std::os::unix::fs::symlink(&source_path, &target_path)?;
    } else {
        // Create distrobox wrapper script
        let script = generate_distrobox_wrapper(&shim.name, &source_path);
        std::fs::write(&target_path, script)?;
        set_executable(&target_path)?;
    }

    Ok(())
}
```

### PATH Considerations

With the PATH architecture from VISION.md:

```
PATH="$HOME/.local/bin/distrobox:$HOME/.local/bin:..."
```

- Normal shims in `~/.local/bin/distrobox/` are found first
- Host-only symlinks in `~/.local/bin/` are found second
- If both exist for the same name, the distrobox shim wins

This is intentional: if you have both a distrobox shim and a host-only symlink for `cargo`, the shim takes precedence. To override, remove the shim.

### Capture Behavior

`bkt shim capture` should detect existing symlinks in `~/.local/bin` and mark them as `host_only: true`:

```rust
pub fn capture_shims() -> Vec<ShimConfig> {
    let mut shims = Vec::new();

    // Capture distrobox shims
    for entry in read_dir("~/.local/bin/distrobox")? {
        if is_distrobox_shim(&entry) {
            shims.push(ShimConfig {
                name: entry.file_name(),
                source: extract_source_from_shim(&entry),
                host_only: false,
            });
        }
    }

    // Capture host-only symlinks
    for entry in read_dir("~/.local/bin")? {
        if entry.is_symlink() && points_to_cargo_or_proto(&entry) {
            shims.push(ShimConfig {
                name: entry.file_name(),
                source: entry.read_link()?,
                host_only: true,
            });
        }
    }

    shims
}
```

## Drawbacks

### Complexity

Adding another mode increases complexity. Users must understand when to use `--host-only`.

**Mitigation**: This is a rare case. Most tools work fine through distrobox shims. Document clearly when `--host-only` is needed.

### Two Locations

Host-only shims go to `~/.local/bin`, normal shims to `~/.local/bin/distrobox`. This could be confusing.

**Mitigation**: `bkt shim list` shows the mode clearly. The distinction is meaningful: distrobox shims are managed by distrobox export, host-only are just symlinks.

## Rationale and Alternatives

### Alternative: Always Direct Symlink

We could abandon the distrobox shim model entirely and just symlink everything.

**Rejected**: The distrobox shim model is correct for 95%+ of tools. Tools like `cargo`, `rustc`, `node` work perfectly through shims. Only system integration tools need direct access.

### Alternative: Container Detection in Tools

Tools like `locald` could detect they're in a container and delegate to the host (like `bkt` does with `flatpak-spawn`).

**Rejected for general use**: This requires each tool to implement delegation logic. The shim approach is simpler and works for any binary.

**Note**: `bkt` already has this delegation logic and it works, but it adds latency and complexity. For frequently-run tools, direct execution is better.

### Alternative: Build on Host

For tools that need host privileges, build them on the host (install gcc on host).

**Rejected**: Pollutes the immutable host with build tools. The whole point of the distrobox model is keeping dev tools in the container.

## Prior Art

- **distrobox-export**: Creates the wrapper scripts we use for normal shims
- **Nix home-manager**: Manages symlinks to Nix-built binaries
- **asdf shims**: Similar wrapper approach for version management

## Unresolved Questions

### Q1: Should `bkt` itself use host-only?

`bkt` currently works via delegation (`flatpak-spawn --host`), which adds latency but works. Should we recommend `--host-only` for `bkt`?

**Tentative answer**: Yes, for installed `bkt`. The delegation logic was designed for the toolbx era. With distrobox's transparency, direct execution is simpler.

### Q2: Auto-detection?

Should we auto-detect that a binary needs `--host-only` based on capabilities (setuid bit, container checks in the binary)?

**Tentative answer**: No. This is error-prone and surprising. Explicit is better.

## Future Possibilities

### Cargo Install Integration

```bash
bkt cargo install --host-only locald
```

This would:

1. Run `cargo install locald` in the container
2. Create a host-only symlink to the resulting binary

### Build-and-Export Workflow

For locally-built tools:

```bash
bkt dev build locald              # Build in container
bkt shim add locald --host-only   # Export to host
```

This is the expected workflow for tools like `locald` and `bkt` during development.
