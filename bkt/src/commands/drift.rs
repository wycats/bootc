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

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand, ValueEnum};
use owo_colors::OwoColorize;
use std::env;
use std::path::PathBuf;
use std::process::Command;

use crate::manifest::find_repo_root;
use crate::output::Output;
use crate::subsystem::SubsystemRegistry;

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

impl DriftCategory {
    pub fn subsystem_ids(&self, registry: &SubsystemRegistry) -> Vec<&'static str> {
        match self {
            DriftCategory::Packages => vec!["system"],
            DriftCategory::Flatpaks => vec!["flatpak"],
            DriftCategory::Extensions => vec!["extension"],
            DriftCategory::All => registry.driftable_ids(),
        }
    }
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
    format: OutputFormat,
    no_host: bool,
) -> Result<()> {
    let repo_root = get_repo_root()?;
    let registry = SubsystemRegistry::builtin();
    let script_path = repo_root.join("scripts").join("check-drift");

    if !script_path.exists() {
        bail!(
            "Drift detection script not found at {}. \
             Make sure you're running from the bootc repository.",
            script_path.display()
        );
    }

    let mut cmd = Command::new("python3");
    cmd.arg(&script_path);
    cmd.current_dir(&repo_root);

    // Add format flag
    match format {
        OutputFormat::Json => {
            cmd.arg("--json");
        }
        OutputFormat::Human => {}
    }

    // Add no-host flag
    if no_host {
        cmd.arg("--no-host");
    }

    // Add category filter (the Python script doesn't support this yet, but we prepare for it)
    if let Some(cat) = &category {
        let ids = cat.subsystem_ids(&registry);
        let invalid_ids: Vec<_> = ids
            .iter()
            .copied()
            .filter(|id| !registry.is_valid_driftable(id))
            .collect();

        if !invalid_ids.is_empty() {
            bail!(
                "Drift category '{:?}' maps to subsystems without drift support: {}",
                cat,
                invalid_ids.join(", ")
            );
        }
    }

    if let Some(cat) = &category
        && !matches!(cat, DriftCategory::All)
    {
        Output::warning(format!(
            "Category filter '{:?}' is not yet supported. Running full drift check.",
            cat
        ));
    }

    Output::info("Running drift detection...");
    Output::info(format!("Repository: {}", repo_root.display()));
    println!();

    let status = cmd
        .status()
        .with_context(|| format!("Failed to run {}", script_path.display()))?;

    match status.code() {
        Some(0) => {
            println!();
            Output::success("No drift detected (or only optional-tier changes).");
            Ok(())
        }
        Some(1) => {
            println!();
            Output::warning("Drift detected in baked or bootstrapped tiers.");
            Output::info("Review the output above to see what has drifted.");
            Ok(())
        }
        Some(2) => {
            bail!("Error collecting system state. Check the output above for details.");
        }
        Some(code) => {
            bail!("Drift check exited with unexpected code: {}", code);
        }
        None => {
            bail!("Drift check was terminated by a signal.");
        }
    }
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
