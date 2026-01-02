//! Base image assumption management command implementation.
//!
//! This module provides commands for managing base image assumptions - the
//! packages, services, and paths that the base image (Bazzite) provides.
//!
//! By tracking assumptions, we can:
//! - Detect when the base image no longer provides expected packages
//! - Distinguish between "our additions" and "base image content"
//! - Get early warning of breaking changes in upstream

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use owo_colors::OwoColorize;
use std::env;
use std::path::PathBuf;
use std::process::Command;

use crate::manifest::{BaseImageAssumptions, find_repo_root};
use crate::output::Output;

#[derive(Debug, Args)]
pub struct BaseArgs {
    #[command(subcommand)]
    pub action: BaseAction,
}

#[derive(Debug, Subcommand)]
pub enum BaseAction {
    /// Verify current system against base image assumptions
    Verify,

    /// Add a package to base image assumptions
    Assume {
        /// Package name to add as assumption
        package: String,

        /// Description/reason for the assumption
        #[arg(short, long)]
        reason: Option<String>,
    },

    /// Remove a package from base image assumptions
    Unassume {
        /// Package name to remove
        package: String,
    },

    /// List current base image assumptions
    List,

    /// Show current base image info
    Info,

    /// Snapshot current base image packages (generates assumptions from system)
    Snapshot {
        /// Only include packages matching this pattern
        #[arg(short, long)]
        filter: Option<String>,

        /// Output to file instead of stdout
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Don't prompt for confirmation
        #[arg(long)]
        yes: bool,
    },
}

pub fn run(args: BaseArgs) -> Result<()> {
    match args.action {
        BaseAction::Verify => handle_verify(),
        BaseAction::Assume { package, reason } => handle_assume(package, reason),
        BaseAction::Unassume { package } => handle_unassume(package),
        BaseAction::List => handle_list(),
        BaseAction::Info => handle_info(),
        BaseAction::Snapshot {
            filter,
            output,
            yes,
        } => handle_snapshot(filter, output, yes),
    }
}

fn get_repo_root() -> Result<PathBuf> {
    let cwd = env::current_dir().context("Failed to get current directory")?;
    find_repo_root(&cwd)
        .context("Not in a git repository. Run this command from within the bootc repository.")
}

#[allow(dead_code)]
fn get_assumptions_path() -> Result<PathBuf> {
    let repo_root = get_repo_root()?;
    Ok(repo_root
        .join("manifests")
        .join("base-image-assumptions.json"))
}

fn handle_verify() -> Result<()> {
    let assumptions = BaseImageAssumptions::load_from_repo()?;

    if assumptions.packages.is_empty() {
        Output::warning("No base image assumptions configured.");
        Output::info("Use 'bkt base assume <package>' to add assumptions.");
        return Ok(());
    }

    Output::header("Verifying Base Image Assumptions");
    Output::info(format!(
        "Checking {} package assumptions...",
        assumptions.packages.len()
    ));
    println!();

    let mut missing = Vec::new();
    let mut present = Vec::new();

    for pkg in &assumptions.packages {
        if is_package_installed(&pkg.name)? {
            present.push(&pkg.name);
        } else {
            missing.push(&pkg.name);
        }
    }

    // Report results
    if missing.is_empty() {
        Output::success(format!(
            "All {} assumptions verified.",
            assumptions.packages.len()
        ));
    } else {
        Output::error(format!(
            "{} of {} assumptions failed:",
            missing.len(),
            assumptions.packages.len()
        ));
        for pkg in &missing {
            println!("  {} {}", "✗".red(), pkg);
        }
        println!();
        Output::warning(
            "These packages are expected in the base image but are missing. \
             This may indicate a breaking change in the upstream image.",
        );
        bail!(
            "Base image verification failed: {} missing packages",
            missing.len()
        );
    }

    Ok(())
}

fn handle_assume(package: String, reason: Option<String>) -> Result<()> {
    let mut assumptions = BaseImageAssumptions::load_from_repo()?;

    // Check if already assumed
    if assumptions.packages.iter().any(|p| p.name == package) {
        Output::warning(format!("Package '{}' is already in assumptions.", package));
        return Ok(());
    }

    // Verify the package exists
    if !is_package_installed(&package)? {
        Output::warning(format!(
            "Package '{}' is not currently installed. Adding anyway.",
            package
        ));
    }

    assumptions.add_package(&package, reason.as_deref());
    assumptions.save_to_repo()?;

    Output::success(format!("Added '{}' to base image assumptions.", package));
    Ok(())
}

fn handle_unassume(package: String) -> Result<()> {
    let mut assumptions = BaseImageAssumptions::load_from_repo()?;

    if !assumptions.remove_package(&package) {
        Output::warning(format!("Package '{}' was not in assumptions.", package));
        return Ok(());
    }

    assumptions.save_to_repo()?;
    Output::success(format!(
        "Removed '{}' from base image assumptions.",
        package
    ));
    Ok(())
}

fn handle_list() -> Result<()> {
    let assumptions = BaseImageAssumptions::load_from_repo()?;

    if assumptions.packages.is_empty() {
        Output::info("No base image assumptions configured.");
        Output::info("Use 'bkt base assume <package>' to add assumptions.");
        return Ok(());
    }

    Output::header("Base Image Assumptions");
    println!();

    // Show base image info
    if let Some(ref source) = assumptions.base_image.source {
        println!("  {} {}", "Base Image:".dimmed(), source);
    }
    if let Some(ref digest) = assumptions.base_image.last_verified_digest {
        println!(
            "  {} {}",
            "Digest:".dimmed(),
            &digest[..32.min(digest.len())]
        );
    }
    if let Some(ref at) = assumptions.base_image.last_verified_at {
        println!("  {} {}", "Verified:".dimmed(), at);
    }
    println!();

    println!("{}", "Packages:".yellow().bold());
    for pkg in &assumptions.packages {
        let reason = pkg.reason.as_deref().unwrap_or("");
        if reason.is_empty() {
            println!("  {}", pkg.name);
        } else {
            println!("  {} {}", pkg.name, format!("({})", reason).dimmed());
        }
    }

    println!();
    Output::info(format!(
        "Total: {} package assumptions",
        assumptions.packages.len()
    ));

    Ok(())
}

fn handle_info() -> Result<()> {
    let repo_root = get_repo_root()?;

    Output::header("Base Image Information");
    println!();

    // Read base image from Containerfile
    let containerfile = repo_root.join("Containerfile");
    if containerfile.exists() {
        let content = std::fs::read_to_string(&containerfile)?;
        for line in content.lines() {
            if line.trim().starts_with("FROM ") {
                let image = line.trim().strip_prefix("FROM ").unwrap_or("");
                println!("  {} {}", "Source:".dimmed(), image);
                break;
            }
        }
    }

    // Read stored digest
    let digest_file = repo_root.join(".base-image-digest");
    if digest_file.exists() {
        let digest = std::fs::read_to_string(&digest_file)?;
        println!("  {} {}", "Digest:".dimmed(), digest.trim());
    }

    // Get current rpm-ostree status
    let output = Command::new("rpm-ostree")
        .args(["status", "--json"])
        .output();

    if let Ok(output) = output
        && output.status.success()
        && let Ok(json) = serde_json::from_slice::<serde_json::Value>(&output.stdout)
        && let Some(deployments) = json.get("deployments").and_then(|d| d.as_array())
        && let Some(booted) = deployments
            .iter()
            .find(|d| d.get("booted").and_then(|b| b.as_bool()).unwrap_or(false))
    {
        if let Some(version) = booted.get("version").and_then(|v| v.as_str()) {
            println!("  {} {}", "Version:".dimmed(), version);
        }
        if let Some(origin) = booted.get("origin").and_then(|v| v.as_str()) {
            println!("  {} {}", "Origin:".dimmed(), origin);
        }
    }

    println!();
    Ok(())
}

fn handle_snapshot(filter: Option<String>, output: Option<PathBuf>, _yes: bool) -> Result<()> {
    Output::info("Collecting installed packages from base image...");

    // Get list of packages that came with the base image
    // On rpm-ostree systems, we can look at the base package list
    let pkg_output = Command::new("rpm")
        .args(["-qa", "--queryformat", "%{NAME}\n"])
        .output()
        .context("Failed to query RPM packages")?;

    if !pkg_output.status.success() {
        bail!("Failed to query RPM packages");
    }

    let packages: Vec<String> = String::from_utf8_lossy(&pkg_output.stdout)
        .lines()
        .filter(|p| {
            if let Some(ref f) = filter {
                p.contains(f.as_str())
            } else {
                true
            }
        })
        .map(String::from)
        .collect();

    Output::info(format!("Found {} packages", packages.len()));

    if let Some(out_path) = output {
        let mut assumptions = BaseImageAssumptions::default();
        for pkg in packages {
            assumptions.add_package(&pkg, None);
        }
        let content = serde_json::to_string_pretty(&assumptions)?;
        std::fs::write(&out_path, content)?;
        Output::success(format!("Wrote assumptions to {}", out_path.display()));
    } else {
        println!();
        for pkg in &packages {
            println!("{}", pkg);
        }
        println!();
        Output::info("Use --output to save to file, or pipe to a file.");
    }

    Ok(())
}

fn is_package_installed(package: &str) -> Result<bool> {
    let output = Command::new("rpm")
        .args(["-q", package])
        .output()
        .context("Failed to check if package is installed")?;

    Ok(output.status.success())
}
