# RFC 0037: Upgrade Command (`bkt upgrade`)

- **Status**: Draft
- Feature Name: `bkt_upgrade`
- Start Date: 2026-02-03
- RFC PR: (leave this empty until PR is opened)
- Tracking Issue: (leave this empty)

## Summary

Implement `bkt upgrade` as a streamlined wrapper around `bootc upgrade` that provides
compact, informative progress output using bootc's `--progress-fd` JSON Lines streaming
API, with a display style inspired by git's terse diff summaries (`+223 -1`).

## Motivation

### Current Pain Points

```bash
# Check for updates
sudo bootc upgrade --check
# Output: verbose, hard to parse at a glance

# Actually upgrade
sudo bootc upgrade
# Output: TUI-style progress that's informative but noisy
```

The current `bootc upgrade` output is designed for interactive use with full terminal
control. For a tool like `bkt` that aims to be the "daily loop hub," we want:

1. **Terse summaries** - Like git's `+223 -1` for diffs
2. **Layer delta visibility** - Know what's changing without SHA noise
3. **Integration with status** - Seamless flow from `bkt status` → `bkt upgrade`
4. **Scriptable progress** - JSON output option for automation

### The Solution

```bash
# Check what would change
bkt upgrade --check
# Output:
#   Image: ghcr.io/wycats/bootc:latest
#   Staged: 41.20250203.0 → 41.20250204.0
#   Layers: 47 total, 3 changed
#     ├─ base    ████████████████████ 100% (cached)
#     ├─ layer12 ████████████████████ 100% (+2.3 MB)
#     └─ layer47 ████████████████████ 100% (+156 KB)
#   Delta: +2.5 MB

# Perform upgrade with compact progress
bkt upgrade
# Output:
#   Upgrading to 41.20250204.0...
#   [████████████████████] 47/47 layers (+2.5 MB)
#   ✓ Staged for next boot

# JSON output for scripting
bkt upgrade --check --format json
```

## Guide-level Explanation

### Relationship to `bkt admin bootc upgrade`

`bkt upgrade` is the primary, user-facing command with enhanced UX (compact progress,
git-inspired summaries, and JSON output). The existing `bkt admin bootc upgrade`
remains a **raw passthrough** to bootc for advanced users and scripting. Internally,
`bkt upgrade` may reuse similar execution patterns, but it owns the higher-level
display logic and progress parsing.

### Basic Usage

```bash
# Check for available updates (no changes made)
bkt upgrade --check

# Stage an upgrade for next boot
bkt upgrade

# Force re-pull even if up-to-date
bkt upgrade --force
```

### Output Modes

#### Default: Compact Progress

The default output shows a single-line progress bar that updates in place:

```
Upgrading to 41.20250204.0...
[████████████░░░░░░░░] 23/47 layers (1.2 MB / 2.5 MB)
```

On completion:

```
✓ Staged 41.20250204.0 for next boot (+2.5 MB, 3 layers changed)
  Run 'systemctl reboot' to apply
```

#### Verbose: Layer Details

With `--verbose`, show per-layer progress similar to git's file-by-file diff:

```
Upgrading to 41.20250204.0...
  base     ████████████████████ cached
  layer12  ████████████████████ +2.3 MB
  layer47  ████████████████████ +156 KB
  ─────────────────────────────────────
  47 layers, 3 changed              +2.5 MB

✓ Staged for next boot
```

#### JSON: Machine-Readable

```bash
bkt upgrade --format json
```

```json
{
  "action": "upgrade",
  "from": {
    "version": "41.20250203.0",
    "digest": "sha256:abc123..."
  },
  "to": {
    "version": "41.20250204.0",
    "digest": "sha256:def456..."
  },
  "layers": {
    "total": 47,
    "changed": 3,
    "cached": 44
  },
  "bytes": {
    "total": 2621440,
    "cached": 0,
    "fetched": 2621440
  },
  "status": "staged"
}
```

### Integration with `bkt status`

When an upgrade is available, `bkt status` will show:

```
  OS
    Version: 41.20250203.0
    Image:   ...ghcr.io/wycats/bootc:latest
    Update:  41.20250204.0 available (+2.5 MB)
```

After staging:

```
  OS
    Version: 41.20250203.0
    Staged:  41.20250204.0 (reboot to apply)
```

## Reference-level Explanation

### bootc `--progress-fd` API

bootc provides a `--progress-fd=<fd>` option that writes JSON Lines progress events
to the specified file descriptor. This is the primitive we'll use.

### Fallback Behavior

The `--progress-fd` API is **experimental** and may be absent or unstable depending
on the bootc version. When it is not available (or fails to initialize), `bkt upgrade`
will fall back to invoking `pkexec bootc upgrade` without `--progress-fd` and will
stream bootc's normal stdout/stderr output. In this mode:

- The command still supports `--check`, `--force`, `--apply`, `--confirm`, and `--yes`.
- `--format json` is limited to pre/post state summaries; no per-layer progress.
- A warning is printed to indicate degraded progress reporting.

#### Detection Strategy

Fallback is triggered when:

1. `bootc upgrade --help` output does not contain `--progress-fd` (checked once at startup), OR
2. The pipe read returns an immediate EOF or parse error on the first event

The detection result can be cached for the session to avoid repeated checks.

#### Event Types

From bootc's `progress_jsonl.rs`:

```rust
pub enum Event {
    ProgressBytes(SubTaskBytes),
    ProgressSteps(SubTaskSteps),
}

pub struct SubTaskBytes {
    pub subtask: String,
    pub description: String,
    pub id: u32,
    pub bytes: u64,
    pub bytes_total: Option<u64>,
    pub bytes_cached: Option<u64>,
}

pub struct LayerProgress {
    pub layer_index: u32,
    pub fetched: u64,
    pub total: u64,
}
```

### Implementation Strategy

```rust
// bkt/src/commands/upgrade.rs

use std::io::{BufRead, BufReader};
use std::os::fd::AsRawFd;
use std::process::{Command, Stdio};

use os_pipe::pipe;

pub fn run(args: UpgradeArgs, plan: &ExecutionPlan) -> Result<()> {
    // Handle --check mode (read-only, no confirmation needed)
    if args.check {
        return run_check(&args, plan);
    }

    // Require confirmation for mutating operations
    let confirmed = args.confirm || args.yes;
    if !confirmed {
        return Err(anyhow!("Use --confirm or --yes to proceed"));
    }

    // Dry-run support
    if plan.dry_run {
        Output::dry_run("Would execute: pkexec bootc upgrade");
        return Ok(());
    }

    // Interactive confirmation (unless --yes)
    if !args.yes && !prompt_continue("Stage upgrade for next boot?")? {
        Output::info("Cancelled.");
        return Ok(());
    }

    // Check if --progress-fd is available
    let use_progress_fd = supports_progress_fd();

    if use_progress_fd {
        run_with_progress(&args, plan)?;
    } else {
        Output::warning("Progress streaming unavailable; using standard output");
        run_fallback(&args, plan)?;
    }

    // Handle --apply: prompt for reboot
    if args.apply {
        if args.yes || prompt_continue("Reboot now to apply?")? {
            exec_reboot()?;
        } else {
            Output::info("Run 'systemctl reboot' when ready.");
        }
    }

    Ok(())
}

fn run_with_progress(args: &UpgradeArgs, plan: &ExecutionPlan) -> Result<()> {
    let (read_pipe, write_pipe) = pipe()?;

    let mut cmd = Command::new("pkexec");
    cmd.arg("bootc").arg("upgrade");
    cmd.arg(format!("--progress-fd={}", write_pipe.as_raw_fd()));

    if args.force {
        cmd.arg("--force");
    }

    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    drop(write_pipe); // Close write end in parent

    let reader = BufReader::new(read_pipe);
    for line in reader.lines() {
        let event: Event = serde_json::from_str(&line?)?;
        update_display(&event, &args);
    }

    child.wait()?;
    Ok(())
}
```

### Display Rendering

The compact display uses the existing `indicatif` patterns in the codebase for
consistent progress bars and in-place updates.

### Command Structure

```rust
#[derive(Debug, Args)]
pub struct UpgradeArgs {
    /// Check for updates without applying
    #[arg(long)]
    check: bool,

    /// Force re-pull even if up-to-date
    #[arg(long)]
    force: bool,

    /// Output format
    #[arg(short, long, value_enum, default_value = "table")]
    format: OutputFormat,

    /// Show verbose layer-by-layer progress
    #[arg(short, long)]
    verbose: bool,

    /// Require explicit confirmation before applying
    #[arg(long)]
    confirm: bool,

    /// Skip confirmation prompt
    #[arg(long)]
    yes: bool,

    /// Apply update immediately (requires reboot)
    #[arg(long)]
    apply: bool,
}
```

### Execution Target

`bkt upgrade` targets `CommandTarget::Host` (host execution required, not toolbox).
Privilege elevation must use `pkexec`, matching the `bkt admin bootc` flow, and
respect `plan.dry_run` and confirmation prompts.

### Subcommands

```
bkt upgrade              # Stage upgrade for next boot (requires --confirm or --yes)
bkt upgrade --check      # Check without staging (no confirmation needed)
bkt upgrade --apply      # Stage and prompt for reboot (requires --confirm or --yes)
```

### `--apply` Behavior

When `--apply` is specified:

1. The upgrade is staged normally
2. On success, the user is prompted: "Reboot now to apply? [y/N]"
3. If confirmed (or `--yes` was passed), `systemctl reboot` is executed
4. If declined, prints "Run 'systemctl reboot' when ready"

This avoids surprise reboots while providing a convenient single-command workflow.

## Drawbacks

1. **Dependency on experimental API**: `--progress-fd` is marked experimental in bootc
2. **Parsing complexity**: JSON Lines parsing adds code complexity
3. **Terminal compatibility**: Progress bars may not render well in all terminals

## Rationale and Alternatives

### Why `--progress-fd` over parsing stdout?

- **Structured data**: JSON is reliable; stdout parsing is fragile
- **Separation of concerns**: Progress events vs. status messages
- **Future-proof**: API is designed for programmatic consumption

### Alternative: Just wrap `bootc upgrade`

We could simply call `bootc upgrade` and let it handle display. However:

- No control over output format
- Can't integrate with `bkt status` display style
- No JSON output option

### Alternative: Parse `bootc status --format json` polling

We could poll `bootc status` during upgrade. However:

- Inefficient (repeated subprocess calls)
- No real-time progress
- Race conditions

## Prior Art

- **git**: Terse diff summaries (`+223 -1`) inspire our byte delta display
- **docker pull**: Layer-by-layer progress with caching indicators
- **cargo**: Compact progress with `Downloading [=====>   ] 3/10`
- **apt**: `Reading package lists... Done` style status updates

## Unresolved Questions

1. **Rollback display**: Should we show rollback image info in status?

2. **Multi-image**: If bootc ever supports multiple images, how do we handle that?

3. **Format vs progress**: Should `--format json` suppress the live progress bar entirely,
   or emit JSON Lines during progress and a final JSON summary?

## Future Possibilities

### `bkt upgrade --watch`

Watch for updates and notify (via desktop notification or terminal):

```bash
bkt upgrade --watch --notify
# Runs in background, sends notification when update available
```

### Integration with changelog

```bash
bkt upgrade
# Shows:
#   Upgrading to 41.20250204.0...
#   Changes in this version:
#     - feat: Add new flatpak apps
#     - fix: Correct gsettings schema
```

### Diff preview

```bash
bkt upgrade --diff
# Shows what packages/layers changed between versions
```
