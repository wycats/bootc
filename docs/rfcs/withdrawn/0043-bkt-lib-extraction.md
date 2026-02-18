# RFC 0043: bkt lib extraction + CommandRunner trait

**Feature**: testing
**Stage**: Withdrawn

## Withdrawal Rationale

This RFC proposed extracting bkt into a library with a `CommandRunner` trait to enable in-process testing. However, [RFC-0050: Persistent Host-Command Helper](../0050-persistent-host-command-helper.md) provides a simpler solution to the underlying performance problem:

- **Daemon approach** (RFC-0050): Reduces cross-boundary latency from ~120ms to ~4ms by eliminating D-Bus overhead
- **Lib extraction** (this RFC): Would require significant refactoring for marginal additional gains

The daemon solution addresses the root cause (D-Bus/podman overhead) without requiring architectural changes to bkt. The lib extraction remains a valid future optimization but is no longer necessary for the immediate performance goals.

---

## Original Proposal (Archived)

## Problem

The bkt integration tests (`tests/cli.rs`, 72 tests) invoke the compiled binary via `assert_cmd`. When run from the host, each `bkt` invocation goes through the distrobox shim → `podman exec`, generating D-Bus traffic. At default cargo test parallelism, this produces ~700 podman execs in 2 minutes, which exhausted dbus-broker's soft fd limit (1,024) and crashed gnome-shell.

Even with the fd limit fix (PR #114), the tests are needlessly slow — each subprocess spawn costs ~50ms of podman overhead vs ~0.1ms for an in-process call.

## Root Cause

`bkt` is a binary-only crate. The command handlers in `src/commands/*.rs` are only reachable through `fn main()` → clap → dispatch. Integration tests must shell out to the binary even when testing pure manifest I/O that has zero external dependencies.

## Analysis

### Call site inventory

87 `Command::new()` call sites across 15 external commands:

| Command          | Sites | Used by test paths?          |
| ---------------- | ----- | ---------------------------- |
| git              | 21    | No (PR/commit workflows)     |
| gsettings        | 8     | Only in sync/apply/capture   |
| flatpak          | 8     | Only in sync/capture         |
| gnome-extensions | 7     | Only in sync/capture         |
| gh               | 7     | No (PR workflows)            |
| dnf/dnf5         | 7     | Only in dev install/remove   |
| rpm              | 5     | Only in base capture         |
| brew             | 5     | Only in homebrew sync        |
| toolbox          | 3     | Only in dev enter/status     |
| podman           | 3     | Only in distrobox/build-info |
| Others           | 13    | Various                      |

### Test path analysis

The high-frequency test commands (shim, gsetting, extension, flatpak, base — CRUD operations) are **pure manifest I/O**:

- `shim.rs`: **0** `Command::new` calls — entirely pure
- `gsetting set --force --local`: skips gsettings apply — pure
- `extension add/remove --local`: skips gnome-extensions — pure
- `flatpak list`: reads manifest — pure
- `base list`: reads manifest — pure

The external commands cluster in `sync`/`apply`/`capture` code paths.

## Design

### Phase 1: Lib extraction

Add `[lib]` to `bkt/Cargo.toml` alongside the existing `[[bin]]`. Create `src/lib.rs` that re-exports the module tree. The binary becomes a thin shell:

```rust
// src/main.rs
fn main() -> anyhow::Result<()> {
    bkt::cli::run()
}
```

Integration tests can then call command handlers in-process:

```rust
use bkt::context::Context;
use bkt::commands::shim;

#[test]
fn shim_add_and_list() {
    let ctx = Context::temp();
    shim::add(&ctx, "test-shim", None).unwrap();
    let output = shim::list(&ctx).unwrap();
    assert!(output.contains("test-shim"));
}
```

This eliminates subprocess spawns for all pure manifest I/O tests (~90% of the test suite).

### Phase 2: CommandRunner trait

```rust
pub trait CommandRunner: Send + Sync {
    fn run(&self, program: &str, args: &[&str]) -> Result<std::process::Output>;
    fn run_status(&self, program: &str, args: &[&str]) -> Result<ExitStatus>;
}

pub struct SystemRunner;
impl CommandRunner for SystemRunner {
    fn run(&self, program: &str, args: &[&str]) -> Result<Output> {
        std::process::Command::new(program)
            .args(args)
            .output()
            .with_context(|| format!("{program} failed"))
    }
    fn run_status(&self, program: &str, args: &[&str]) -> Result<ExitStatus> {
        std::process::Command::new(program)
            .args(args)
            .status()
            .with_context(|| format!("{program} failed"))
    }
}
```

Thread through `Context` (which already exists and is passed everywhere):

```rust
pub struct Context {
    pub runner: Box<dyn CommandRunner>,
    // ... existing fields
}
```

Replace `Command::new("gsettings")` → `ctx.runner.run("gsettings", &[...])` across 87 call sites.

Test stub:

```rust
pub struct StubRunner {
    responses: HashMap<String, Output>,
}
```

This makes `sync`/`apply`/`capture` testable without real gsettings/flatpak/gnome-extensions.

### Phasing

Both phases land in a single PR. Phase 1 is the structural change (lib.rs, Cargo.toml, main.rs thinning). Phase 2 is the mechanical `Command::new` → `ctx.runner.run` replacement. The test migration can be incremental — existing binary tests continue to work, new in-process tests are added alongside.

### What stays as binary tests

- CLI argument parsing (clap wiring): `--help`, missing required args, `--version`
- Smoke tests that verify the binary runs at all
- Any test that specifically validates the CLI output format

These are ~25 tests, low frequency, cheap even as subprocesses.

## Impact

- Eliminates ~700 podman exec invocations per test run
- Test suite runs in seconds instead of minutes
- No more risk of crashing the desktop session
- sync/apply/capture become testable without real system state
- Foundation for CI testing without a full desktop environment
