//! Tune command implementation.
//!
//! Proactive optimization commands for the system:
//! - `tune memory` - Analyze and reclaim system memory (RAM, swap, GPU, caches)
//! - `tune layers` - Analyze and suggest layer groupings (RFC-0050, not yet implemented)
//! - `tune prune` - Clean up ostree deployments/objects (RFC-0050, not yet implemented)

use crate::output::Output;
use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use owo_colors::OwoColorize;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Args)]
pub struct TuneArgs {
    #[command(subcommand)]
    pub command: TuneCommand,
}

#[derive(Debug, Subcommand)]
pub enum TuneCommand {
    /// Analyze and reclaim system memory (RAM, swap, GPU, caches)
    ///
    /// By default shows what could be reclaimed; use --apply to act.
    #[command(alias = "mem")]
    Memory(MemoryArgs),

    /// Analyze external packages and suggest layer groupings
    ///
    /// (RFC-0050 - not yet implemented)
    Layers(LayersArgs),

    /// Clean up ostree deployments and unreferenced objects
    ///
    /// (RFC-0050 - not yet implemented)
    Prune(PruneArgs),
}

pub fn run(args: TuneArgs) -> Result<()> {
    match args.command {
        TuneCommand::Memory(memory_args) => run_memory(memory_args),
        TuneCommand::Layers(_) => {
            Output::warning("bkt tune layers is not yet implemented (see RFC-0050)");
            Ok(())
        }
        TuneCommand::Prune(_) => {
            Output::warning("bkt tune prune is not yet implemented (see RFC-0050)");
            Ok(())
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Memory subcommand
// ─────────────────────────────────────────────────────────────────────────────

/// Thresholds for "bloated" warnings
const SWAP_WARN_MB: u64 = 100;
const GTT_WARN_MB: u64 = 500;
const MEM_AVAILABLE_LOW_PCT: u64 = 10;

#[derive(Debug, Args)]
pub struct MemoryArgs {
    /// Actually perform reclamation (default is dry-run analysis)
    #[arg(long)]
    apply: bool,

    /// Show current memory state only (no recommendations)
    #[arg(long)]
    status: bool,

    /// Skip confirmation prompts (with --apply)
    #[arg(long, short = 'y')]
    yes: bool,

    /// Output format
    #[arg(short, long, default_value = "table")]
    format: String,
}

/// Collected system status
#[derive(Default)]
struct SystemStatus {
    // Memory (in KB)
    mem_total: u64,
    mem_available: u64,
    #[allow(dead_code)]
    mem_free: u64,
    cached: u64,
    slab_reclaimable: u64,
    swap_total: u64,
    swap_free: u64,
    swap_used: u64,
    reclaimable: u64,

    // Memory pressure
    pressure_avg10: Option<f64>,

    // GPU (in bytes)
    gpu_card: Option<String>,
    vram_total: u64,
    vram_used: u64,
    #[allow(dead_code)]
    gtt_total: u64,
    gtt_used: u64,
    gpu_busy: Option<u64>,

    // Compositor
    #[allow(dead_code)]
    compositor_pid: Option<u32>,
    compositor_name: Option<String>,
    #[allow(dead_code)]
    compositor_vram: u64,
    compositor_gtt: u64,
}

#[derive(Debug, Clone, PartialEq)]
enum Action {
    DropCaches,
    FlushSwap,
    RestartCompositor,
}

fn run_memory(args: MemoryArgs) -> Result<()> {
    let status = collect_status()?;
    let mut actions = Vec::new();

    // Display status
    show_status(&status, &mut actions);

    if args.status {
        return Ok(());
    }

    if args.format == "json" {
        return output_json(&status, &actions);
    }

    // No actions needed?
    if actions.is_empty() {
        Output::blank();
        Output::success("System memory is healthy - no reclamation needed");
        return Ok(());
    }

    // Show recommended actions
    Output::header("Recommended Actions");
    for action in &actions {
        match action {
            Action::DropCaches => {
                println!(
                    "  {} Drop filesystem caches (~{})",
                    "•".cyan(),
                    human_size_kb(status.reclaimable)
                );
            }
            Action::FlushSwap => {
                println!(
                    "  {} Flush swap to RAM (~{})",
                    "•".cyan(),
                    human_size_kb(status.swap_used)
                );
            }
            Action::RestartCompositor => {
                println!("  {} Address compositor GPU memory bloat", "•".cyan());
            }
        }
    }
    println!();

    if !args.apply {
        Output::info("This was a dry run. Use --apply to actually reclaim memory.");
        return Ok(());
    }

    // Check if we need root for cache/swap operations
    let needs_root = actions
        .iter()
        .any(|a| matches!(a, Action::DropCaches | Action::FlushSwap));

    if needs_root && !is_root() {
        Output::error("Root privileges required for cache/swap operations");
        Output::hint("Run: sudo bkt tune memory --apply");
        return Ok(());
    }

    // Confirm unless --yes
    if !args.yes {
        print!("Proceed with reclamation? [y/N] ");
        use std::io::{self, Write};
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            Output::info("Aborted");
            return Ok(());
        }
    }

    println!();

    // Execute actions
    for action in &actions {
        match action {
            Action::DropCaches => execute_drop_caches(&status)?,
            Action::FlushSwap => execute_flush_swap(&status)?,
            Action::RestartCompositor => show_compositor_advice(&status),
        }
    }

    // Show final state
    Output::header("Final State");
    let final_status = collect_status()?;
    let mut dummy_actions = Vec::new();
    show_status(&final_status, &mut dummy_actions);

    Ok(())
}

fn collect_status() -> Result<SystemStatus> {
    let mut status = SystemStatus::default();

    // Parse /proc/meminfo
    let meminfo = fs::read_to_string("/proc/meminfo").context("Failed to read /proc/meminfo")?;
    let mem_values = parse_meminfo(&meminfo);

    status.mem_total = mem_values.get("MemTotal").copied().unwrap_or(0);
    status.mem_available = mem_values.get("MemAvailable").copied().unwrap_or(0);
    status.mem_free = mem_values.get("MemFree").copied().unwrap_or(0);
    status.cached = mem_values.get("Cached").copied().unwrap_or(0);
    status.slab_reclaimable = mem_values.get("SReclaimable").copied().unwrap_or(0);
    status.swap_total = mem_values.get("SwapTotal").copied().unwrap_or(0);
    status.swap_free = mem_values.get("SwapFree").copied().unwrap_or(0);
    status.swap_used = status.swap_total.saturating_sub(status.swap_free);
    status.reclaimable = status.cached + status.slab_reclaimable;

    // Memory pressure
    if let Ok(pressure) = fs::read_to_string("/proc/pressure/memory") {
        // Parse: some avg10=0.00 avg60=0.01 avg300=0.00 total=126992
        for line in pressure.lines() {
            if line.starts_with("some")
                && let Some(avg10) = line
                    .split_whitespace()
                    .find(|s| s.starts_with("avg10="))
                    .and_then(|s| s.strip_prefix("avg10="))
                    .and_then(|s| s.parse::<f64>().ok())
            {
                status.pressure_avg10 = Some(avg10);
            }
        }
    }

    // GPU memory - find the primary card
    for card_num in [1, 0] {
        let card_path = format!("/sys/class/drm/card{}", card_num);
        let vram_path = format!("{}/device/mem_info_vram_total", card_path);

        if Path::new(&vram_path).exists() {
            status.gpu_card = Some(card_path.clone());

            status.vram_total =
                read_sysfs_u64(&format!("{}/device/mem_info_vram_total", card_path));
            status.vram_used = read_sysfs_u64(&format!("{}/device/mem_info_vram_used", card_path));
            status.gtt_total = read_sysfs_u64(&format!("{}/device/mem_info_gtt_total", card_path));
            status.gtt_used = read_sysfs_u64(&format!("{}/device/mem_info_gtt_used", card_path));
            status.gpu_busy = read_sysfs_u64_opt(&format!("{}/device/gpu_busy_percent", card_path));

            break;
        }
    }

    // Find compositor
    for compositor in ["gnome-shell", "kwin_wayland", "sway"] {
        if let Ok(output) = Command::new("pgrep").arg("-x").arg(compositor).output()
            && output.status.success()
            && let Ok(pid_str) = String::from_utf8(output.stdout)
            && let Ok(pid) = pid_str.trim().parse::<u32>()
        {
            status.compositor_pid = Some(pid);
            status.compositor_name = Some(compositor.to_string());

            // Get compositor GPU memory from fdinfo
            let (vram, gtt) = get_process_gpu_memory(pid);
            status.compositor_vram = vram;
            status.compositor_gtt = gtt;
            break;
        }
    }

    Ok(status)
}

fn parse_meminfo(content: &str) -> HashMap<String, u64> {
    let mut values = HashMap::new();
    for line in content.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            let key = parts[0].trim_end_matches(':');
            if let Ok(value) = parts[1].parse::<u64>() {
                values.insert(key.to_string(), value);
            }
        }
    }
    values
}

fn read_sysfs_u64(path: &str) -> u64 {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

fn read_sysfs_u64_opt(path: &str) -> Option<u64> {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().parse().ok())
}

fn get_process_gpu_memory(pid: u32) -> (u64, u64) {
    let fdinfo_dir = format!("/proc/{}/fdinfo", pid);
    let mut vram_kb: u64 = 0;
    let mut gtt_kb: u64 = 0;

    if let Ok(entries) = fs::read_dir(&fdinfo_dir) {
        for entry in entries.flatten() {
            if let Ok(content) = fs::read_to_string(entry.path()) {
                for line in content.lines() {
                    if let Some(rest) = line.strip_prefix("drm-memory-vram:") {
                        if let Some(val) = rest.split_whitespace().next() {
                            vram_kb += val.parse::<u64>().unwrap_or(0);
                        }
                    } else if let Some(rest) = line.strip_prefix("drm-memory-gtt:")
                        && let Some(val) = rest.split_whitespace().next()
                    {
                        gtt_kb += val.parse::<u64>().unwrap_or(0);
                    }
                }
            }
        }
    }

    // Convert from KiB to bytes
    (vram_kb * 1024, gtt_kb * 1024)
}

fn show_status(status: &SystemStatus, actions: &mut Vec<Action>) {
    // System Memory
    Output::header("System Memory");

    let mem_used_pct = if status.mem_total > 0 {
        ((status.mem_total - status.mem_available) * 100) / status.mem_total
    } else {
        0
    };

    println!("  Total:       {}", human_size_kb(status.mem_total));
    println!(
        "  Available:   {} ({}% used)",
        human_size_kb(status.mem_available),
        mem_used_pct
    );
    println!(
        "  Reclaimable: {} (cache + slab)",
        human_size_kb(status.reclaimable)
    );

    if let Some(pressure) = status.pressure_avg10 {
        println!("  Pressure:    {:.2} (avg10)", pressure);
    }

    // Check if memory is low
    let available_pct = if status.mem_total > 0 {
        (status.mem_available * 100) / status.mem_total
    } else {
        100
    };
    if available_pct < MEM_AVAILABLE_LOW_PCT {
        Output::warning(format!("Available memory is low ({}%)", available_pct));
        actions.push(Action::DropCaches);
    }

    // Swap
    Output::header("Swap");

    if status.swap_total == 0 {
        println!("  No swap configured");
    } else {
        let swap_pct = if status.swap_total > 0 {
            (status.swap_used * 100) / status.swap_total
        } else {
            0
        };
        println!("  Total:       {}", human_size_kb(status.swap_total));
        println!(
            "  Used:        {} ({}%)",
            human_size_kb(status.swap_used),
            swap_pct
        );

        let swap_used_mb = status.swap_used / 1024;
        if swap_used_mb > SWAP_WARN_MB {
            Output::warning("Swap usage is elevated");
            actions.push(Action::FlushSwap);
        } else {
            Output::success("Swap usage is healthy");
        }
    }

    // GPU Memory
    if status.gpu_card.is_some() {
        Output::header("GPU Memory");

        let vram_pct = if status.vram_total > 0 {
            (status.vram_used * 100) / status.vram_total
        } else {
            0
        };

        println!(
            "  VRAM:        {} / {} ({}%)",
            human_size_bytes(status.vram_used),
            human_size_bytes(status.vram_total),
            vram_pct
        );
        println!("  GTT (spillover): {}", human_size_bytes(status.gtt_used));

        if let Some(busy) = status.gpu_busy {
            println!("  GPU busy:    {}%", busy);
        }

        let gtt_mb = status.gtt_used / 1024 / 1024;
        if gtt_mb > GTT_WARN_MB {
            Output::warning(format!(
                "GTT spillover is elevated ({}MB > {}MB threshold)",
                gtt_mb, GTT_WARN_MB
            ));

            if let Some(ref name) = status.compositor_name {
                let comp_gtt_mb = status.compositor_gtt / 1024 / 1024;
                println!(
                    "  └─ {} using {} GTT",
                    name,
                    human_size_bytes(status.compositor_gtt)
                );

                if comp_gtt_mb > GTT_WARN_MB {
                    actions.push(Action::RestartCompositor);
                }
            }
        } else {
            Output::success("GPU memory is healthy");
        }
    }
}

fn execute_drop_caches(status: &SystemStatus) -> Result<()> {
    Output::info("Syncing filesystems...");
    Command::new("sync").status()?;

    Output::info("Dropping caches...");
    fs::write("/proc/sys/vm/drop_caches", "3").context("Failed to drop caches")?;

    // Re-measure
    let new_available = {
        let meminfo = fs::read_to_string("/proc/meminfo")?;
        let values = parse_meminfo(&meminfo);
        values.get("MemAvailable").copied().unwrap_or(0)
    };

    let freed = new_available.saturating_sub(status.mem_available);
    Output::success(format!("Freed {} of memory", human_size_kb(freed)));

    Ok(())
}

fn execute_flush_swap(status: &SystemStatus) -> Result<()> {
    let swap_mb = status.swap_used / 1024;
    let mem_available_mb = status.mem_available / 1024;
    let needed = swap_mb + 512; // 512MB safety margin

    if mem_available_mb < needed {
        Output::warning(format!(
            "Insufficient RAM ({}MB) to absorb swap ({}MB)",
            mem_available_mb, swap_mb
        ));
        Output::info("Dropping caches first...");
        execute_drop_caches(status)?;

        // Recheck
        let new_available = {
            let meminfo = fs::read_to_string("/proc/meminfo")?;
            let values = parse_meminfo(&meminfo);
            values.get("MemAvailable").copied().unwrap_or(0) / 1024
        };

        if new_available < swap_mb {
            Output::error("Still insufficient RAM after dropping caches");
            return Ok(());
        }
    }

    Output::info("Disabling swap (migrating pages to RAM)...");
    let swapoff = Command::new("swapoff").arg("-a").status()?;

    if !swapoff.success() {
        Output::error("swapoff failed - system under memory pressure");
        return Ok(());
    }

    Output::info("Re-enabling swap...");
    Command::new("swapon").arg("-a").status()?;

    Output::success("Swap flushed successfully");
    Ok(())
}

fn show_compositor_advice(status: &SystemStatus) {
    let comp_gtt_mb = status.compositor_gtt / 1024 / 1024;
    let name = status.compositor_name.as_deref().unwrap_or("compositor");

    Output::warning(format!(
        "Compositor {} is using {}MB of GTT memory",
        name, comp_gtt_mb
    ));
    println!();
    println!("  To reclaim GPU memory, you can:");
    println!("    1. Log out and back in (recommended)");
    println!(
        "    2. Restart the compositor: systemctl --user restart {}",
        name
    );
    println!();
    println!("  This is often caused by:");
    println!("    • Many windows/tabs open over time");
    println!("    • GPU-accelerated apps not releasing memory");
    println!("    • Insufficient VRAM allocation (check BIOS settings)");
}

fn output_json(status: &SystemStatus, actions: &[Action]) -> Result<()> {
    let json = serde_json::json!({
        "memory": {
            "total_kb": status.mem_total,
            "available_kb": status.mem_available,
            "reclaimable_kb": status.reclaimable,
            "pressure_avg10": status.pressure_avg10,
        },
        "swap": {
            "total_kb": status.swap_total,
            "used_kb": status.swap_used,
        },
        "gpu": {
            "vram_total": status.vram_total,
            "vram_used": status.vram_used,
            "gtt_total": status.gtt_total,
            "gtt_used": status.gtt_used,
            "busy_percent": status.gpu_busy,
        },
        "compositor": {
            "name": status.compositor_name,
            "pid": status.compositor_pid,
            "vram": status.compositor_vram,
            "gtt": status.compositor_gtt,
        },
        "actions": actions.iter().map(|a| format!("{:?}", a)).collect::<Vec<_>>(),
    });

    println!("{}", serde_json::to_string_pretty(&json)?);
    Ok(())
}

fn human_size_kb(kb: u64) -> String {
    if kb >= 1_048_576 {
        format!("{:.1}G", kb as f64 / 1_048_576.0)
    } else if kb >= 1024 {
        format!("{}M", kb / 1024)
    } else {
        format!("{}K", kb)
    }
}

fn human_size_bytes(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.1}G", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1_048_576 {
        format!("{}M", bytes / 1_048_576)
    } else if bytes >= 1024 {
        format!("{}K", bytes / 1024)
    } else {
        format!("{}B", bytes)
    }
}

fn is_root() -> bool {
    // Check effective UID
    unsafe { libc::geteuid() == 0 }
}

// ─────────────────────────────────────────────────────────────────────────────
// Layers subcommand (RFC-0050 - stub)
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Args)]
pub struct LayersArgs {
    /// Apply suggested layer groupings to manifests
    #[arg(long)]
    pub apply: bool,
}

// ─────────────────────────────────────────────────────────────────────────────
// Prune subcommand (RFC-0050 - stub)
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Args)]
pub struct PruneArgs {
    /// Keep rollback deployment (default)
    #[arg(long, default_value = "true")]
    pub keep_rollback: bool,

    /// Remove rollback deployment to free hardlinks
    #[arg(long)]
    pub remove_rollback: bool,

    /// Remove unreferenced ostree objects
    #[arg(long)]
    pub prune_objects: bool,
}
