# RFC 0046: Test Performance Investigation

## Status

Investigation

## Problem Statement

Running `cargo test` in the `bkt` crate causes severe system resource
exhaustion, crippling the system for 10-15 minutes. This has been a
recurring issue that previous mitigations (reducing `jobs = 8` in
`.cargo/config.toml`) did not fully resolve.

## Empirical Findings

### OOM Events

Journal analysis shows three OOM kills in the past 24 hours, all
targeting VS Code:

| Time         | Process           | Virtual Memory |
| ------------ | ----------------- | -------------- |
| Feb 16 20:56 | code (PID 11914)  | 1.4TB          |
| Feb 16 23:41 | code (PID 513612) | 1.4TB          |
| Feb 17 14:11 | code (PID 44475)  | 1.4TB          |

The 1.4TB virtual memory is characteristic of V8's memory mapping
behavior, not actual allocation.

### Process Explosion at OOM Time

The kernel OOM dump at 20:56:46 shows dozens of parallel process
chains:

```
[ 437542]  bkt
[ 437543]  distrobox-host-
[ 437564]  host-spawn
[ 437724]  conmon
[ 437727]  bkt
[ 437728]  distrobox-host-
...
```

Each `bkt` CLI test invocation creates a 4-process chain:

1. `bkt` binary (inside distrobox)
2. `conmon` (container monitor)
3. `distrobox-host-exec` (distrobox IPC)
4. `host-spawn` (flatpak-spawn equivalent)

### Baseline Memory State

At time of investigation, VS Code extensions were consuming:

| Process          | RSS   | % of 64GB |
| ---------------- | ----- | --------- |
| tsgo (native TS) | 6.1GB | 9.4%      |
| tsserver.js      | 4.4GB | 6.7%      |
| eslintServer.js  | 3.8GB | 5.8%      |
| rust-analyzer    | 3.0GB | 4.6%      |

**Total: ~17GB just for VS Code language servers.**

### Test Suite Characteristics

- 298 unit tests (in `bkt/src/`)
- 72 CLI integration tests (in `bkt/tests/cli.rs`)
- 11 property tests with proptest (256 cases each = 2,816 iterations)
- 4 test binaries total (lib, main, cli, properties)

The CLI tests use `assert_cmd` to spawn the `bkt` binary 86 times.

### The Multiplication Problem

Default `cargo test` behavior:

- Runs tests in parallel using all available cores (24 on this system)
- Each CLI test spawns `bkt` binary
- Inside distrobox, each `bkt` spawn creates 4 processes

Worst case: 24 threads × 4 processes = **96 concurrent processes**
just from the test harness, before counting the actual work.

## Questions to Consider

### 1. Why is distrobox involved at all?

**Investigated and answered.**

The `cargo` command on the host is a distrobox wrapper script at
`~/.local/bin/distrobox/cargo`:

```sh
if [ -z "${CONTAINER_ID}" ]; then
    exec "/usr/bin/distrobox-enter" -n bootc-dev -- \
        '/home/wycats/.cargo/bin/cargo' "$@"
fi
```

So `cargo test` from VS Code (running on host) enters the `bootc-dev`
distrobox to run tests.

Additionally, `bkt` itself has delegation logic in `main.rs`:

- Commands with `CommandTarget::Host` (flatpak, extension, gsetting,
  shim, capture, apply, status, doctor, profile, etc.) delegate via
  `distrobox-host-exec` when run inside a container.

This means for Host-targeted commands, the chain is:

```
VS Code (host)
  → cargo wrapper (host)
    → distrobox-enter bootc-dev
      → cargo test (container)
        → bkt flatpak list (container)
          → distrobox-host-exec bkt (container→host)
            → bkt flatpak list (host, actual execution)
```

**Two distrobox crossings per CLI test** for Host-targeted commands.

The 72 CLI tests include many Host-targeted commands (flatpak, shim,
extension, gsetting, capture, profile, etc.), each incurring this
double-crossing overhead.

### 2. Is the CLI test design appropriate?

The CLI tests spawn a fresh `bkt` process for each assertion. This is
the `assert_cmd` pattern — good for true integration testing, but
expensive.

Alternatives:

- In-process testing via library API (no subprocess)
- Batch multiple assertions per process spawn
- Mock the CLI layer and test the underlying functions

**Trade-off**: True CLI testing catches real integration bugs (arg
parsing, exit codes, output formatting) that in-process tests miss.

### 3. Is parallel execution the right default?

The `jobs = 8` setting limits _compilation_ parallelism, not _test
execution_ parallelism. These are independent:

- `jobs = N` → max N rustc processes during build
- `RUST_TEST_THREADS = N` → max N test threads during execution

Currently: 8 compile jobs, 24 test threads.

### 4. What's the interaction with VS Code?

VS Code language servers consume 17GB at baseline. The OOM killer
targets VS Code, not cargo/rustc. This suggests:

- The test suite pushes memory over the edge
- VS Code has the highest `oom_score_adj` (300) so gets killed first
- The actual memory hog might be something else entirely

**Question**: Would the tests cause problems on a system without VS
Code running? Or is VS Code the real issue?

### 5. Is there a memory leak?

The process explosion shows many `bkt` processes alive simultaneously.
Are they:

- Legitimately running in parallel (expected)
- Stuck/hanging (bug)
- Leaking and not cleaning up (bug)

### 6. What about the proptest cases?

11 property tests × 256 cases = 2,816 test iterations. These are
pure-Rust tests (no subprocess spawning), but they do run in parallel.

With 24 threads, that's potentially 24 concurrent proptest executions,
each doing JSON serialization/deserialization.

## Implications of the Double-Crossing

The distrobox involvement is **intentional by design**:

1. **Cargo wrapper**: Ensures Rust toolchain runs in the dev container
   where dependencies are installed. This is correct — you don't want
   to maintain a Rust toolchain on the immutable host.

2. **bkt delegation**: Commands like `flatpak list` need to run on the
   host because that's where flatpak state lives. The delegation logic
   is correct — it's the whole point of the host/dev split.

The problem is that **CLI integration tests exercise this delegation
path**, which is expensive. Each test:

1. Spawns a process (cargo test thread)
2. Enters distrobox (distrobox-enter + conmon)
3. Spawns bkt binary
4. bkt delegates to host (distrobox-host-exec + host-spawn)
5. Host bkt runs the actual command
6. Results propagate back through the chain

This is **6+ processes per test**, not 4.

### Why "Just Limit Parallelism" Is Wrong

The naive fix `RUST_TEST_THREADS = 4` would:

1. **Mask the architectural issue** — The double-crossing is the
   problem, not parallelism per se
2. **Slow down all tests** — Unit tests (298 of them) don't cross
   distrobox boundaries; they'd be penalized for CLI tests
3. **Not address the root cause** — Even with 4 threads, each CLI
   test still does 6+ process spawns
4. **Leave VS Code memory unaddressed** — 17GB baseline is a separate
   issue that compounds the problem

### The Real Question

Should CLI integration tests exercise the distrobox delegation path?

Arguments for:

- Tests should match production behavior
- Delegation bugs would be caught
- "It works on my machine" confidence

Arguments against:

- Massive overhead (6+ processes per test)
- Tests become environment-dependent
- Unit tests of delegation logic could suffice
- CI runs on host anyway (no distrobox)

### Alternative Architectures

**Option A: `--no-delegate` for tests**

The `bkt` CLI already has `--no-delegate` flag. CLI tests could use:

```rust
fn bkt() -> Command {
    let mut cmd = cargo_bin_cmd!("bkt");
    cmd.arg("--no-delegate");
    cmd
}
```

This would skip the host delegation, running commands in-container.
Some tests would fail (those that actually need host state), but
those could be marked `#[ignore]` for normal runs.

**Option B: Run tests on host directly**

If `cargo` weren't wrapped, tests would run on host. The `bkt` binary
would detect `RuntimeEnvironment::Host` and not delegate.

This requires either:

- A separate `cargo-host` command
- Running tests outside VS Code terminal
- Unwrapping cargo for test runs

**Option C: Separate test suites**

- `cargo test --lib` — Unit tests only, fast, no subprocess
- `cargo test --test cli` — CLI tests, slow, subprocess-heavy
- `cargo test --test cli -- --ignored` — Full integration with delegation

**Option D: Mock the delegation**

Instead of actually delegating, tests could mock the delegation
decision and test the command logic directly. This loses true
integration coverage but gains speed.

## Investigation TODO

- [x] Determine why `bkt` binary invocations go through distrobox
  - Cargo wrapper + bkt delegation logic = double-crossing
- [ ] Profile memory usage during test execution (not just at OOM)
- [ ] Check if CLI tests can run with `--no-delegate`
- [ ] Measure test duration with different configurations
- [ ] Investigate VS Code memory usage patterns
- [ ] Consider test suite restructuring (unit vs integration split)

## Potential Solutions (Evaluated)

| Solution                 | Pros                             | Cons                                   |
| ------------------------ | -------------------------------- | -------------------------------------- |
| `--no-delegate` in tests | Simple, preserves test structure | Some tests may fail without host state |
| Run tests on host        | Eliminates all overhead          | Requires toolchain on host             |
| Separate test suites     | Granular control                 | More complex CI                        |
| `serial_test` for CLI    | Limits CLI parallelism only      | Still has overhead per test            |
| Reduce proptest cases    | Faster proptest                  | Less coverage                          |
| VS Code memory limits    | Addresses baseline               | Doesn't fix test issue                 |

## Open Questions

1. ~~Is the distrobox involvement intentional or accidental?~~
   **Answered: Intentional (cargo wrapper + bkt delegation)**
2. What's the actual memory profile during test execution?
3. Are there hanging/leaked processes?
4. Which CLI tests actually need host state vs just testing CLI parsing?
5. Would `--no-delegate` break tests or just skip host-dependent ones?
6. Is the 17GB VS Code baseline acceptable or a separate problem?

## Measured Overhead

| Path                                | Time  | Overhead vs Direct |
| ----------------------------------- | ----- | ------------------ |
| Direct (host)                       | 6ms   | baseline           |
| Single crossing (distrobox enter)   | 123ms | 20x                |
| Double crossing (enter + host-exec) | 142ms | 24x                |

The time overhead is significant but not catastrophic. The real issue
is **concurrent process count** under parallel test execution.

Each `distrobox enter` spawns a `conmon` process. With 24 parallel
test threads, each doing double-crossings, the system can have
hundreds of concurrent processes.

## The Distrobox Primitive Stack

```
Container process
  → host-spawn (Go binary, uses D-Bus)
    → org.freedesktop.Flatpak.Development.HostCommand (D-Bus portal)
      → flatpak-spawn --host (on host)
        → actual command (on host)
```

The `host-spawn` → D-Bus → `flatpak-spawn` chain is the fundamental
primitive. This is how Flatpak sandboxes communicate with the host,
and distrobox reuses this mechanism.

### Is the overhead fundamental?

**Partially.** The D-Bus round-trip and process spawning are
fundamental to the security model — the container can't directly
execute host code. But there are potential optimizations:

1. **Batch operations**: Instead of N separate host-spawn calls,
   batch them into a single call that executes multiple commands.

2. **Persistent connection**: Keep a host-side daemon running that
   accepts commands over a socket, avoiding per-call D-Bus overhead.

3. **Direct execution for trusted containers**: Distrobox containers
   share the home directory and have extensive host access. A more
   direct execution path might be possible.

4. **Wrapper optimization**: Our distrobox wrappers (in
   `~/.local/bin/distrobox/`) could be smarter about when to delegate.

### Wrapper Optimization Opportunity

Currently, the cargo wrapper always enters distrobox:

```sh
if [ -z "${CONTAINER_ID}" ]; then
    exec "/usr/bin/distrobox-enter" -n bootc-dev -- \
        '/home/wycats/.cargo/bin/cargo' "$@"
fi
```

But for `cargo test`, we could detect that we're about to run tests
and either:

- Run on host directly (if toolchain is available)
- Pass `--no-delegate` to bkt invocations
- Use a test-specific wrapper that avoids double-crossing

### The Deeper Question

The distrobox strategy assumes:

1. Dev tools (cargo, rustc) live in the container
2. Host operations (flatpak, systemd) need host access
3. Crossing the boundary is acceptable overhead

For interactive use, 142ms per command is fine. For test suites with
hundreds of invocations, it becomes problematic.

**Options:**

1. Accept the overhead, limit parallelism
2. Restructure tests to minimize crossings
3. Optimize the crossing mechanism
4. Maintain host toolchain for testing

## Next Steps

1. **Categorize CLI tests** — Which actually need host state?
2. **Try `--no-delegate`** — See what breaks
3. **Profile a single test** — Measure actual process/memory overhead
4. **Consider test architecture** — Unit tests for logic, integration
   tests for delegation (run separately)
5. **Investigate wrapper optimization** — Can we make smarter
   decisions about when to cross?
