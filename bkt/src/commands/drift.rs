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
use serde::Serialize;
use std::env;
use std::path::PathBuf;

use crate::manifest::find_repo_root;
use crate::output::Output;
use crate::subsystem::{DriftReport, SubsystemContext, SubsystemRegistry};

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

struct SubsystemDrift {
    id: &'static str,
    name: &'static str,
    report: DriftReport,
}

#[derive(Serialize)]
struct DriftOutput {
    has_drift: bool,
    subsystems: Vec<SubsystemDriftOutput>,
}

#[derive(Serialize)]
struct SubsystemDriftOutput {
    id: &'static str,
    name: &'static str,
    expected: Vec<String>,
    actual: Vec<String>,
    missing: Vec<String>,
    extra: Vec<String>,
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

fn print_human(reports: &[SubsystemDrift], has_drift: bool) {
    if !has_drift {
        println!("{}", "✓ No drift detected".green());
        return;
    }

    for subsystem in reports.iter().filter(|r| r.report.has_drift()) {
        println!(
            "{}",
            format!("═══ {} ({}) ═══", subsystem.name, subsystem.id)
                .yellow()
                .bold()
        );

        if !subsystem.report.missing.is_empty() {
            println!("  {}", "missing:".red());
            for item in &subsystem.report.missing {
                println!("    - {}", item);
            }
        }

        if !subsystem.report.extra.is_empty() {
            println!("  {}", "extra:".blue());
            for item in &subsystem.report.extra {
                println!("    + {}", item);
            }
        }

        println!();
    }
}

fn print_json(reports: &[SubsystemDrift], has_drift: bool) -> Result<()> {
    let subsystems = reports
        .iter()
        .map(|r| SubsystemDriftOutput {
            id: r.id,
            name: r.name,
            expected: r.report.expected.clone(),
            actual: r.report.actual.clone(),
            missing: r.report.missing.clone(),
            extra: r.report.extra.clone(),
        })
        .collect();

    let output = DriftOutput {
        has_drift,
        subsystems,
    };

    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
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
    if no_host {
        Output::warning("--no-host has no effect for native drift detection.");
    }

    let mut ctx = SubsystemContext::with_repo_root(repo_root.clone());
    if !ctx.system_manifest_dir.exists() {
        let repo_manifests = repo_root.join("manifests");
        if repo_manifests.exists() {
            ctx.system_manifest_dir = repo_manifests;
        }
    }

    let subsystems = match category {
        Some(cat) => {
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

            registry.filtered(Some(ids.as_slice()), &[])
        }
        None => registry.driftable(),
    };

    Output::info("Running drift detection...");
    Output::info(format!("Repository: {}", repo_root.display()));
    println!();

    let mut reports = Vec::new();
    for subsystem in subsystems {
        if let Some(report) = subsystem.drift(&ctx)? {
            reports.push(SubsystemDrift {
                id: subsystem.id(),
                name: subsystem.name(),
                report,
            });
        }
    }

    let has_drift = reports.iter().any(|r| r.report.has_drift());

    match format {
        OutputFormat::Json => print_json(&reports, has_drift)?,
        OutputFormat::Human => print_human(&reports, has_drift),
    }

    if has_drift {
        println!();
        Output::warning("Drift detected. Review the output above to see what has drifted.");
    } else {
        println!();
        Output::success("No drift detected.");
    }

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
