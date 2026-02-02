//! Drift detection command implementation.
//!
//! This module provides commands for detecting configuration drift between
//! the manifest declarations and the actual system state.
//!
//! Drift can occur when:
//! - Packages are installed/removed outside of `bkt`
//! - Flatpaks are added/removed via the GUI
//! - Extensions are enabled/disabled manually
//! - Settings are changed via the UI

use anyhow::{Context, Result};
use clap::{Args, Subcommand, ValueEnum};
use owo_colors::OwoColorize;
use std::env;
use std::path::PathBuf;

use crate::manifest::find_repo_root;
use crate::output::Output;

#[derive(Debug, Args)]
pub struct DriftArgs {
    #[command(subcommand)]
    pub action: DriftAction,
}

#[derive(Debug, Subcommand)]
pub enum DriftAction {
    /// Check for drift between manifests and system state
    Check {
        /// Category to check (default: all)
        #[arg(value_enum)]
        category: Option<DriftCategory>,

        /// Output format
        #[arg(short, long, value_enum, default_value = "human")]
        format: OutputFormat,

        /// Don't re-execute on host when running in toolbox
        #[arg(long)]
        no_host: bool,
    },

    /// Show summary of last drift check
    Status,

    /// Explain what drift detection does
    Explain,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum DriftCategory {
    /// Check RPM packages
    Packages,
    /// Check Flatpak applications
    Flatpaks,
    /// Check GNOME extensions
    Extensions,
    /// Check all categories
    All,
}

#[derive(Debug, Clone, Copy, ValueEnum, Default)]
pub enum OutputFormat {
    /// Human-readable output
    #[default]
    Human,
    /// JSON output
    Json,
}

pub fn run(args: DriftArgs) -> Result<()> {
    match args.action {
        DriftAction::Check {
            category,
            format,
            no_host,
        } => handle_check(category, format, no_host),
        DriftAction::Status => handle_status(),
        DriftAction::Explain => handle_explain(),
    }
}

fn get_repo_root() -> Result<PathBuf> {
    let cwd = env::current_dir().context("Failed to get current directory")?;
    find_repo_root(&cwd)
        .context("Not in a git repository. Run this command from within the bootc repository.")
}

fn handle_check(
    category: Option<DriftCategory>,
    _format: OutputFormat,
    _no_host: bool,
) -> Result<()> {
    // TODO: Implement drift detection natively in Rust
    // See RFC 0007 for the full design
    //
    // The previous implementation depended on a Python script which violates
    // the project's "No Custom Python Scripts" axiom (see docs/VISION.md).
    //
    // For now, this command explains what drift detection will do and
    // directs users to use `bkt capture` to detect changes.

    Output::warning("Drift detection is not yet implemented in Rust.");
    println!();

    if let Some(cat) = category {
        Output::info(format!("Category filter: {:?}", cat));
    }

    println!();
    println!("Drift detection will compare your system state against manifests.");
    println!("For now, you can use these alternatives:");
    println!();
    println!(
        "  {} - Capture current system state to manifests",
        "bkt capture".cyan()
    );
    println!(
        "  {} - Show what capture would change",
        "bkt capture --dry-run".cyan()
    );
    println!("  {} - See git diff after capture", "git diff".cyan());
    println!();
    println!(
        "Run {} for more information about drift detection.",
        "bkt drift explain".cyan()
    );

    Ok(())
}

fn handle_status() -> Result<()> {
    let repo_root = get_repo_root()?;
    let state_dir = repo_root.join(".local").join("state").join("bkt");
    let last_check = state_dir.join("last-drift-check.json");

    if !last_check.exists() {
        Output::info("No drift check has been run yet.");
        Output::info("Run 'bkt drift check' to perform a drift check.");
        return Ok(());
    }

    let content = std::fs::read_to_string(&last_check)
        .with_context(|| format!("Failed to read {}", last_check.display()))?;

    println!("{}", content);
    Ok(())
}

fn handle_explain() -> Result<()> {
    println!("{}", "Drift Detection".cyan().bold());
    println!();
    println!("Drift occurs when your system state diverges from your declared manifests.");
    println!();
    println!("{}", "Types of Drift:".yellow().bold());
    println!("  {} - Items installed outside of bkt", "Additive".green());
    println!("  {} - Items removed outside of bkt", "Subtractive".red());
    println!("  {} - Settings changed via GUI", "Modificational".blue());
    println!();
    println!("{}", "Detection Tiers:".yellow().bold());
    println!(
        "  {} - RPM packages, system extensions (from Containerfile)",
        "Baked".cyan()
    );
    println!(
        "  {} - Flatpaks, user extensions (from manifests)",
        "Bootstrapped".green()
    );
    println!(
        "  {} - User-installed items beyond manifests",
        "Optional".dimmed()
    );
    println!();
    println!("{}", "Exit Codes:".yellow().bold());
    println!("  {} - No drift (or only optional-tier)", "0".green());
    println!("  {} - Drift in baked/bootstrapped tiers", "1".yellow());
    println!("  {} - Error collecting state", "2".red());
    println!();
    println!("{}", "Commands:".yellow().bold());
    println!("  bkt drift check          - Run drift detection");
    println!("  bkt drift check --json   - Output as JSON");
    println!("  bkt drift status         - Show last check results");
    println!();
    Ok(())
}
