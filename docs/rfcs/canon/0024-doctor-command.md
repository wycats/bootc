# RFC 0024: Doctor Command

- **Status**: Implemented
- **Created**: 2026-01-25
- **Updated**: 2026-02-23

One-line summary: Provide `bkt doctor` to validate system readiness and preflight requirements for PR workflows, with optional `--fix` to remediate common issues.

## Motivation

PR workflows depend on a stable environment (git, gh, repo configuration, and tooling shims). Users need a single check that verifies these assumptions and provides actionable fixes before they attempt a PR-based command.

## Design

`bkt doctor` runs preflight checks and reports them in a consistent format.

### Command Surface

```bash
bkt doctor [--format table|json]
bkt doctor --fix [--yes]
```

### Checks Performed

- Standard preflight checks used by PR workflows (repo state, tooling availability, authentication).
- Distrobox shim validation and PATH precedence.
- Devtool resolution checks to ensure commands resolve to the distrobox wrappers when present.

### Output

- Table output includes pass/fail status, a short message, and a fix hint when applicable.
- JSON output includes `name`, `passed`, `message`, and `fix_hint` for automation.

## The `--fix` Flag (Proposed)

When `bkt doctor --fix` is run, doctor attempts to remediate fixable issues automatically.

### Fixable Issues

| Check                                        | Fix Action                                                                        |
| -------------------------------------------- | --------------------------------------------------------------------------------- |
| PATH precedence (host devtools shadow shims) | Move `~/.cargo` and `~/.proto` to `~/.cache/bootc/disabled-devtools/<timestamp>/` |
| Missing distrobox shims                      | Run `bkt distrobox apply`                                                         |
| Stale shim wrappers                          | Regenerate via `bkt distrobox apply --force`                                      |

### Behavior

- Without `--yes`, prompts for confirmation before destructive actions (moving directories).
- With `--yes`, proceeds without prompts (for automation).
- Non-fixable issues are reported but not attempted.
- After fixes, re-runs checks to confirm resolution.

### Example

```bash
$ bkt doctor
✗ PATH precedence: ~/.cargo/bin shadows distrobox shims
  Fix: bkt doctor --fix

$ bkt doctor --fix
⚠ Will move ~/.cargo to ~/.cache/bootc/disabled-devtools/20260223-143000/
  Continue? [y/N] y
→ Moving ~/.cargo...
→ Moving ~/.proto...
✓ Devtools relocated

Re-checking...
✓ PATH precedence: distrobox shims take priority
✓ All checks passed
```

## Implementation Notes

- The command is implemented in [bkt/src/commands/doctor.rs](bkt/src/commands/doctor.rs).
- Core preflight checks come from `run_preflight_checks` and are reused from PR workflows.
- PATH validation ensures `~/.local/bin/distrobox` exists and appears before host toolchain paths such as `~/.cargo/bin` and `~/.proto/shims`.
- Wrapper validation inspects files in `~/.local/bin/distrobox` and confirms they contain the `distrobox_binary` marker.
- Resolution checks verify that `cargo`, `node`, and `pnpm` resolve to distrobox shims when those shims exist.
- The `--fix` logic absorbs functionality from `scripts/prune-host-devtools`.

## Migration

The `scripts/prune-host-devtools` script is superseded by `bkt doctor --fix`. Once `--fix` is implemented, the script should be removed.
