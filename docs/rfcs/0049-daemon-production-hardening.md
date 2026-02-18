# RFC 0049: Daemon Production Hardening

- **Status**: Draft
- **Created**: 2026-02-17
- **Depends on**: RFC-0048 (Persistent Host-Command Helper)

## Summary

Harden the daemon PoC from RFC-0048 for production use as the **sole mechanism** for distrobox exports. This replaces both `distrobox-host-exec` for bkt delegation and `flatpak-spawn --host` for shims.

---

## 1. Current State

### What Exists (RFC-0048 Phase 1 Complete)

| Component                 | Status     | Location                                                           |
| ------------------------- | ---------- | ------------------------------------------------------------------ |
| Protocol with SCM_RIGHTS  | ✅ Working | [daemon/protocol.rs](../../bkt/src/daemon/protocol.rs)             |
| Server with fork_exec     | ✅ Working | [daemon/server.rs](../../bkt/src/daemon/server.rs)                 |
| Client for requests       | ✅ Working | [daemon/client.rs](../../bkt/src/daemon/client.rs)                 |
| `bkt admin daemon run`    | ✅ Working | [commands/admin/daemon.rs](../../bkt/src/commands/admin/daemon.rs) |
| `bkt admin daemon status` | ✅ Working | Shows socket path and availability                                 |
| `bkt admin daemon test`   | ✅ Working | Runs `echo hello` through daemon                                   |

### Performance

- **Daemon overhead**: ~4ms (server-side fork_exec)
- **Total latency**: ~100ms (dominated by bkt startup, not daemon)
- **Baseline comparison**: `distrobox-host-exec` is ~10-20ms warm, ~120ms cold

### What's Missing

1. **Integration**: `delegate_to_host()` still uses `distrobox-host-exec`
2. **Shims**: Still use `flatpak-spawn --host`
3. **Lifecycle**: No auto-start, no systemd integration
4. **Arbitrary commands**: Only `bkt admin daemon test` works; no general `exec`

---

## 2. Goal

After this RFC:

```bash
# From distrobox, ALL of these use the daemon:
bkt status              # delegate_to_host() → daemon
bootc                   # shim → daemon
systemctl --user        # shim → daemon
flatpak                 # shim → daemon
```

The daemon starts automatically on login and serves all host command requests.

---

## 3. Implementation Plan

### Phase 1: `daemon exec` Command

Add a general-purpose command execution interface:

```bash
# Execute arbitrary command through daemon
bkt admin daemon exec -- ls -la /
bkt admin daemon exec -- bootc status
```

This is the primitive that shims will call.

**Implementation**:

```rust
// In commands/admin/daemon.rs
DaemonAction::Exec { command } => {
    let client = DaemonClient::connect()?;
    let status = client.execute(&command, &env::vars().collect(), &env::current_dir()?)?;
    std::process::exit(status);
}
```

### Phase 2: Integrate `delegate_to_host()`

Modify [main.rs](../../bkt/src/main.rs) to try daemon first:

```rust
fn delegate_to_host() -> Result<()> {
    // Try fast path: daemon socket
    if daemon::daemon_available() {
        output::Output::info("Delegating to host via daemon...");
        return delegate_via_daemon();
    }

    // Fall back to slow path: distrobox-host-exec
    output::Output::info("Delegating to host via distrobox-host-exec...");
    delegate_via_host_exec()
}

fn delegate_via_daemon() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let client = daemon::DaemonClient::connect()?;

    // Execute "bkt" with remaining args
    let mut cmd = vec!["bkt".to_string()];
    cmd.extend(args[1..].iter().cloned());

    let status = client.execute_inherit_stdio(&cmd)?;
    std::process::exit(status);
}
```

### Phase 3: Daemon-Aware Shims

Update [shim.rs](../../bkt/src/commands/shim.rs) `generate_shim_script()`:

**Option A: Shell fallback** (simpler, more robust)

```bash
#!/bin/bash
# Auto-generated shim - delegates to host command
# Managed by: bkt shim
# Host command: {host_cmd}

if [[ -S "${XDG_RUNTIME_DIR}/bkt/host.sock" ]]; then
    exec bkt admin daemon exec -- {quoted} "$@"
else
    exec flatpak-spawn --host {quoted} "$@"
fi
```

**Option B: Daemon-only** (faster, requires daemon running)

```bash
#!/bin/bash
exec bkt admin daemon exec -- {quoted} "$@"
```

**Recommendation**: Option A for robustness during transition, with a flag to generate Option B once stable.

### Phase 4: Systemd User Service

Create service files for auto-start:

```ini
# ~/.config/systemd/user/bkt-hostd.service
[Unit]
Description=BKT Host Command Daemon
Documentation=man:bkt(1)

[Service]
Type=simple
ExecStart=%h/.local/bin/bkt admin daemon run
Restart=on-failure
RestartSec=1

[Install]
WantedBy=default.target
```

Add management commands:

```bash
bkt admin daemon install   # Install and enable systemd service
bkt admin daemon uninstall # Disable and remove systemd service
```

### Phase 5: Robustness

1. **Stale socket detection**: Check if socket is connectable, not just exists
2. **Graceful shutdown**: Handle SIGTERM, clean up socket
3. **Connection timeout**: Don't hang forever on unresponsive daemon
4. **Logging**: Structured logs for debugging

---

## 4. Migration Path

### Step 1: Deploy daemon infrastructure

- Merge Phase 1-2 (exec command, delegate_to_host integration)
- Users can manually start daemon: `bkt admin daemon run &`

### Step 2: Update shims

- Run `bkt shim sync` to regenerate with daemon support
- Shims fall back to flatpak-spawn if daemon unavailable

### Step 3: Enable auto-start

- Run `bkt admin daemon install`
- Daemon starts on login, serves all requests

### Step 4: Remove fallback (optional)

- Once stable, regenerate shims without fallback
- Remove distrobox-host-exec dependency

---

## 5. Testing Strategy

### Unit Tests

- Protocol serialization/deserialization
- Socket path resolution
- Shim script generation

### Integration Tests

- Daemon start/stop lifecycle
- Command execution through daemon
- Fallback behavior when daemon unavailable

### Manual Testing

```bash
# Terminal 1 (host)
bkt admin daemon run

# Terminal 2 (distrobox)
bkt admin daemon status          # Should show "available"
bkt admin daemon exec -- whoami  # Should return host username
bkt status                       # Should use daemon (check logs)
bootc status                     # Shim should use daemon
```

---

## 6. Success Criteria

1. **Functional**: All host commands work through daemon
2. **Performance**: No regression from current daemon PoC (~4ms overhead)
3. **Reliability**: Graceful fallback when daemon unavailable
4. **Operability**: Auto-start on login, survives logout/login cycles
5. **Observability**: Clear logging of daemon vs fallback path

---

## 7. Open Questions

### Resolved

1. **Shim fallback strategy?**
   - **Decision**: Use shell fallback (Option A) initially for robustness

2. **Socket activation vs explicit start?**
   - **Decision**: Explicit start via systemd service (simpler, more predictable)

### Open

1. **Should `daemon exec` require `--` separator?**
   - Pro: Unambiguous argument parsing
   - Con: More typing for simple commands
   - Leaning: Yes, require `--` for safety

2. **Log verbosity in daemon?**
   - Default: Errors only
   - With flag: All requests (for debugging)

---

## 8. References

- [RFC-0048: Persistent Host-Command Helper](0048-persistent-host-command-helper.md) - Daemon design
- [RFC-0046: Test Performance Investigation](0046-test-performance.md) - Problem analysis
- [bkt/src/daemon/](../../bkt/src/daemon/) - Current implementation
- [bkt/src/commands/shim.rs](../../bkt/src/commands/shim.rs) - Shim generation
