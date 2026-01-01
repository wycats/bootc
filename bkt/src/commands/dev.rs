//! Development toolbox command implementation.
//!
//! `bkt dev` is a command prefix that forces ExecutionContext::Dev
//! and provides toolbox-specific subcommands. It's syntactic sugar for
//! `bkt --context dev` with some additional toolbox-specific commands.
//!
//! # Examples
//!
//! ```bash
//! # Install a package in the toolbox
//! bkt dev dnf install gcc
//!
//! # Enter the development toolbox
//! bkt dev enter
//!
//! # Check what packages are missing
//! bkt dev diff
//!
//! # Sync toolbox to manifest (install missing packages)
//! bkt dev update
//! ```

use crate::context::{CommandDomain, ExecutionContext};
use crate::manifest::ToolboxPackagesManifest;
use crate::pipeline::ExecutionPlan;
use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use std::process::Command;

#[derive(Debug, Args)]
pub struct DevArgs {
    #[command(subcommand)]
    pub action: DevAction,
}

#[derive(Debug, Subcommand)]
pub enum DevAction {
    /// Manage DNF packages in toolbox
    Dnf(crate::commands::dnf::DnfArgs),

    /// Enter the development toolbox
    Enter {
        /// Toolbox name (default: bootc-dev)
        #[arg(short, long)]
        name: Option<String>,
    },

    /// Show status of toolbox packages
    Status,

    /// Sync toolbox to manifest (install missing packages)
    Update {
        /// Force rebuild from Containerfile (not yet implemented)
        #[arg(long)]
        rebuild: bool,
    },

    /// Show difference between manifest and installed packages
    Diff,

    /// Manage COPR repositories in toolbox
    Copr {
        #[command(subcommand)]
        action: crate::commands::dnf::CoprAction,
    },
}

/// Run the dev command.
///
/// Forces ExecutionContext::Dev for all subcommands.
pub fn run(args: DevArgs, base_plan: &ExecutionPlan) -> Result<()> {
    // Force Dev context for all dev commands
    let mut plan = base_plan.clone();
    plan.context = ExecutionContext::Dev;

    match args.action {
        DevAction::Dnf(dnf_args) => {
            // Re-validate with forced Dev context
            plan.validate_domain(CommandDomain::Dnf)?;
            crate::commands::dnf::run(dnf_args, &plan)
        }
        DevAction::Enter { name } => handle_enter(name),
        DevAction::Status => handle_status(&plan),
        DevAction::Update { rebuild } => handle_update(rebuild, &plan),
        DevAction::Diff => handle_diff(&plan),
        DevAction::Copr { action } => {
            plan.validate_domain(CommandDomain::Dnf)?;
            crate::commands::dnf::handle_copr(action, &plan)
        }
    }
}

// =============================================================================
// Enter Command
// =============================================================================

fn handle_enter(name: Option<String>) -> Result<()> {
    let toolbox_name = name.unwrap_or_else(|| "bootc-dev".to_string());

    // Check if toolbox exists
    let exists = check_toolbox_exists(&toolbox_name)?;

    if !exists {
        println!("Toolbox '{}' not found. Creating...", toolbox_name);
        create_toolbox(&toolbox_name)?;
    }

    // Enter toolbox (uses exec, doesn't return)
    println!("Entering toolbox: {}", toolbox_name);
    enter_toolbox(&toolbox_name)
}

fn check_toolbox_exists(name: &str) -> Result<bool> {
    let output = Command::new("toolbox")
        .args(["list", "--containers"])
        .output()
        .context("Failed to run `toolbox list`")?;

    if !output.status.success() {
        bail!("toolbox list failed");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout.lines().any(|line| line.contains(name)))
}

fn create_toolbox(name: &str) -> Result<()> {
    // TODO: In the future, we can check for a custom Containerfile
    // and use `toolbox create --image <custom-image>` instead

    let status = Command::new("toolbox")
        .args(["create", name])
        .status()
        .context("Failed to create toolbox")?;

    if !status.success() {
        bail!("Failed to create toolbox '{}'", name);
    }

    println!("✓ Created toolbox: {}", name);
    Ok(())
}

fn enter_toolbox(name: &str) -> Result<()> {
    // Use exec to replace current process with toolbox shell
    use std::os::unix::process::CommandExt;

    let err = Command::new("toolbox")
        .args(["enter", name])
        .exec();

    // exec() only returns on error
    bail!("Failed to enter toolbox: {}", err)
}

// =============================================================================
// Status Command
// =============================================================================

fn handle_status(plan: &ExecutionPlan) -> Result<()> {
    println!("=== Development Toolbox Status ===\n");

    // Check if we're in a toolbox
    let in_toolbox = is_in_toolbox();
    if in_toolbox {
        if let Ok(name) = std::env::var("TOOLBOX_PATH") {
            println!("Currently in toolbox: {}", name);
        } else {
            println!("Currently in a toolbox container");
        }
    } else {
        println!("Not in a toolbox (running on host)");
    }
    println!();

    // Load and display manifest
    let manifest = ToolboxPackagesManifest::load_user()?;

    if manifest.packages.is_empty() && manifest.groups.is_empty() {
        println!("No packages in toolbox manifest.");
        println!("\nAdd packages with: bkt dev dnf install <package>");
        return Ok(());
    }

    // Display packages with installed status
    if !manifest.packages.is_empty() {
        println!("PACKAGES:");
        println!("{:<40} STATUS", "NAME");
        println!("{}", "-".repeat(50));

        for pkg in &manifest.packages {
            let installed = if is_package_installed(pkg) {
                "✓ installed"
            } else {
                "✗ not installed"
            };
            println!("{:<40} {}", pkg, installed);
        }
        println!();
    }

    // Display groups
    if !manifest.groups.is_empty() {
        println!("GROUPS:");
        for group in &manifest.groups {
            println!("  {}", group);
        }
        println!();
    }

    // Display COPR repos
    if !manifest.copr_repos.is_empty() {
        println!("COPR REPOSITORIES:");
        println!("{:<40} ENABLED", "NAME");
        println!("{}", "-".repeat(50));
        for copr in &manifest.copr_repos {
            let status = if copr.enabled { "yes" } else { "no" };
            println!("{:<40} {}", copr.name, status);
        }
        println!();
    }

    // Summary
    let missing: Vec<_> = manifest
        .packages
        .iter()
        .filter(|p| !is_package_installed(p))
        .collect();

    println!(
        "{} packages ({} installed, {} missing)",
        manifest.packages.len(),
        manifest.packages.len() - missing.len(),
        missing.len()
    );

    if !missing.is_empty() && !plan.dry_run {
        println!("\nTo install missing packages: bkt dev update");
    }

    Ok(())
}

// =============================================================================
// Update Command (sync manifest to toolbox)
// =============================================================================

fn handle_update(rebuild: bool, plan: &ExecutionPlan) -> Result<()> {
    if rebuild {
        // Future: implement full rebuild from Containerfile
        println!("Full rebuild not yet implemented.");
        println!("Use 'bkt dev dnf sync' to install missing packages.");
        return Ok(());
    }

    // Load manifest and find missing packages
    let manifest = ToolboxPackagesManifest::load_user()?;

    if manifest.packages.is_empty() && manifest.groups.is_empty() {
        println!("Toolbox manifest is empty. Nothing to sync.");
        return Ok(());
    }

    let missing: Vec<String> = manifest
        .packages
        .iter()
        .filter(|p| !is_package_installed(p))
        .cloned()
        .collect();

    if missing.is_empty() {
        println!("✓ All {} packages are already installed.", manifest.packages.len());
        return Ok(());
    }

    println!(
        "Installing {} missing package(s): {}",
        missing.len(),
        missing.join(", ")
    );

    if plan.dry_run {
        println!("[dry-run] Would run: dnf install -y {}", missing.join(" "));
        return Ok(());
    }

    // Install missing packages via dnf
    let status = Command::new("dnf")
        .arg("install")
        .arg("-y")
        .args(&missing)
        .status()
        .context("Failed to run dnf")?;

    if !status.success() {
        bail!("dnf install failed");
    }

    println!("✓ Installed {} package(s)", missing.len());
    Ok(())
}

// =============================================================================
// Diff Command
// =============================================================================

fn handle_diff(plan: &ExecutionPlan) -> Result<()> {
    let manifest = ToolboxPackagesManifest::load_user()?;

    if manifest.packages.is_empty() {
        println!("Toolbox manifest is empty.");
        return Ok(());
    }

    let mut missing = Vec::new();
    let mut installed = Vec::new();

    for pkg in &manifest.packages {
        if is_package_installed(pkg) {
            installed.push(pkg);
        } else {
            missing.push(pkg);
        }
    }

    println!("=== Toolbox Package Diff ===\n");

    if !installed.is_empty() {
        println!("Installed ({}):", installed.len());
        for pkg in &installed {
            println!("  ✓ {}", pkg);
        }
        println!();
    }

    if !missing.is_empty() {
        println!("Missing ({}):", missing.len());
        for pkg in &missing {
            println!("  ✗ {}", pkg);
        }
        println!();

        if !plan.dry_run {
            println!("To install missing packages: bkt dev update");
        }
    } else {
        println!("All packages are installed ✓");
    }

    Ok(())
}

// =============================================================================
// Helper Functions
// =============================================================================

/// Check if we're running inside a toolbox container.
fn is_in_toolbox() -> bool {
    // Toolbox sets TOOLBOX_PATH environment variable
    std::env::var("TOOLBOX_PATH").is_ok() ||
    // Also check for container file that toolbox creates
    std::path::Path::new("/run/.containerenv").exists()
}

/// Check if a package is installed (uses rpm).
fn is_package_installed(package: &str) -> bool {
    Command::new("rpm")
        .args(["-q", package])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_in_toolbox_false_by_default() {
        // In test environment, we're typically not in a toolbox
        // This is just a sanity check that the function doesn't crash
        let _ = is_in_toolbox();
    }

    #[test]
    fn test_is_package_installed() {
        // bash should be installed on any Linux system
        // This might fail in minimal containers
        #[cfg(target_os = "linux")]
        {
            // Don't assert, just make sure it doesn't crash
            let _ = is_package_installed("bash");
        }
    }
}
