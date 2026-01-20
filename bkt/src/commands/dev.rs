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

use crate::context::{CommandDomain, ExecutionContext, is_in_toolbox};
use crate::manifest::ToolboxPackagesManifest;
use crate::output::Output;
use crate::pipeline::ExecutionPlan;
use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use owo_colors::OwoColorize;
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
        Output::info(format!("Toolbox '{}' not found. Creating...", toolbox_name));
        create_toolbox(&toolbox_name)?;
    }

    // Enter toolbox (uses exec, doesn't return)
    Output::info(format!("Entering toolbox: {}", toolbox_name));
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

    Output::success(format!("Created toolbox: {}", name));
    Ok(())
}

fn enter_toolbox(name: &str) -> Result<()> {
    // Use exec to replace current process with toolbox shell
    use std::os::unix::process::CommandExt;

    let err = Command::new("toolbox").args(["enter", name]).exec();

    // exec() only returns on error
    bail!("Failed to enter toolbox: {}", err)
}

// =============================================================================
// Status Command
// =============================================================================

fn handle_status(plan: &ExecutionPlan) -> Result<()> {
    Output::header("=== Development Toolbox Status ===");

    // Check if we're in a toolbox
    let in_toolbox = is_in_toolbox();
    if in_toolbox {
        if let Ok(name) = std::env::var("TOOLBOX_PATH") {
            Output::kv("Toolbox", name);
        } else {
            Output::kv("Toolbox", "(in container)");
        }
    } else {
        Output::kv("Toolbox", "not in toolbox (running on host)");
    }
    Output::blank();

    // Load and display manifest
    let manifest = ToolboxPackagesManifest::load_user()?;

    if manifest.packages.is_empty() && manifest.groups.is_empty() {
        Output::info("No packages in toolbox manifest.");
        Output::hint("Add packages with: bkt dev dnf install <package>");
        return Ok(());
    }

    // Display packages with installed status
    if !manifest.packages.is_empty() {
        Output::subheader("PACKAGES:");
        println!("{:<40} STATUS", "NAME".cyan());
        Output::separator();

        for pkg in &manifest.packages {
            let installed = if is_package_installed(pkg) {
                format!("{} installed", "✓".green())
            } else {
                format!("{} not installed", "✗".red())
            };
            println!("{:<40} {}", pkg, installed);
        }
        Output::blank();
    }

    // Display groups
    if !manifest.groups.is_empty() {
        Output::subheader("GROUPS:");
        for group in &manifest.groups {
            Output::list_item(group);
        }
        Output::blank();
    }

    // Display COPR repos
    if !manifest.copr_repos.is_empty() {
        Output::subheader("COPR REPOSITORIES:");
        println!("{:<40} ENABLED", "NAME".cyan());
        Output::separator();
        for copr in &manifest.copr_repos {
            let status = if copr.enabled {
                "yes".green().to_string()
            } else {
                "no".red().to_string()
            };
            println!("{:<40} {}", copr.name, status);
        }
        Output::blank();
    }

    // Summary
    let missing: Vec<_> = manifest
        .packages
        .iter()
        .filter(|p| !is_package_installed(p))
        .collect();

    let installed_count = manifest.packages.len() - missing.len();
    if missing.is_empty() {
        Output::success(format!(
            "{} packages ({} installed)",
            manifest.packages.len(),
            installed_count
        ));
    } else {
        Output::warning(format!(
            "{} packages ({} installed, {} missing)",
            manifest.packages.len(),
            installed_count,
            missing.len()
        ));
        if !plan.dry_run {
            Output::hint("To install missing packages: bkt dev update");
        }
    }

    Ok(())
}

// =============================================================================
// Update Command (sync manifest to toolbox)
// =============================================================================

fn handle_update(rebuild: bool, plan: &ExecutionPlan) -> Result<()> {
    if rebuild {
        // Future: implement full rebuild from Containerfile
        Output::warning("Full rebuild not yet implemented.");
        Output::hint("Use 'bkt dev dnf sync' to install missing packages.");
        return Ok(());
    }

    // Load manifest and find missing packages
    let manifest = ToolboxPackagesManifest::load_user()?;

    if manifest.packages.is_empty() && manifest.groups.is_empty() {
        Output::info("Toolbox manifest is empty. Nothing to sync.");
        return Ok(());
    }

    let missing: Vec<String> = manifest
        .packages
        .iter()
        .filter(|p| !is_package_installed(p))
        .cloned()
        .collect();

    if missing.is_empty() {
        Output::success(format!(
            "All {} packages are already installed.",
            manifest.packages.len()
        ));
        return Ok(());
    }

    Output::info(format!(
        "Installing {} missing package(s): {}",
        missing.len(),
        missing.join(", ")
    ));

    if plan.dry_run {
        Output::dry_run(format!("Would run: dnf install -y {}", missing.join(" ")));
        return Ok(());
    }

    // Install missing packages via dnf
    let spinner = Output::spinner("Installing packages...");
    let status = Command::new("dnf")
        .arg("install")
        .arg("-y")
        .args(&missing)
        .status()
        .context("Failed to run dnf")?;

    if !status.success() {
        spinner.finish_error("Installation failed");
        bail!("dnf install failed");
    }

    spinner.finish_success(format!("Installed {} package(s)", missing.len()));
    Ok(())
}

// =============================================================================
// Diff Command
// =============================================================================

fn handle_diff(plan: &ExecutionPlan) -> Result<()> {
    let manifest = ToolboxPackagesManifest::load_user()?;

    if manifest.packages.is_empty() {
        Output::info("Toolbox manifest is empty.");
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

    Output::header("=== Toolbox Package Diff ===");

    if !installed.is_empty() {
        Output::subheader(format!("Installed ({}):", installed.len()));
        for pkg in &installed {
            println!("  {} {}", "✓".green(), pkg);
        }
        Output::blank();
    }

    if !missing.is_empty() {
        Output::subheader(format!("Missing ({}):", missing.len()));
        for pkg in &missing {
            println!("  {} {}", "✗".red(), pkg);
        }
        Output::blank();

        if !plan.dry_run {
            Output::hint("To install missing packages: bkt dev update");
        }
    } else {
        Output::success("All packages are installed");
    }

    Ok(())
}

// =============================================================================
// Helper Functions
// =============================================================================

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
    fn test_is_in_toolbox_returns() {
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
