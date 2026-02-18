# RFC 0048: Persistent Host-Command Helper

- **Status**: Phase 1 Complete
- **Created**: 2026-02-17
- **Depends on**: RFC-0010 (Transparent Delegation), RFC-0046 (Test Performance)
- **Next**: [RFC-0049](0049-daemon-production-hardening.md) (Production Hardening)

## Summary

Spec out a persistent Unix socket-based daemon to replace the per-invocation D-Bus/host-spawn overhead when executing host commands from within a distrobox container. This addresses the ~120ms overhead per command that causes process explosion and OOM conditions during test runs.

### Implementation Status

| Phase                   | Status      | Notes                                               |
| ----------------------- | ----------- | --------------------------------------------------- |
| Phase 1: Protocol & PoC | ✅ Complete | Daemon works, ~4ms overhead                         |
| Phase 2: Integration    | 🔜 Next     | See [RFC-0049](0049-daemon-production-hardening.md) |
| Phase 3: Robustness     | Planned     |                                                     |
| Phase 4: Optimization   | Planned     |                                                     |
| Phase 5: systemd        | Planned     |                                                     |

---

## 1. Current State Summary

### How Delegation Works Today

The delegation system is implemented in [bkt/src/main.rs](../../bkt/src/main.rs) with supporting types in [bkt/src/context.rs](../../bkt/src/context.rs) and [bkt/src/cli.rs](../../bkt/src/cli.rs).

#### Key Components

| Component               | Location                                                   | Purpose                                             |
| ----------------------- | ---------------------------------------------------------- | --------------------------------------------------- |
| `maybe_delegate()`      | [main.rs#L13-L66](../../bkt/src/main.rs#L13-L66)           | Entry point; decides if delegation is needed        |
| `delegate_to_host()`    | [main.rs#L68-L84](../../bkt/src/main.rs#L68-L84)           | Executes `distrobox-host-exec bkt ...`              |
| `delegate_to_toolbox()` | [main.rs#L86-L101](../../bkt/src/main.rs#L86-L101)         | Executes `distrobox enter bootc-dev -- bkt ...`     |
| `CommandTarget`         | [context.rs#L452-L460](../../bkt/src/context.rs#L452-L460) | Enum: `Host`, `Dev`, `Either`                       |
| `Commands::target()`    | [cli.rs#L172-L211](../../bkt/src/cli.rs#L172-L211)         | Maps each command to its natural target             |
| `RuntimeEnvironment`    | [context.rs#L546-L556](../../bkt/src/context.rs#L546-L556) | Enum: `Host`, `Toolbox`, `Container`                |
| `detect_environment()`  | [context.rs#L558-L595](../../bkt/src/context.rs#L558-L595) | Detects current runtime via `/run/.toolboxenv` etc. |

#### Delegation Flow

```
┌─────────────────────────────────────────────────────────────────────────┐
│                           main() entry                                   │
├─────────────────────────────────────────────────────────────────────────┤
│  1. Parse CLI args (Cli::parse())                                       │
│  2. Call maybe_delegate(&cli)                                           │
│     ├─ Skip if BKT_DELEGATED=1 (recursion guard)                        │
│     ├─ Skip if --no-delegate flag                                       │
│     ├─ Detect RuntimeEnvironment (Host/Toolbox/Container)               │
│     ├─ Get CommandTarget from cli.command.target()                      │
│     └─ Match (runtime, target):                                         │
│        ├─ (Toolbox, Host) → delegate_to_host() → exit                   │
│        ├─ (Host, Dev) → delegate_to_toolbox() → exit                    │
│        ├─ (Container, Host) → error (no delegation path)                │
│        └─ _ → continue locally                                          │
│  3. Create ExecutionPlan                                                │
│  4. Dispatch to command handler                                         │
└─────────────────────────────────────────────────────────────────────────┘
```

#### The Primitive Stack (Current)

When `delegate_to_host()` runs inside a distrobox:

```
bkt (container)
  → distrobox-host-exec (shell script)
    → host-spawn (Go binary)
      → D-Bus session bus
        → org.freedesktop.Flatpak.Development.HostCommand
          → flatpak-spawn --host
            → bkt (host)
```

**Measured overhead**: ~120-142ms per invocation (see [RFC-0046](0046-test-performance.md#measured-overhead)).

#### Command Target Classification

From [cli.rs](../../bkt/src/cli.rs#L172-L211):

| Target     | Commands                                                                                                                                                                  |
| ---------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Host**   | `flatpak`, `extension`, `gsetting`, `shim`, `capture`, `apply`, `status`, `doctor`, `profile`, `base`, `system`, `distrobox`, `appimage`, `fetchbin`, `homebrew`, `admin` |
| **Dev**    | `dev`                                                                                                                                                                     |
| **Either** | `drift`, `repo`, `schema`, `completions`, `upstream`, `changelog`, `skel`, `build-info`, `containerfile`, `local`, `wrap`                                                 |

### Related Infrastructure

#### CommandRunner Abstraction

[bkt/src/command_runner.rs](../../bkt/src/command_runner.rs) provides a `CommandRunner` trait for external command execution:

```rust
pub trait CommandRunner: Send + Sync {
    fn run_output(&self, program: &str, args: &[&str], options: &CommandOptions) -> Result<Output>;
    fn run_status(&self, program: &str, args: &[&str], options: &CommandOptions) -> Result<ExitStatus>;
}
```

This is used by ~83 call sites across 18 files. A daemon-based runner could implement this trait.

#### D-Bus Integration

[bkt/src/dbus/](../../bkt/src/dbus/) already uses `zbus` for systemd operations. The D-Bus infrastructure routes correctly from toolbox to host automatically via flatpak-portal.

#### Shim System

[bkt/src/commands/shim.rs](../../bkt/src/commands/shim.rs) generates wrapper scripts that delegate to host:

```bash
#!/bin/bash
exec flatpak-spawn --host <command> "$@"
```

This is the same primitive stack, just for individual commands rather than bkt itself.

---

## 2. Proposed Architecture

### The horizontal.c Approach

The [horizontal.c](https://git.disroot.org/Sir_Walrus/misctoys/src/branch/master/horizontal.c) proof-of-concept demonstrates a Unix socket-based forwarder:

#### Key Primitives

1. **Unix domain socket** (`AF_UNIX`, `SOCK_STREAM`) for IPC
2. **File descriptor passing** via `SCM_RIGHTS` (stdin/stdout/stderr)
3. **Environment forwarding** (full `envp` serialized in message)
4. **Working directory forwarding** (`getcwd()` sent with request)
5. **Exit code propagation** (server sends `waitpid` status back)

#### Protocol

```
Client → Server:
┌──────────────────────────────────────────────────────────────┐
│ Header: { n_fds, n_argv, n_envp }                            │
│ Body: cwd\0 argv[0]\0 argv[1]\0 ... envp[0]\0 envp[1]\0 ...  │
│ Ancillary: SCM_RIGHTS with fd[0], fd[1], fd[2]               │
└──────────────────────────────────────────────────────────────┘

Server → Client:
┌──────────────────────────────────────────────────────────────┐
│ int: waitpid status (WIFEXITED/WIFSIGNALED encoded)          │
└──────────────────────────────────────────────────────────────┘
```

### Where the Daemon Lives

**Recommendation: Host-side daemon, container-side client.**

```
┌─────────────────────────────────────────────────────────────────────────┐
│                              HOST                                        │
│  ┌─────────────────────────────────────────────────────────────────┐    │
│  │  bkt-hostd (daemon)                                              │    │
│  │  - Listens on $XDG_RUNTIME_DIR/bkt-host.sock                     │    │
│  │  - Accepts connections, forks, execs requested command           │    │
│  │  - Returns exit status                                           │    │
│  └─────────────────────────────────────────────────────────────────┘    │
│                              ↑                                           │
│                              │ Unix socket (bind-mounted)                │
│                              ↓                                           │
│  ┌─────────────────────────────────────────────────────────────────┐    │
│  │  DISTROBOX (bootc-dev)                                           │    │
│  │  ┌─────────────────────────────────────────────────────────────┐ │    │
│  │  │  bkt (client mode)                                          │ │    │
│  │  │  - Detects daemon socket exists                             │ │    │
│  │  │  - Sends command request via socket                         │ │    │
│  │  │  - Waits for exit status                                    │ │    │
│  │  └─────────────────────────────────────────────────────────────┘ │    │
│  └─────────────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────────────┘
```

#### Why Host-Side Daemon?

1. **Socket visibility**: `$XDG_RUNTIME_DIR` is bind-mounted into distrobox, so the socket is accessible from both sides
2. **No double-crossing**: Container connects directly to host daemon, bypassing D-Bus entirely
3. **Simpler security model**: Daemon runs as the same user, no privilege escalation needed
4. **Matches existing pattern**: Similar to how D-Bus session bus works (host daemon, container client)

#### Socket Location

```
$XDG_RUNTIME_DIR/bkt/host.sock
# e.g., /run/user/1000/bkt/host.sock
```

This directory is:

- Automatically created by systemd for the user session
- Bind-mounted into distrobox containers
- Cleaned up on logout

### Daemon Lifecycle

#### Option A: Explicit Management

```bash
# Start daemon (from host)
bkt admin daemon start

# Stop daemon
bkt admin daemon stop

# Check status
bkt admin daemon status
```

#### Option B: Socket Activation (systemd)

```ini
# ~/.config/systemd/user/bkt-hostd.socket
[Socket]
ListenStream=%t/bkt/host.sock

[Install]
WantedBy=sockets.target
```

```ini
# ~/.config/systemd/user/bkt-hostd.service
[Service]
ExecStart=/usr/bin/bkt admin daemon run
```

**Recommendation**: Start with Option A for simplicity, add Option B later.

#### Option C: Auto-Start on First Use

When `delegate_to_host()` detects no daemon socket:

1. Start daemon via `distrobox-host-exec bkt admin daemon start`
2. Wait for socket to appear
3. Connect and proceed

This provides the best UX (transparent) but adds complexity.

---

## 3. Integration Points

### Modified Delegation Flow

```rust
// In main.rs, modify delegate_to_host()

fn delegate_to_host() -> Result<()> {
    // Try fast path first: daemon socket
    let socket_path = daemon_socket_path()?;
    if socket_path.exists() {
        return delegate_via_daemon(&socket_path);
    }

    // Fall back to slow path: distrobox-host-exec
    delegate_via_host_exec()
}

fn delegate_via_daemon(socket: &Path) -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let envp: Vec<(String, String)> = std::env::vars().collect();
    let cwd = std::env::current_dir()?;

    let status = daemon_client::execute(
        socket,
        &args[0],  // bkt binary path on host
        &args[1..],
        &envp,
        &cwd,
    )?;

    std::process::exit(status);
}
```

### New Module Structure

```
bkt/src/
├── daemon/
│   ├── mod.rs          # Public API
│   ├── protocol.rs     # Wire format (header, serialization)
│   ├── server.rs       # Host-side daemon implementation
│   └── client.rs       # Container-side client implementation
├── commands/
│   └── admin/
│       ├── mod.rs      # Add Daemon variant
│       └── daemon.rs   # bkt admin daemon {start,stop,status,run}
```

### CommandRunner Integration

A `DaemonCommandRunner` could implement the `CommandRunner` trait:

```rust
pub struct DaemonCommandRunner {
    socket_path: PathBuf,
}

impl CommandRunner for DaemonCommandRunner {
    fn run_output(&self, program: &str, args: &[&str], options: &CommandOptions) -> Result<Output> {
        // Send request to daemon, capture output
    }

    fn run_status(&self, program: &str, args: &[&str], options: &CommandOptions) -> Result<ExitStatus> {
        // Send request to daemon, inherit stdio
    }
}
```

This would allow individual commands to use the daemon for host operations without full delegation.

---

## 4. Command Interface

### New Commands

```bash
# Daemon management (host-only)
bkt admin daemon start      # Start the host daemon
bkt admin daemon stop       # Stop the host daemon
bkt admin daemon status     # Show daemon status (running, socket path, PID)
bkt admin daemon run        # Run daemon in foreground (for systemd)

# Optional: explicit daemon usage
bkt --use-daemon <command>  # Force daemon path (for testing)
bkt --no-daemon <command>   # Force host-exec path (for debugging)
```

### Environment Variables

```bash
BKT_DAEMON_SOCKET=/path/to/socket  # Override socket location
BKT_NO_DAEMON=1                    # Disable daemon usage
BKT_DAEMON_TIMEOUT=5000            # Connection timeout (ms)
```

### Status Output

```
$ bkt admin daemon status
Daemon Status: running
  Socket: /run/user/1000/bkt/host.sock
  PID: 12345
  Uptime: 2h 34m
  Connections served: 1,247
  Average latency: 3.2ms
```

---

## 5. Implementation Phases

### Phase 1: Protocol & Proof of Concept ✅

**Goal**: Validate the approach works for bkt delegation.

**Status**: Complete (2026-02-17)

**Commits**:

- `737dbea` - Initial daemon PoC
- `91d4e63` - Optimize fork_exec with raw execve

**Implementation**:

| Component             | File                                                               | Status |
| --------------------- | ------------------------------------------------------------------ | ------ |
| Protocol (SCM_RIGHTS) | [daemon/protocol.rs](../../bkt/src/daemon/protocol.rs)             | ✅     |
| Server (fork_exec)    | [daemon/server.rs](../../bkt/src/daemon/server.rs)                 | ✅     |
| Client                | [daemon/client.rs](../../bkt/src/daemon/client.rs)                 | ✅     |
| CLI commands          | [commands/admin/daemon.rs](../../bkt/src/commands/admin/daemon.rs) | ✅     |

**Performance**:

- Daemon overhead: ~4ms (server-side fork_exec)
- Total latency: ~100ms (dominated by bkt startup, not daemon)

**Commands available**:

```bash
bkt admin daemon run     # Run daemon in foreground
bkt admin daemon status  # Check if daemon is available
bkt admin daemon test    # Test with `echo hello`
```

### Phase 2: Integration 🔜

**Goal**: Make daemon usage transparent.

**Status**: Planned — see [RFC-0049](0049-daemon-production-hardening.md) for detailed plan.

1. Add `bkt admin daemon exec -- <cmd>` for arbitrary commands
2. Modify `delegate_to_host()` to try daemon first
3. Update shim generation to use daemon
4. Add fallback to host-exec when daemon unavailable

### Phase 3: Robustness

**Goal**: Production-ready daemon.

1. Connection pooling (reuse connections for multiple commands)
2. Timeout handling
3. Graceful shutdown
4. Logging and metrics
5. Error recovery (daemon crash detection, auto-restart)

### Phase 4: Optimization

**Goal**: Maximize performance gains.

1. Benchmark against host-exec baseline
2. Consider persistent connections (keep-alive)
3. Consider batching (multiple commands per connection)
4. Profile and optimize hot paths

### Phase 5: systemd Integration (Optional)

**Goal**: Zero-configuration daemon lifecycle.

1. Generate socket unit file
2. Generate service unit file
3. Add `bkt admin daemon install` to set up units
4. Test socket activation

---

## 6. Design Decisions

### Scope: Arbitrary Commands (Not Just bkt)

The daemon handles **any command**, not just `bkt`. This is the primary purpose:

- Replaces `distrobox-host-exec` / `flatpak-spawn --host` for all shims
- Shims generated by `bkt shim` will use the daemon socket instead of D-Bus
- Same security model as current (same user, same trust level as shell)

### Double-Crossing: Non-Issue

The "double-crossing" scenario (host → container → host) is not actually a problem:

```
Scenario: cargo test (host) → distrobox-enter → cargo test (container) → bkt flatpak (needs host)

With daemon:
  Container connects directly to $XDG_RUNTIME_DIR/bkt/host.sock
  No "crossing back" - socket is bind-mounted, direct connection
  ~3ms latency
```

The socket is visible from both host and container via the bind mount. There's no nested delegation - the container reaches the host daemon directly.

The only case where host→container delegation matters is `bkt dev` commands, which are rare and can continue using `distrobox enter` (the slow path is acceptable for interactive dev commands).

---

## 7. Open Questions

### Security

1. **Socket permissions?**
   - Default: User-only (0700 on directory, 0600 on socket)
   - Sufficient since distrobox runs as same user
   - **Recommendation**: Match XDG_RUNTIME_DIR permissions

### Operations

2. **Daemon crash recovery?**
   - Client detects dead socket → fall back to host-exec
   - Auto-restart via systemd (Phase 5)
   - **Recommendation**: Graceful fallback in Phase 2

3. **Multiple distrobox containers?**
   - All share same XDG_RUNTIME_DIR
   - Single daemon serves all containers
   - **Recommendation**: Works by default, no special handling

### Testing

4. **How to test daemon in CI?**
   - CI runs on host (no distrobox)
   - Need mock or integration test environment
   - **Recommendation**: Unit tests for protocol, integration tests optional

---

## 8. Alignment with Vision

From [VISION.md](../VISION.md):

### "The Immediate Development Axiom"

> The running system is the source of truth. Changes should take effect immediately wherever possible.

The daemon reduces latency from ~120ms to ~3ms, making cross-boundary operations feel immediate.

### "No Custom Python Scripts"

The daemon is implemented in Rust, consistent with the "all tooling in Rust" principle.

### "The Distrobox Strategy"

> Development tools live in a distrobox container... Host exports selected binaries.

The daemon is the infrastructure that makes this strategy performant. It's the "fast path" for the host↔container bridge.

### Declarative Management

The daemon itself is ephemeral (runtime state), but its configuration could be declarative:

- Socket path derived from XDG_RUNTIME_DIR
- systemd units generated from manifest (Phase 5)

---

## 9. References

- [RFC-0010: Transparent Command Delegation](canon/0010-transparent-delegation.md) - Current delegation design
- [RFC-0046: Test Performance Investigation](0046-test-performance.md) - Problem analysis
- [horizontal.c](https://git.disroot.org/Sir_Walrus/misctoys/src/branch/master/horizontal.c) - Proof of concept
- [bkt/src/main.rs](../../bkt/src/main.rs) - Current delegation implementation
- [bkt/src/command_runner.rs](../../bkt/src/command_runner.rs) - CommandRunner abstraction
