# RFC-0010: Transparent Command Delegation

- **Status**: Implemented (daemon optimization in progress)
- **Created**: 2026-01-05
- **Updated**: 2026-02-20
- **Depends on**: RFC-0009 (Privileged Operations)
- **Related**: [RFC-0017](../0017-distrobox-integration.md) (Distrobox Integration)
- **Absorbs**: RFC-0050 (Persistent Host-Command Helper)

> **✅ This RFC is implemented.**
>
> When a host-only command (like `bkt status`) runs inside a container, it
> automatically re-executes on the host via `flatpak-spawn --host`.
>
> This supersedes [RFC-0018: Host-Only Shims](../withdrawn/0018-host-only-shims.md), which
> proposed a different approach that was never implemented.
>
> **Implementation Notes (2026-02-20):**
>
> - Use `flatpak-spawn --host --env=BKT_DELEGATED=1` (not `distrobox-host-exec`)
>   - `distrobox-host-exec` doesn't forward env vars; `flatpak-spawn --env=` does
>   - This is the underlying mechanism distrobox uses anyway (via host-spawn → D-Bus)
> - `BKT_DELEGATED=1` prevents recursion when `bkt` is exported as a distrobox shim
> - `Status`, `Doctor`, `Profile`, `Base` are **Host** (not Either) — they read system manifests
> - See [RFC-0017](../0017-distrobox-integration.md) for why `bkt` must be excluded from distrobox exports
> - **Daemon optimization** (Phase 1 complete): See [Performance Optimization](#performance-optimization-host-command-daemon)

## Summary

Commands that must run on the host should work identically from both the host
and the toolbox. When run from the toolbox, `bkt` automatically delegates to
the host via `flatpak-spawn --host`.

This RFC defines a **CommandTarget** enum and early delegation mechanism that
makes this transparent to both users and command implementations.

**Note**: This RFC intentionally does NOT propose a unified `Command` trait.
After analyzing all 18 command modules, we found that the variation between
command types (plan-based vs. read-only vs. utility) is too great for a single
trait to be useful. See [Appendix: Why Not a Command Trait?](#appendix-why-not-a-command-trait)
for the full analysis.

## Motivation

### Current Pain Point

Currently, most `bkt` commands fail when run from the toolbox:

```bash
# In toolbox:
$ bkt flatpak add org.gnome.Boxes
Error: Invalid context for domain

Flatpaks are host-level applications.
Did you mean to run this on the host instead of in the dev toolbox?
```

The user must then:

1. Exit the toolbox, OR
2. Manually run `flatpak-spawn --host bkt flatpak add org.gnome.Boxes`

This friction violates the "it just works" philosophy.

### The Goal

```bash
# In toolbox (works!):
$ bkt flatpak add org.gnome.Boxes
→ Delegating to host...
✓ Added org.gnome.Boxes to manifest
```

The only command that currently achieves this is `bkt admin bootc`, which
manually implements delegation. This RFC generalizes that pattern.

## Key Concepts

### RuntimeEnvironment vs ExecutionContext

The codebase has two distinct concepts that must be understood:

| Concept                | Question                       | Values                   | Example                          |
| ---------------------- | ------------------------------ | ------------------------ | -------------------------------- |
| **RuntimeEnvironment** | "Where is bkt _running_?"      | Host, Toolbox, Container | Detected via `/run/.toolboxenv`  |
| **ExecutionContext**   | "What is the user's _intent_?" | Host, Dev, Image         | `--context dev` or auto-detected |

**The problem**: Currently, `resolve_context()` conflates these. If you're in a
toolbox, it assumes you _want_ Dev context. But if you run `bkt flatpak add`,
you clearly want Host context—you just happen to be _in_ a toolbox.

### CommandTarget: Where Does a Command _Want_ to Run?

Each command has a natural target:

| Command             | Target     | Notes                                |
| ------------------- | ---------- | ------------------------------------ |
| `bkt flatpak *`     | **Host**   | Flatpaks are installed on host       |
| `bkt extension *`   | **Host**   | GNOME extensions are host-level      |
| `bkt gsetting *`    | **Host**   | GSettings are host-level             |
| `bkt shim *`        | **Host**   | Shims enable host→toolbox bridging   |
| `bkt capture`       | **Host**   | Reads host state                     |
| `bkt apply`         | **Host**   | Applies to host                      |
| `bkt status`        | **Host**   | Reads system manifests in /usr/share |
| `bkt doctor`        | **Host**   | Validates host toolchains and paths  |
| `bkt profile`       | **Host**   | Reads system manifests, calls rpm    |
| `bkt base`          | **Host**   | Requires host rpm/rpm-ostree         |
| `bkt system *`      | **Host**   | System/image-level operations        |
| `bkt distrobox *`   | **Host**   | Distrobox config is host-level       |
| `bkt appimage *`    | **Host**   | AppImages are host-level (GearLever) |
| `bkt fetchbin *`    | **Host**   | Host binaries                        |
| `bkt homebrew *`    | **Host**   | Linuxbrew is host-level              |
| `bkt dev *`         | **Dev**    | Toolbox operations                   |
| `bkt admin bootc`   | **Host**   | Already delegates correctly          |
| `bkt drift`         | **Either** | Only reads local state file          |
| `bkt schema`        | **Either** | Pure utility                         |
| `bkt completions`   | **Either** | Pure utility                         |
| `bkt repo`          | **Either** | Works on repo files                  |
| `bkt upstream`      | **Either** | Works on repo files                  |
| `bkt changelog`     | **Either** | Works on repo files                  |
| `bkt skel`          | **Either** | Works on user files                  |
| `bkt buildinfo`     | **Either** | Read-only info                       |
| `bkt containerfile` | **Either** | Ephemeral manifest                   |
| `bkt local`         | **Either** | Manifest editing                     |

## Design

### 1. Add `CommandTarget` Enum

```rust
/// Where a command naturally wants to execute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandTarget {
    /// Must run on the host (flatpak, extension, gsetting, shim, capture, apply)
    Host,
    /// Must run in the dev toolbox (bkt dev dnf)
    Dev,
    /// Can run either place, meaning depends on context
    Either,
}
```

### 2. Map Commands to Targets

Add to `main.rs` or a new `delegation.rs`:

```rust
impl Commands {
    /// Get the natural target for this command.
    pub fn target(&self) -> CommandTarget {
        match self {
            // Host-only commands (read system manifests or require host daemons)
            Commands::Flatpak(_) => CommandTarget::Host,
            Commands::Extension(_) => CommandTarget::Host,
            Commands::Gsetting(_) => CommandTarget::Host,
            Commands::Shim(_) => CommandTarget::Host,
            Commands::Capture(_) => CommandTarget::Host,
            Commands::Apply(_) => CommandTarget::Host,
            Commands::Status(_) => CommandTarget::Host,    // Reads /usr/share/bootc-bootstrap/
            Commands::Doctor(_) => CommandTarget::Host,    // Validates host toolchains
            Commands::Profile(_) => CommandTarget::Host,   // Reads system manifests, calls rpm
            Commands::Base(_) => CommandTarget::Host,      // Requires host rpm/rpm-ostree
            Commands::System(_) => CommandTarget::Host,    // System/image operations
            Commands::Distrobox(_) => CommandTarget::Host, // Distrobox config is host-level
            Commands::AppImage(_) => CommandTarget::Host,  // AppImages are host-level
            Commands::Fetchbin(_) => CommandTarget::Host,  // Host binaries
            Commands::Homebrew(_) => CommandTarget::Host,  // Linuxbrew is host-level
            Commands::Admin(_) => CommandTarget::Host,     // Already handles delegation internally

            // Dev-only commands (toolbox operations)
            Commands::Dev(_) => CommandTarget::Dev,

            // Either: pure utilities or work on repo/user files only
            Commands::Drift(_) => CommandTarget::Either,   // Only reads local state file
            Commands::Repo(_) => CommandTarget::Either,
            Commands::Schema(_) => CommandTarget::Either,
            Commands::Completions(_) => CommandTarget::Either,
            Commands::Upstream(_) => CommandTarget::Either,
            Commands::Changelog(_) => CommandTarget::Either,
            Commands::Skel(_) => CommandTarget::Either,
            Commands::BuildInfo(_) => CommandTarget::Either,
            Commands::Containerfile(_) => CommandTarget::Either,
            Commands::Local(_) => CommandTarget::Either,
        }
    }
}
```

### 3. Early Delegation Check

Add delegation check at the top of `main()`, _before_ matching on commands but
_after_ parsing (so we know the command target):

```rust
fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();

    // Check if we need to delegate to a different context
    maybe_delegate(&cli)?;

    // If we reach here, we're in the right place
    let plan = pipeline::ExecutionPlan::from_cli(&cli);
    // ... rest of main
}

/// Delegate to the appropriate context if needed.
fn maybe_delegate(cli: &Cli) -> Result<()> {
    // Skip if already delegated (prevent infinite recursion)
    if std::env::var("BKT_DELEGATED").is_ok() {
        return Ok(());
    }

    let runtime = context::detect_environment();
    let target = cli.command.target();

    match (runtime, target) {
        // In toolbox, command wants host → delegate to host
        (RuntimeEnvironment::Toolbox, CommandTarget::Host) => {
            delegate_to_host()?;
        }

        // On host, command wants dev → delegate to toolbox
        (RuntimeEnvironment::Host, CommandTarget::Dev) => {
            delegate_to_toolbox()?;
        }

        // Generic container, command wants host → error (no delegation path)
        (RuntimeEnvironment::Container, CommandTarget::Host) => {
            bail!(
                "Cannot run host commands from a generic container\n\n\
                 This command requires the host system, but you're in a container\n\
                 without distrobox-host-exec access.\n\n\
                 Options:\n  \
                 • Exit this container and run on the host\n  \
                 • Use a distrobox instead: distrobox create && distrobox enter"
            );
        }

        // All other cases: run locally
        _ => Ok(()),
    }
}
```

### 4. Delegation Functions

```rust
/// Delegate the current command to the host via flatpak-spawn.
///
/// We use `flatpak-spawn --host` directly instead of `distrobox-host-exec` because:
/// 1. It's the underlying mechanism distrobox uses anyway (via host-spawn → D-Bus)
/// 2. It supports `--env=VAR=VALUE` to pass environment variables to the host
/// 3. distrobox-host-exec doesn't forward env vars set via Command::env()
///
/// The `BKT_DELEGATED=1` env var prevents recursion when bkt is exported as a
/// distrobox shim (see RFC-0017 for why bkt must be excluded from exports).
fn delegate_to_host() -> Result<()> {
    Output::info("Delegating to host...");

    let args: Vec<String> = std::env::args().collect();
    let status = std::process::Command::new("flatpak-spawn")
        .arg("--host")
        .arg("--env=BKT_DELEGATED=1")
        .arg("bkt")
        .args(&args[1..])  // Skip argv[0] (the current binary path)
        .status()
        .context("Failed to execute flatpak-spawn --host")?;

    // Exit with the same code as the delegated command
    std::process::exit(status.code().unwrap_or(1));
}

/// Delegate the current command to the default toolbox.
fn delegate_to_toolbox() -> Result<()> {
    Output::info("Delegating to toolbox...");

    let args: Vec<String> = std::env::args().collect();
    let status = std::process::Command::new("toolbox")
        .arg("run")
        .arg("bkt")
        .args(&args[1..])
        .env("BKT_DELEGATED", "1")
        .status()
        .context("Failed to execute toolbox run")?;

    std::process::exit(status.code().unwrap_or(1));
}
```

### 5. The `bkt dev` Prefix: Semantic Override

The `bkt dev` prefix is a **semantic signal** that the user wants Dev context,
regardless of where they're running:

```bash
# On host:
$ bkt dev dnf install gcc
# → Delegates to toolbox (user explicitly wants dev)

# In toolbox:
$ bkt dev dnf install gcc
# → Runs directly (already in the right place)
```

This is already how it works conceptually, but the implementation should use
the same delegation infrastructure:

```rust
// In Commands::target()
Commands::Dev(_) => CommandTarget::Dev,
```

Combined with the delegation logic, this means:

- `bkt dev dnf install gcc` from host → `toolbox run bkt dev dnf install gcc`
- `bkt dev dnf install gcc` from toolbox → runs directly

### 6. Handling `--context` Overrides

If the user explicitly specifies `--context`, that should override the natural
target. This allows advanced use cases like:

```bash
# Force host context even though we're running bkt dnf which is "Either"
$ bkt --context host dnf install vim
```

Update `maybe_delegate`:

```rust
fn maybe_delegate(cli: &Cli) -> Result<()> {
    if std::env::var("BKT_DELEGATED").is_ok() {
        return Ok(());
    }

    let runtime = context::detect_environment();

    // Explicit --context overrides natural target
    let target = match cli.context {
        Some(ExecutionContext::Host) => CommandTarget::Host,
        Some(ExecutionContext::Dev) => CommandTarget::Dev,
        Some(ExecutionContext::Image) => return Ok(()),  // Image = no execution
        None => cli.command.target(),
    };

    // ... rest of delegation logic
}
```

### 7. Dry-Run Support

For `--dry-run`, we should show what delegation _would_ happen:

```rust
fn maybe_delegate(cli: &Cli) -> Result<()> {
    // ... earlier checks ...

    match (runtime, target) {
        (RuntimeEnvironment::Toolbox, CommandTarget::Host) => {
            if cli.dry_run {
                Output::dry_run("Would delegate to host: flatpak-spawn --host bkt ...");
                // Continue to show the rest of the dry-run output
                return Ok(());
            }
            delegate_to_host()?;
        }
        // ...
    }
}
```

### 8. CLI Flag: `--no-delegate`

For debugging and advanced use, allow suppressing delegation:

```rust
#[derive(Debug, Parser)]
pub struct Cli {
    // ... existing fields ...

    /// Don't auto-delegate to host/toolbox (for debugging)
    #[arg(long, global = true, hide = true)]
    pub no_delegate: bool,
}
```

```rust
fn maybe_delegate(cli: &Cli) -> Result<()> {
    if cli.no_delegate {
        return Ok(());
    }
    // ...
}
```

## Compatibility with Existing Commands

### `bkt admin bootc` (Already Correct)

This command already implements delegation internally via `exec_bootc()`. With
the new infrastructure:

- **Option A**: Keep internal delegation (no change)
- **Option B**: Remove internal delegation, rely on global `maybe_delegate()`

**Recommendation**: Option A for now. The internal delegation in `bootc.rs` also
wraps commands with `pkexec`, which the global delegation doesn't handle.

### Commands That Use `validate_domain()`

Currently, commands like `flatpak.rs` call `plan.validate_domain()` which
rejects invalid contexts. With the new infrastructure:

1. Global delegation runs first → we're in the right place
2. Context is correctly set to Host (not Dev)
3. `validate_domain()` passes

**No changes needed** to individual commands—they just work.

## Validation of Context Detection Changes

Currently, `resolve_context()` has this problematic logic:

```rust
pub fn resolve_context(explicit: Option<ExecutionContext>) -> ExecutionContext {
    match explicit {
        Some(ctx) => ctx,
        None => {
            // Auto-detect: if in toolbox, default to Dev; otherwise Host
            match detect_environment() {
                RuntimeEnvironment::Toolbox => ExecutionContext::Dev,  // ← PROBLEM
                _ => ExecutionContext::Host,
            }
        }
    }
}
```

With delegation in place, this logic should change:

```rust
pub fn resolve_context(explicit: Option<ExecutionContext>) -> ExecutionContext {
    match explicit {
        Some(ctx) => ctx,
        // After delegation, we're always in the "right" place
        // So Host means we're on host, Dev means we're in toolbox
        None => match detect_environment() {
            RuntimeEnvironment::Toolbox => ExecutionContext::Dev,
            RuntimeEnvironment::Host | RuntimeEnvironment::Container => ExecutionContext::Host,
        }
    }
}
```

Actually, this is still correct! The key insight:

- _Before_ delegation: We check `Commands::target()` to decide where to go
- _After_ delegation: `resolve_context()` correctly reflects where we are

The issue was that we were using `resolve_context()` to determine _intent_, but
now we use `Commands::target()` for that.

## Implementation Plan

### Phase 1: Minimal Viable Delegation (1-2 days)

1. Add `CommandTarget` enum to [context.rs](../../bkt/src/context.rs)
2. Add `Commands::target()` method in [main.rs](../../bkt/src/main.rs)
3. Add `delegate_to_host()` and `maybe_delegate()` functions
4. Test: `bkt flatpak list` from toolbox should work

### Phase 2: Full Integration (1-2 days)

1. Add `delegate_to_toolbox()` for `bkt dev` commands from host
2. Add `--no-delegate` hidden flag
3. Add dry-run delegation output
4. Update WORKFLOW.md to remove manual workarounds

### Phase 3: Testing & Polish (1 day)

1. Add integration tests for delegation scenarios
2. Verify all command targets are correct
3. Update RFC-0009 to reference this design
4. Remove redundant error messages about "exit toolbox first"

## Open Questions

### Q1: Should delegation be silent or verbose?

**Current proposal**: Show a brief message:

```
→ Delegating to host via flatpak-spawn...
```

**Alternative**: Silent by default, verbose with `-v`:

```
# No message by default
$ bkt flatpak add org.gnome.Boxes
✓ Added org.gnome.Boxes to manifest
```

### Q2: How to handle stdin/stdout for interactive prompts?

Some commands (like `bkt admin bootc upgrade`) prompt for confirmation. This
should work through `flatpak-spawn --host` because it inherits the terminal.

**Need to verify**: Does `flatpak-spawn --host` properly pass through TTY?

### Q3: Should `BKT_DELEGATED` be visible to users?

Currently proposed as an internal recursion guard. Could also be used for:

- Debugging ("am I running delegated?")
- Scripting (force local execution)

**Recommendation**: Keep internal, use `--no-delegate` for explicit control.

## Summary

This RFC introduces:

1. **CommandTarget enum**: Host, Dev, Either—where a command _wants_ to run
2. **Early delegation in main()**: Transparently re-exec via `flatpak-spawn`
3. **BKT_DELEGATED env var**: Prevents infinite recursion
4. **--no-delegate flag**: Escape hatch for debugging

The result: all `bkt` commands work identically from host or toolbox, without
users needing to think about `flatpak-spawn`.

---

## Performance Optimization: Host-Command Daemon

> **Status**: Phase 3 Complete (robustness improvements)
>
> This section was consolidated from RFC-0050 (Persistent Host-Command Helper).

### The Problem

The `flatpak-spawn --host` delegation path has ~120ms overhead per invocation:

```
bkt (container)
  → flatpak-spawn --host
    → D-Bus session bus
      → org.freedesktop.Flatpak.Development.HostCommand
        → bkt (host)
```

This causes process explosion and OOM conditions during test runs (see
[RFC-0046](../complete/0046-test-performance.md) for the investigation).

### The Solution: Unix Socket Daemon

A persistent daemon on the host accepts commands via Unix socket, bypassing
D-Bus entirely:

```
┌─────────────────────────────────────────────────────────────────────────┐
│                              HOST                                        │
│  ┌─────────────────────────────────────────────────────────────────┐    │
│  │  bkt admin daemon run                                            │    │
│  │  - Listens on $XDG_RUNTIME_DIR/bkt/host.sock                     │    │
│  │  - Accepts connections, forks, execs requested command           │    │
│  │  - Returns exit status                                           │    │
│  └─────────────────────────────────────────────────────────────────┘    │
│                              ↑                                           │
│                              │ Unix socket (bind-mounted into container) │
│                              ↓                                           │
│  ┌─────────────────────────────────────────────────────────────────┐    │
│  │  DISTROBOX (bootc-dev)                                           │    │
│  │  - bkt detects daemon socket exists                              │    │
│  │  - Sends command request via socket                              │    │
│  │  - Waits for exit status                                         │    │
│  └─────────────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────────────┘
```

**Measured overhead**: ~4ms (vs ~120ms for D-Bus path).

### Implementation Status

| Phase                              | Status      | Notes                                              |
| ---------------------------------- | ----------- | -------------------------------------------------- |
| Phase 1: Protocol & PoC            | ✅ Complete | Daemon works, ~4ms overhead                        |
| Phase 2: Integration               | ✅ Complete | delegate_to_host() uses daemon                     |
| Phase 3: Robustness                | ✅ Complete | Timeouts, stale socket detection, logging          |
| Phase 4: Validation & Benchmarking | ✅ Complete | Benchmarks, stress tests, keep-alive investigation |
| Phase 5: systemd                   | Planned     | Auto-start on login                                |

### Module Structure

```
bkt/src/daemon/
├── mod.rs          # Public API: socket_path(), daemon_available()
├── protocol.rs     # Wire format (header, SCM_RIGHTS for fd passing)
├── server.rs       # Host-side daemon (fork_exec)
└── client.rs       # Container-side client
```

### Commands

```bash
bkt admin daemon run     # Run daemon in foreground
bkt admin daemon status  # Check if daemon is available
bkt admin daemon test    # Test with `echo hello`
```

### Phase 2: Integration Plan

When Phase 2 is implemented, `delegate_to_host()` will try the daemon first:

```rust
fn delegate_to_host() -> Result<()> {
    // Try fast path: daemon socket
    if daemon::daemon_available() {
        Output::info("Delegating to host via daemon...");
        return delegate_via_daemon();
    }

    // Fall back to slow path: flatpak-spawn
    Output::info("Delegating to host via flatpak-spawn...");
    delegate_via_flatpak_spawn()
}
```

Shims generated by `bkt shim` will also use the daemon when available:

```bash
#!/bin/bash
if [[ -S "${XDG_RUNTIME_DIR}/bkt/host.sock" ]]; then
    exec bkt admin daemon exec -- {command} "$@"
else
    exec flatpak-spawn --host {command} "$@"
fi
```

### Why Host-Side Daemon?

1. **Socket visibility**: `$XDG_RUNTIME_DIR` is bind-mounted into distrobox
2. **No double-crossing**: Container connects directly to host daemon
3. **Same security model**: Daemon runs as same user, no privilege escalation
4. **Matches existing pattern**: Similar to D-Bus session bus architecture

---

## Appendix: Why Not a Command Trait?

During the design of this RFC, we analyzed all 18 command modules to determine
whether a unified `Command` trait could standardize implementation. The analysis
revealed that such a trait would not be beneficial.

### Command Categories

| Category                 | Commands                                          | Pattern                                     |
| ------------------------ | ------------------------------------------------- | ------------------------------------------- |
| **Plan-based (full)**    | `apply`, `capture`                                | `Plannable` trait, `ExecutionPlan`          |
| **Plan-based (partial)** | `flatpak`, `extension`, `gsetting`, `dnf`, `shim` | Some actions use `Plannable`, others direct |
| **Direct + Delegation**  | `admin`, `dev`                                    | Delegates to subcommands/external tools     |
| **Read-only Inspection** | `status`, `doctor`, `drift`, `profile`            | No `ExecutionPlan`, read-only               |
| **Utility/Generation**   | `completions`, `repo`, `schema`                   | Minimal, stateless                          |
| **Manifest CRUD**        | `upstream`, `changelog`, `base`, `skel`           | File/manifest operations, no `Plannable`    |

### Key Findings

1. **Inconsistent `ExecutionPlan` usage**:
   - 8 commands receive `&ExecutionPlan` but 4 mark it `_plan` (unused)
   - 10 commands don't take `ExecutionPlan` at all

2. **`Plannable` trait is underutilized**:
   - Only `apply` and `capture` fully embrace it
   - Domain commands use it for sync/capture but not add/remove/list

3. **Domain validation is inconsistent**:
   - Some commands call `validate_domain()`, others don't
   - This is intentional: read-only commands don't need validation

4. **Two fundamental command types**:
   - **Mutating commands**: Need context, validation, dry-run, PR workflow
   - **Inspection commands**: Read-only, no side effects, simpler signature

### Why a Unified Trait Doesn't Work

1. **Too Much Variation**: A unified trait would either be too generic
   (`fn run(args) -> Result<()>`) or too specific, forcing boilerplate.

2. **Existing `Plannable` Is Correct**: The Plan-Execute pattern already
   captures the right abstraction for mutating commands.

3. **Delegation Is Orthogonal**: `CommandTarget` routing belongs in `main()`,
   not in individual commands.

4. **Read-only Commands Are Different**: Commands like `status` don't need
   `ExecutionPlan`, validation, or dry-run support.

### Recommended Approach

Instead of a unified trait:

1. **Expand `Plannable` adoption**: Migrate add/remove actions to use Plan types
2. **Keep `CommandTarget` in main()**: Routing is a dispatch concern
3. **Validation as opt-in helper**: Commands call `validate_domain()` when needed
4. **Allow signature variation**: Read-only commands can omit `ExecutionPlan`

This keeps the codebase flexible while still providing consistency where it
matters (the Plan-Execute pattern for mutating operations).
