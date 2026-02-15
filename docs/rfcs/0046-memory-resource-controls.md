# RFC 0046: Memory Resource Controls

- Feature Name: `memory_resource_controls`
- Start Date: 2026-02-15
- RFC PR: (leave this empty)
- Tracking Issue: (leave this empty)

## Status

Validated — ready for implementation

## Summary

Protect the system from runaway memory consumption by configuring per-application
`MemoryMax` limits and tightening `systemd-oomd` thresholds. Motivated by a
production OOM crash where VS Code + rust-analyzer consumed 60+ GB, exhausted
16 GB swap, and caused system-wide failure — despite `systemd-oomd` being active.

## Motivation

### The Incident (2026-02-14)

On a 62 GB RAM + 16 GB swap system, two VS Code workspaces with rust-analyzer
drove memory to 60+ GB. The crash timeline:

| Time     | Event                                                                         |
| -------- | ----------------------------------------------------------------------------- |
| 23:11:33 | **Kernel** OOM-kills VS Code process (pid 511189)                             |
| 23:11:33 | VS Code scope `app-gnome-code-510847.scope` fails: **8.9 GB peak, 1 GB swap** |
| 23:12:16 | GNOME Shell relaunches VS Code (new scope)                                    |
| 23:17:56 | **Kernel** OOM-kills second VS Code (pid 1923018)                             |
| 23:17:56 | Cascading: GNOME Shell OOM-killed (3.5 GB peak)                               |
| 23:17:59 | Full user session collapse                                                    |

**systemd-oomd never fired.** The kernel OOM killer handled everything.

### Why oomd Failed

The default oomd configuration is too lenient for workstation use:

| Setting                            | Default | Problem                                             |
| ---------------------------------- | ------- | --------------------------------------------------- |
| Memory pressure threshold          | 80%     | Too high — system was dying first                   |
| Pressure duration                  | 20s     | Too long — 20s of sustained death                   |
| VS Code `MemoryMax`                | ∞       | No cap at all                                       |
| VS Code `ManagedOOMMemoryPressure` | auto    | Inherits parent, no scope-level enforcement         |
| Swap monitoring                    | 90%     | Effectively unreachable — system is dead before 90% |

### Current Baseline (single workspace, 2026-02-15)

```
VS Code scope (cgroup accounting):
  Current: 15.0 GB
  Peak:    15.1 GB

Breakdown:
  code processes:     9.6 GB RSS (8 processes)
  rust-analyzer:      4.5 GB RSS (2 instances)
  tsgo:               0.6 GB RSS (1 instance)
  Total ecosystem:   ~15 GB
```

With two workspaces open (separate GNOME scopes), the combined footprint would
be ~30 GB — leaving only 32 GB for the OS, GNOME Shell, browsers, Steam, Slack,
and everything else.

## Guide-level Explanation

### What Changes

Three layers of defense, baked into the image:

1. **VS Code memory cap** — A systemd user slice (`app-vscode.slice`) with
   `MemoryHigh=20G` and `MemoryMax=24G` that covers all VS Code instances
   (including rust-analyzer, tsgo, and all child processes).
2. **Edge browser cap** — A parallel slice (`app-msedge.slice`) with
   `MemoryHigh=8G` and `MemoryMax=12G`.
3. **Tighter oomd thresholds** — Lower the global memory pressure threshold
   from 60%/30s to 60%/20s, with per-slice overrides at 50% for managed apps.

Additionally, wrapper scripts **replace** the standard `/usr/bin/code` and
`/usr/bin/microsoft-edge-stable` symlinks, ensuring **every** launch path
(desktop, terminal, toolbox, scripts) goes through the managed slice.

### How It Feels

| Scenario                       | Before                | After                                         |
| ------------------------------ | --------------------- | --------------------------------------------- |
| 1 workspace, quiet             | 15 GB, no limit       | 15 GB, well under MemoryHigh                  |
| 1 workspace, heavy RA load     | 20+ GB, no limit      | Pressure at 20 GB, reclaim kicks in           |
| 2 workspaces                   | 30+ GB → system crash | MemoryMax at 24 GB → OOM kills worst offender |
| 2 workspaces + browser + Steam | 60 GB → full crash    | VS Code + Edge capped, oomd intervenes early  |

> **Interaction with existing tuning**: The image already sets `vm.swappiness=10`
> (via `system/etc/sysctl.d/99-bootc-vm-tuning.conf`), which keeps swap as a
> last resort. This means swap fills slowly under pressure, making swap-based
> oomd triggers less responsive. The primary defense is therefore PSI-based
> pressure monitoring, with swap monitoring as a secondary backstop.

The `MemoryHigh` soft limit triggers kernel memory pressure _within the cgroup_,
causing the kernel to aggressively reclaim pages. If there are reclaimable pages
(file cache, inactive mappings), the impact is nearly invisible. Only when all
memory is active anonymous pages (RA index, V8 heap) does it cause real slowdown.
If reclaim isn't enough, `MemoryMax` hard-kills the worst offender.

### Why MemoryHigh = 20 GB

The baseline is 15 GB for a single workspace. The question is headroom:

| Value     | Headroom       | Triggers On                                    | Daily Impact               |
| --------- | -------------- | ---------------------------------------------- | -------------------------- |
| 16 GB     | 1 GB (7%)      | Every `cargo check`, RA re-index, file save    | Constant throttling        |
| **20 GB** | **5 GB (33%)** | **2nd workspace, stacking RA + clippy + tsgo** | **Only during heavy work** |
| 24 GB     | = MemoryMax    | Nothing — straight from "fine" to "killed"     | No soft limit at all       |

20 GB lets normal single-workspace operation breathe, triggers pressure only
when usage is genuinely elevated, and preserves 4 GB of runway between "slow
down" and "kill."

### User Controls

```bash
# Check VS Code memory usage
systemctl --user status app-vscode.slice

# Temporarily raise the limit for a heavy session
systemctl --user set-property app-vscode.slice MemoryMax=32G

# Disable the cap entirely (until next boot)
systemctl --user set-property app-vscode.slice MemoryMax=infinity

# Same for Edge
systemctl --user status app-msedge.slice
```

## Reference-level Explanation

### File Inventory

```
systemd/user/app-vscode.slice                      # VS Code memory limits
systemd/user/app-msedge.slice                       # Edge memory limits
system/etc/systemd/oomd.conf.d/10-bootc-tuning.conf # Tighter oomd thresholds
scripts/code-managed                                # VS Code wrapper
scripts/msedge-managed                              # Edge wrapper
```

### 1. VS Code Slice

`systemd/user/app-vscode.slice`:

```ini
[Slice]
Description=VS Code Memory Budget

# Soft limit: triggers memory pressure reclaim within the cgroup.
# At 20 GB, the kernel starts reclaiming pages aggressively,
# making VS Code slower but keeping the system alive.
# Baseline is ~15 GB (one workspace + RA + tsgo), so this
# gives 5 GB (~33%) headroom for normal spikes.
MemoryHigh=20G

# Hard limit: processes are OOM-killed if they exceed this.
# 24 GB = ~39% of 62 GB, leaving 38 GB for everything else.
MemoryMax=24G

# Tell oomd to monitor this slice for memory pressure.
ManagedOOMMemoryPressure=kill
ManagedOOMMemoryPressureLimit=50%
```

Installed to `/usr/lib/systemd/user/app-vscode.slice`.

### 2. Edge Slice

`systemd/user/app-msedge.slice`:

```ini
[Slice]
Description=Microsoft Edge Memory Budget

MemoryHigh=8G
MemoryMax=12G

ManagedOOMMemoryPressure=kill
ManagedOOMMemoryPressureLimit=50%
```

Installed to `/usr/lib/systemd/user/app-msedge.slice`.

### 3. oomd Tuning

`system/etc/systemd/oomd.conf.d/10-bootc-tuning.conf`:

```ini
[OOM]
# Defaults are 60%/30s. Tighten duration to 20s (Fedora workstation guidance).
# Keep the global pressure limit at 60% to avoid false positives on
# background services. Per-app slices override to 50% via
# ManagedOOMMemoryPressureLimit on their unit files.
DefaultMemoryPressureLimit=60%
DefaultMemoryPressureDurationSec=20s

# Keep swap monitoring at default 90%. With vm.swappiness=10, swap fills
# slowly — lowering this threshold risks premature kills during normal
# swap usage. The primary defense is PSI-based pressure, not swap level.
SwapUsedLimit=90%
```

Installed to `/etc/systemd/oomd.conf.d/10-bootc-tuning.conf`.

**Why split global vs per-slice thresholds**: Background services and GNOME
Shell can experience transient pressure spikes that don't warrant kills.
The global 60%/20s catches genuine system-wide emergencies. The per-slice
50% on VS Code and Edge slices catches app-specific runaway growth faster,
since those are the known offenders.

### 4. VS Code Wrapper

`scripts/code-managed`:

```bash
#!/bin/bash
# Launch VS Code within the memory-managed slice.
# All child processes (rust-analyzer, tsgo, etc.) inherit the cgroup.
#
# This script REPLACES /usr/bin/code (which was a symlink to
# /usr/share/code/bin/code). It covers every entry point:
# - Desktop launch (.desktop Exec=code %F)
# - Terminal: code .
# - Toolbox: flatpak-spawn --host code
# - Scripts: any invocation of /usr/bin/code

# Preserve the original VS Code remote-cli behavior
if [ -n "$VSCODE_IPC_HOOK_CLI" ]; then
    REMOTE_CLI="$(which -a 'code' | grep /remote-cli/)"
    if [ -n "$REMOTE_CLI" ]; then
        "$REMOTE_CLI" "$@"
        exit $?
    fi
fi

exec systemd-run --user \
    --slice=app-vscode.slice \
    --scope \
    --unit="vscode-$$-$(date +%N)" \
    --description="VS Code (managed)" \
    --property=MemoryOOMGroup=yes \
    -- /usr/share/code/bin/code "$@"
```

**Key details:**

- Replaces `/usr/bin/code` (originally a symlink to `/usr/share/code/bin/code`)
- Calls the original launcher script at `/usr/share/code/bin/code` (not the
  electron binary directly) to preserve VS Code's WSL detection, path
  resolution, and CLI initialization
- Uses `$$-$(date +%N)` for the unit name (PID + nanoseconds) to avoid
  collisions on rapid successive launches
- Sets `MemoryOOMGroup=yes` so that when `MemoryMax` triggers, the entire
  VS Code process tree is killed as a unit rather than picking off individual
  renderer processes (which leaves a broken, partially-alive VS Code)
- Preserves the `VSCODE_IPC_HOOK_CLI` remote-cli passthrough for VS Code
  Server scenarios

### 5. Edge Wrapper

`scripts/msedge-managed`:

```bash
#!/bin/bash
# Launch Microsoft Edge within the memory-managed slice.
# Replaces /usr/bin/microsoft-edge-stable (originally a symlink
# to /opt/microsoft/msedge/microsoft-edge).

exec systemd-run --user \
    --slice=app-msedge.slice \
    --scope \
    --unit="msedge-$$-$(date +%N)" \
    --description="Microsoft Edge (managed)" \
    --property=MemoryOOMGroup=yes \
    -- /usr/lib/opt/microsoft/msedge/microsoft-edge "$@"
```

**Key details:**

- Replaces `/usr/bin/microsoft-edge-stable`
- On this bootc image, `/opt` is relocated to `/usr/lib/opt` for ostree
  compatibility, so the real binary is at `/usr/lib/opt/microsoft/msedge/microsoft-edge`
- `/usr/bin/microsoft-edge` (the alternatives symlink) still points to
  `microsoft-edge-stable`, so it also goes through our wrapper

### Containerfile Integration

The wrappers need both COPY and RUN instructions, making this a **Run**-type
module in the `bkt` Containerfile generator:

**COPYs** (in the `collect-config` stage):

```dockerfile
COPY systemd/user/app-vscode.slice /usr/lib/systemd/user/app-vscode.slice
COPY systemd/user/app-msedge.slice /usr/lib/systemd/user/app-msedge.slice
COPY system/etc/systemd/oomd.conf.d/10-bootc-tuning.conf /etc/systemd/oomd.conf.d/10-bootc-tuning.conf
COPY scripts/code-managed /usr/bin/code
COPY scripts/msedge-managed /usr/bin/microsoft-edge-stable
```

**Consolidated RUN** additions:

```bash
chmod 0755 /usr/bin/code /usr/bin/microsoft-edge-stable;
```

Note: The `COPY scripts/code-managed /usr/bin/code` instruction deliberately
overwrites the symlink that the VS Code RPM installed. This is safe because
the image is immutable at runtime — no RPM updates can restore the symlink.
The same applies to the Edge wrapper.

### Why a Slice (not per-scope limits)

GNOME Shell creates a new transient scope (`app-gnome-code-PID.scope`) for each
VS Code launch. Even with our wrapper placing VS Code into `app-vscode.slice`,
GNOME's scope is separate. The slice aggregates all scopes:

- Without slice: Two instances × 24 GB each = 48 GB potential
- With slice: All instances share a single 24 GB budget

The wrapper's `--slice=app-vscode.slice` tells systemd-run to place the new
scope _inside_ the slice. All processes under that scope (and any further
scopes in the same slice) share the slice's memory budget.

### Sizing Rationale

| Parameter            | Value   | Reasoning                                                                |
| -------------------- | ------- | ------------------------------------------------------------------------ |
| VS Code MemoryHigh   | 20 GB   | 33% above baseline. Breathes normally, pressure on heavy work.           |
| VS Code MemoryMax    | 24 GB   | ~39% of 62 GB. Prevents multi-workspace crash.                           |
| Edge MemoryHigh      | 8 GB    | Generous for browser tabs. Pressure for heavy sessions.                  |
| Edge MemoryMax       | 12 GB   | Hard stop. Tabs die before system does.                                  |
| oomd global pressure | 60%/20s | Tighter than default (60%/30s). Catches system-wide issues.              |
| oomd per-slice limit | 50%     | Stricter for known offenders (VS Code, Edge).                            |
| oomd swap limit      | 90%     | Default. With swappiness=10, swap fills slowly — PSI is primary defense. |

**Total reserved**: VS Code (24 GB) + Edge (12 GB) = 36 GB worst case.
Remaining: 26 GB for OS, GNOME Shell (3.5 GB), Steam (4 GB), Slack, etc.

### Safety: What Gets Killed

With `MemoryOOMGroup=yes` on each scope, a `MemoryMax` kill takes down the
entire VS Code (or Edge) process tree as a unit. This is intentional:

- **Without `MemoryOOMGroup`**: The kernel picks the process with the highest
  `oom_score_adj` (renderer processes, `oom_score_adj=300`). This leaves a
  partially-alive VS Code with missing tabs and broken extension hosts — worse
  than a clean restart.
- **With `MemoryOOMGroup`**: All processes in the scope die together. VS Code
  can be cleanly relaunched, and session restore recovers open files.

For `MemoryHigh` (soft limit) pressure, no killing occurs — the kernel just
reclaims pages aggressively within the cgroup.

## Drawbacks

### Wrapper Fragility

The wrappers hardcode paths to the underlying binaries:

- VS Code: `/usr/share/code/bin/code`
- Edge: `/usr/lib/opt/microsoft/msedge/microsoft-edge`

If either RPM changes its binary location, the wrapper breaks. Mitigation:
these paths have been stable across major version updates. A broken wrapper
produces a clear error ("file not found") rather than silent misbehavior.

### No Aggregate System Budget

Individual app slices don't prevent the sum of all apps from exceeding RAM.
A misbehaving app without a slice can still cause OOM. Mitigation: oomd's
tighter global thresholds (60%/20s) provide a backstop for unmanaged apps.

### systemd-run Overhead

Each VS Code launch goes through `systemd-run`, which creates a transient
scope unit. This adds ~50ms to startup. Negligible for an application that
takes 2-3 seconds to fully initialize.

## Rationale and Alternatives

### Alternative: oomd-only (no MemoryMax)

Rely purely on tighter oomd thresholds. Problem: oomd kills the
highest-pressure process _across the session_, which might be GNOME Shell
rather than the actual offender. MemoryMax ensures the offender's scope
is contained.

### Alternative: skel .desktop Override (instead of wrapper replacement)

Place a `.desktop` file in `skel/` that calls a wrapper script. Problem:
only covers desktop launches. Terminal `code .` invocations, toolbox
`flatpak-spawn --host code`, and scripts all bypass the override.
Replacing `/usr/bin/code` covers every entry point.

### Alternative: VS Code `--max-memory` flag

VS Code supports `--max-memory=4096` to limit the main process heap. But
this doesn't limit renderer processes, extension hosts, or child processes
like rust-analyzer. The systemd cgroup approach covers everything.

### Alternative: Electron `--js-flags="--max-old-space-size=4096"`

Same problem — only limits V8 heap in one process. rust-analyzer is a native
binary unaffected by V8 flags.

### Alternative: `ulimit -v`

Per-process virtual memory limit. Would break VS Code immediately since
Chromium maps huge virtual address spaces (1.4 TB observed in crash data)
that are mostly unused. Cgroup memory limits track actual RSS, not virtual.

## Future Possibilities

- **`bkt admin memory`** — CLI to manage per-app memory budgets:
  `bkt admin memory set vscode --max 32G`
- **Dynamic adjustment** — Monitor actual usage patterns and auto-tune limits
- **Per-workspace limits** — Separate budgets per VS Code workspace using
  `--user-data-dir` detection
- **Browser tab integration** — Use Edge's managed policies + MemoryMax
  together for defense-in-depth
- **Flatpak apps** — Similar slices for Slack, Telegram, etc. if they
  become memory offenders
- **Manifest-driven policy** — Represent per-app memory budgets as manifest
  data (`manifests/memory-policy.json`) so `bkt` can diff, validate, and
  regenerate policies like other subsystems
- **Percentage-based scaling** — Express limits as RAM percentages
  (`MemoryHigh=30%`, `MemoryMax=40%`) so policies work across machines
  with different RAM sizes

## Validation Results

Runtime experiments (2026-02-15) confirmed the design:

### GNOME Scope Interaction ✓

When VS Code is launched via `systemd-run --user --slice=app-vscode.slice
--scope`, it lands **inside our slice** as intended:

```
├─app-vscode.slice
│ └─test-vscode-39397.scope    ← Our wrapper's scope
│   ├─367318 sh /usr/share/code/bin/code --wait
│   ├─367358 /usr/share/code/code ...
│   └─... (all VS Code processes)
```

GNOME Shell does **not** create a duplicate `app-gnome-code-*.scope` for
processes that are already in a systemd scope. The wrapper approach works.

### Leaf Cgroup Structure ✓

All VS Code child processes (rust-analyzer, tsgo, extension hosts, renderers)
live **directly in the scope** — no nested child cgroups:

```
app-vscode.slice: 111M        ← Slice (aggregates all scopes)
test-vscode-39397.scope: 110M ← Scope (leaf cgroup, all processes here)
```

This means:

- `MemoryOOMGroup=yes` will kill the entire process tree cleanly
- oomd's leaf-cgroup targeting correctly identifies the scope as the kill target

### Swap Behavior

With `vm.swappiness=10`, swap is a last resort. PSI-based pressure monitoring
(via `ManagedOOMMemoryPressure=kill` on the slices) is the primary defense.
The `SwapUsedLimit=90%` setting is a secondary backstop for edge cases where
swap fills before pressure thresholds trigger.
