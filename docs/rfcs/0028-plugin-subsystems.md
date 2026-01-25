# RFC-0028: Plugin Subsystems

- **Status**: Draft
- **Created**: 2026-01-24
- **Depends on**: None

## Summary

Enable third-party subsystems to be discovered and registered without modifying the core `bkt` codebase.

This RFC introduces a declarative **Phase 1** plugin system that covers most use cases via a `plugin.json` schema. A future **Phase 2** proposes sandboxed WASM plugins using Extism for advanced introspection, pending community demand.

## Motivation

`bkt` has grown to support multiple subsystems (flatpaks, gsettings, distrobox, etc.). As new domains emerge, requiring core changes for each subsystem slows iteration and makes local experimentation cumbersome. We want:

- **Extensibility**: Add new subsystems without patching `bkt` itself
- **Safety**: Explicit opt-in to prevent untrusted code execution
- **Consistency**: Plugins integrate with existing capture/sync flows
- **Portability**: Plugins can be shared and installed like other config assets

## Goals

- Allow third-party subsystems to register via a declarative `plugin.json`
- Support capture and sync workflows via subprocess execution
- Maintain a stable JSON interface for plugin output
- Provide discovery via standard XDG paths
- Require explicit config opt-in for security

## Non-Goals

- A universal plugin API for arbitrary Rust extensions
- Inline script execution from manifests
- Auto-loading untrusted plugins without user approval
- A WASM runtime in the initial release

## Guide-level Explanation

### Plugin Layout

```
my-subsystem/
├── plugin.json
├── my-manifest.json
└── README.md
```

### plugin.json Example

```json
{
  "id": "my-subsystem",
  "name": "My Custom Subsystem",
  "manifest_file": "my-manifest.json",
  "capabilities": ["capture", "sync"],
  "capture_command": ["my-tool", "capture", "--json"],
  "sync_command": ["my-tool", "sync", "--manifest", "{manifest_path}"]
}
```

### Discovery Paths

Plugins are discovered in standard locations:

- `~/.config/bkt/plugins/`
- `/usr/share/bkt/plugins/`

### Explicit Opt-in

For security, discovered plugins are **not** enabled automatically. The user must opt in explicitly:

```toml
# ~/.config/bkt/config.toml
[plugins]
allowed = ["my-subsystem", "org.example.hardware"]
```

Only plugins listed in `allowed` are loaded and executed.

## Reference-level Explanation

### Schema Overview

A plugin is a directory containing `plugin.json`. The schema is intentionally small and declarative.

```json
{
  "id": "my-subsystem",
  "name": "My Custom Subsystem",
  "manifest_file": "my-manifest.json",
  "capabilities": ["capture", "sync"],
  "capture_command": ["my-tool", "capture", "--json"],
  "sync_command": ["my-tool", "sync", "--manifest", "{manifest_path}"]
}
```

#### Fields

- `id` (string, required): Stable identifier, used for opt-in and registry keys.
- `name` (string, required): Human-readable label.
- `manifest_file` (string, required): Plugin-local manifest path, relative to plugin directory.
- `capabilities` (array, required): Supported operations; valid values are `capture`, `sync`.
- `capture_command` (array, optional): Command used to capture system state, must emit JSON to stdout.
- `sync_command` (array, optional): Command used to apply the manifest to the system.

Commands are executed as subprocesses by the registry. JSON output is parsed and merged into the manifest system. For `sync_command`, `bkt` substitutes `{manifest_path}` with the resolved plugin manifest path.

### Registry Behavior

- **Load**: Discover plugin directories in XDG paths.
- **Validate**: Parse `plugin.json` against schema; reject invalid plugins.
- **Filter**: Only allow plugins present in config `plugins.allowed`.
- **Execute**: Run commands in a subprocess with a minimal environment.
- **Parse**: Require well-formed JSON from `capture_command`.

### Example Plugin Structure

```
~/.config/bkt/plugins/my-subsystem/
├── plugin.json
├── my-manifest.json
└── scripts/
    └── my-tool
```

### JSON Output Contract

`capture_command` must emit a JSON object compatible with the manifest system. The registry treats the plugin’s output as a top-level namespace keyed by the plugin `id`:

```json
{
  "my-subsystem": {
    "items": ["example"],
    "metadata": {
      "captured_at": "2026-01-24T12:00:00Z"
    }
  }
}
```

The exact schema of the plugin payload is the plugin’s responsibility, but it must remain stable for diffing and sync.

## Implementation Plan

### Phase 1: Declarative Config Plugins (Initial Release)

Focus on covering the majority of use cases with a simple JSON schema and subprocess execution.

- Define `plugin.json` schema and validation
- Add XDG discovery (`~/.config/bkt/plugins/`, `/usr/share/bkt/plugins/`)
- Add explicit opt-in list in config
- Implement registry: load, validate, execute, parse
- Wire capture and sync into existing command flow

### Phase 2: WASM Plugins (Future)

For advanced introspection and safer sandboxed execution, introduce a WASM runtime using Extism.

- `bkt` loads WASM modules from plugin directories
- Plugins interact via a stable ABI for capture/sync
- Execution is sandboxed and resource-limited

**Note**: This phase is deferred until real community demand emerges.

## Security Considerations

- Plugins are untrusted by default.
- Execution requires explicit opt-in per plugin `id`.
- Commands run with least privilege available to `bkt`.
- Future WASM sandboxing reduces risk for complex plugins.

## Alternatives Considered

- **Native Rust plugin API**: Too brittle; requires ABI stability and compiler alignment.
- **Inline scripts in manifests**: Hard to secure and audit; rejected.
- **Auto-enable plugins**: Violates least-privilege principle.

## Open Questions

- Should plugins be allowed to declare additional config sections?
- How should plugin versions be tracked (semver in `plugin.json`)?
- Do we want a `bkt plugin doctor` command to validate execution locally?
