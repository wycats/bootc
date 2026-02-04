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
use std::os::unix::io::{AsRawFd, FromRawFd};
use std::process::{Command, Stdio};

pub fn run(args: UpgradeArgs) -> Result<()> {
    // Create a pipe for progress events
    let (read_fd, write_fd) = nix::unistd::pipe()?;
    
    // Spawn bootc with --progress-fd pointing to our pipe
    let mut child = Command::new("bootc")
        .arg("upgrade")
        .arg(format!("--progress-fd={}", write_fd))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    
    // Close write end in parent
    drop(write_fd);
    
    // Read progress events from pipe
    let reader = BufReader::new(unsafe { File::from_raw_fd(read_fd) });
    
    for line in reader.lines() {
        let event: Event = serde_json::from_str(&line?)?;
        update_display(&event, &args);
    }
    
    child.wait()?;
    Ok(())
}
```

### Display Rendering

The compact display uses ANSI escape codes for in-place updates:

```rust
fn render_progress_bar(current: u64, total: u64, width: usize) -> String {
    let filled = (current as f64 / total as f64 * width as f64) as usize;
    let empty = width - filled;
    format!("[{}{}]", "█".repeat(filled), "░".repeat(empty))
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_000_000 {
        format!("{:.1} MB", bytes as f64 / 1_000_000.0)
    } else if bytes >= 1_000 {
        format!("{:.1} KB", bytes as f64 / 1_000.0)
    } else {
        format!("{} B", bytes)
    }
}
```

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
    
    /// Apply update immediately (requires reboot)
    #[arg(long)]
    apply: bool,
}
```

### Subcommands

```
bkt upgrade              # Stage upgrade for next boot
bkt upgrade --check      # Check without staging
bkt upgrade --apply      # Stage and trigger reboot (with confirmation)
```

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

1. **Reboot integration**: Should `bkt upgrade --apply` trigger reboot automatically
   or just print the command?

2. **Rollback display**: Should we show rollback image info in status?

3. **Multi-image**: If bootc ever supports multiple images, how do we handle that?

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
