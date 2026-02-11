# RFC 0024: Doctor Command

One-line summary: Provide `bkt doctor` to validate system readiness and preflight requirements for PR workflows.

## Motivation

PR workflows depend on a stable environment (git, gh, repo configuration, and tooling shims). Users need a single check that verifies these assumptions and provides actionable fixes before they attempt a PR-based command.

## Design

`bkt doctor` runs preflight checks and reports them in a consistent format.

Command surface:

- `bkt doctor [--format table|json]`.

Checks performed:

- Standard preflight checks used by PR workflows (repo state, tooling availability, authentication).
- Distrobox shim validation and PATH precedence.
- Devtool resolution checks to ensure commands resolve to the distrobox wrappers when present.

Output:

- Table output includes pass/fail status, a short message, and a fix hint when applicable.
- JSON output includes `name`, `passed`, `message`, and `fix_hint` for automation.

## Implementation Notes

- The command is implemented in [bkt/src/commands/doctor.rs](bkt/src/commands/doctor.rs).
- Core preflight checks come from `run_preflight_checks` and are reused from PR workflows.
- PATH validation ensures `~/.local/bin/distrobox` exists and appears before host toolchain paths such as `~/.cargo/bin` and `~/.proto/shims`.
- Wrapper validation inspects files in `~/.local/bin/distrobox` and confirms they contain the `distrobox_binary` marker.
- Resolution checks verify that `cargo`, `node`, and `pnpm` resolve to distrobox shims when those shims exist.

## Known Gaps

None.
