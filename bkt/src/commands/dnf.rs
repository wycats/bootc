//! DNF/RPM package management command implementation.
//!
//! Provides a unified interface for managing RPM packages across contexts:
//! - Host: Uses rpm-ostree for atomic package layering
//! - Toolbox: Uses dnf for direct package installation
//!
//! Query commands (search, info, provides) pass through to dnf5 directly.

use crate::context::{CommandDomain, ExecutionContext};
use crate::manifest::{CoprRepo, SystemPackagesManifest};
use crate::output::Output;
use crate::pipeline::ExecutionPlan;
use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use owo_colors::OwoColorize;
use std::process::Command;

#[derive(Debug, Args)]
pub struct DnfArgs {
    #[command(subcommand)]
    pub action: DnfAction,
}

#[derive(Debug, Subcommand)]
pub enum DnfAction {
    /// Install packages
    Install {
        /// Package names to install
        packages: Vec<String>,
        /// Apply immediately without reboot (rpm-ostree --apply-live)
        #[arg(long)]
        now: bool,
    },
    /// Remove packages
    Remove {
        /// Package names to remove
        packages: Vec<String>,
    },
    /// List managed packages from manifest
    List {
        /// Output format (table, json)
        #[arg(short, long, default_value = "table")]
        format: String,
    },
    /// Search for packages (query-only, no manifest changes)
    Search {
        /// Search term
        query: String,
    },
    /// Show package info (query-only)
    Info {
        /// Package name
        package: String,
    },
    /// Find what package provides a file (query-only)
    Provides {
        /// File path or command name
        path: String,
    },
    /// Show difference between manifest and installed packages
    Diff,
    /// Sync: install packages from manifest
    Sync {
        /// Apply immediately without reboot
        #[arg(long)]
        now: bool,
    },
    /// Manage COPR repositories
    Copr {
        #[command(subcommand)]
        action: CoprAction,
    },
}

#[derive(Debug, Subcommand)]
pub enum CoprAction {
    /// Enable a COPR repository
    Enable {
        /// COPR name (e.g., atim/starship)
        name: String,
    },
    /// Disable a COPR repository
    Disable {
        /// COPR name
        name: String,
    },
    /// List COPR repositories in manifest
    List,
}

pub fn run(args: DnfArgs, plan: &ExecutionPlan) -> Result<()> {
    match args.action {
        DnfAction::Install { packages, now } => handle_install(packages, now, plan),
        DnfAction::Remove { packages } => handle_remove(packages, plan),
        DnfAction::List { format } => handle_list(format, plan),
        DnfAction::Search { query } => handle_search(query),
        DnfAction::Info { package } => handle_info(package),
        DnfAction::Provides { path } => handle_provides(path),
        DnfAction::Diff => handle_diff(plan),
        DnfAction::Sync { now } => handle_sync(now, plan),
        DnfAction::Copr { action } => handle_copr(action, plan),
    }
}

// =============================================================================
// Query Commands (pass-through to dnf5, no manifest changes)
// =============================================================================

fn handle_search(query: String) -> Result<()> {
    // Use dnf5 for queries (works on both host and toolbox)
    let status = Command::new("dnf5")
        .args(["search", &query])
        .status()
        .context("Failed to run dnf5 search")?;

    if !status.success() {
        bail!("dnf5 search failed");
    }
    Ok(())
}

fn handle_info(package: String) -> Result<()> {
    let status = Command::new("dnf5")
        .args(["info", &package])
        .status()
        .context("Failed to run dnf5 info")?;

    if !status.success() {
        bail!("dnf5 info failed");
    }
    Ok(())
}

fn handle_provides(path: String) -> Result<()> {
    let status = Command::new("dnf5")
        .args(["provides", &path])
        .status()
        .context("Failed to run dnf5 provides")?;

    if !status.success() {
        bail!("dnf5 provides failed");
    }
    Ok(())
}

// =============================================================================
// List Command (read manifest)
// =============================================================================

fn handle_list(format: String, _plan: &ExecutionPlan) -> Result<()> {
    let system = SystemPackagesManifest::load_system()?;
    let user = SystemPackagesManifest::load_user()?;
    let merged = SystemPackagesManifest::merged(&system, &user);

    if format == "json" {
        println!("{}", serde_json::to_string_pretty(&merged)?);
        return Ok(());
    }

    // Table format
    if merged.packages.is_empty() && merged.groups.is_empty() && merged.copr_repos.is_empty() {
        Output::info("No packages in manifest.");
        return Ok(());
    }

    // List packages
    if !merged.packages.is_empty() {
        Output::subheader("PACKAGES:");
        println!("{:<40} {}", "NAME".cyan(), "SOURCE");
        Output::separator();
        for pkg in &merged.packages {
            let source = if user.find_package(pkg) {
                "user".yellow().to_string()
            } else {
                "system".dimmed().to_string()
            };
            let installed = if is_package_installed(pkg) {
                "✓".green().to_string()
            } else {
                "✗".red().to_string()
            };
            println!("{:<40} {} {}", pkg, source, installed);
        }
        Output::blank();
    }

    // List groups
    if !merged.groups.is_empty() {
        Output::subheader("GROUPS:");
        for group in &merged.groups {
            Output::list_item(group);
        }
        Output::blank();
    }

    // List COPR repos
    if !merged.copr_repos.is_empty() {
        Output::subheader("COPR REPOSITORIES:");
        println!("{:<40} {} {}", "NAME".cyan(), "ENABLED", "GPG");
        Output::separator();
        for copr in &merged.copr_repos {
            let enabled = if copr.enabled {
                "yes".green().to_string()
            } else {
                "no".red().to_string()
            };
            let gpg = if copr.gpg_check {
                "yes".green().to_string()
            } else {
                "no".yellow().to_string()
            };
            println!("{:<40} {:<8} {}", copr.name, enabled, gpg);
        }
        Output::blank();
    }

    Output::success(format!(
        "{} packages, {} groups, {} COPR repos",
        merged.packages.len(),
        merged.groups.len(),
        merged.copr_repos.len()
    ));

    Ok(())
}

// =============================================================================
// Install Command
// =============================================================================

fn handle_install(packages: Vec<String>, now: bool, plan: &ExecutionPlan) -> Result<()> {
    // Validate context for mutating operations
    plan.validate_domain(CommandDomain::Dnf)?;

    if packages.is_empty() {
        bail!("No packages specified");
    }

    let system = SystemPackagesManifest::load_system()?;
    let mut user = SystemPackagesManifest::load_user()?;

    // Track which packages are new
    let mut new_packages = Vec::new();
    let mut already_in_manifest = Vec::new();

    for pkg in &packages {
        if system.find_package(pkg) || user.find_package(pkg) {
            already_in_manifest.push(pkg.clone());
        } else {
            new_packages.push(pkg.clone());
        }
    }

    // Report already-managed packages
    for pkg in &already_in_manifest {
        Output::info(format!("Already in manifest: {}", pkg));
    }

    if new_packages.is_empty() && already_in_manifest.len() == packages.len() {
        Output::success("All packages already in manifest.");
        return Ok(());
    }

    // Update local manifest
    if plan.should_update_local_manifest() {
        for pkg in &new_packages {
            user.add_package(pkg.clone());
        }
        user.save_user()?;
        Output::success(format!("Added {} package(s) to user manifest", new_packages.len()));
    } else if plan.dry_run {
        for pkg in &new_packages {
            Output::dry_run(format!("Would add to manifest: {}", pkg));
        }
    }

    // Execute installation
    if plan.should_execute_locally() {
        match plan.context {
            ExecutionContext::Host => {
                install_via_rpm_ostree(&packages, now)?;
            }
            ExecutionContext::Dev => {
                install_via_dnf(&packages)?;
            }
            ExecutionContext::Image => {
                // Image context means --pr-only, no local execution
            }
        }
    } else if plan.dry_run {
        match plan.context {
            ExecutionContext::Host => {
                let live = if now { " --apply-live" } else { "" };
                Output::dry_run(format!(
                    "Would run: rpm-ostree install{} {}",
                    live,
                    packages.join(" ")
                ));
            }
            ExecutionContext::Dev => {
                Output::dry_run(format!("Would run: dnf install -y {}", packages.join(" ")));
            }
            ExecutionContext::Image => {}
        }
    }

    // Create PR if needed
    if plan.should_create_pr() && !new_packages.is_empty() {
        let mut system_manifest = SystemPackagesManifest::load_system()?;
        for pkg in &new_packages {
            system_manifest.add_package(pkg.clone());
        }
        let manifest_content = serde_json::to_string_pretty(&system_manifest)?;

        plan.maybe_create_pr(
            "dnf",
            "install",
            &new_packages.join(", "),
            "system-packages.json",
            &manifest_content,
        )?;
    }

    Ok(())
}

fn install_via_rpm_ostree(packages: &[String], now: bool) -> Result<()> {
    let mut args = vec!["install"];

    if now {
        args.push("--apply-live");
    }

    for pkg in packages {
        args.push(pkg.as_str());
    }

    Output::running(format!("rpm-ostree {}", args.join(" ")));

    let status = Command::new("rpm-ostree")
        .args(&args)
        .status()
        .context("Failed to run rpm-ostree")?;

    if !status.success() {
        bail!("rpm-ostree install failed");
    }

    if now {
        Output::success("Packages installed and applied live");
    } else {
        Output::success("Packages staged for next boot (reboot required)");
    }

    Ok(())
}

fn install_via_dnf(packages: &[String]) -> Result<()> {
    let mut args = vec!["install", "-y"];

    for pkg in packages {
        args.push(pkg.as_str());
    }

    Output::running(format!("dnf {}", args.join(" ")));

    let status = Command::new("dnf")
        .args(&args)
        .status()
        .context("Failed to run dnf")?;

    if !status.success() {
        bail!("dnf install failed");
    }

    Output::success("Packages installed in toolbox");
    Ok(())
}

// =============================================================================
// Remove Command
// =============================================================================

fn handle_remove(packages: Vec<String>, plan: &ExecutionPlan) -> Result<()> {
    plan.validate_domain(CommandDomain::Dnf)?;

    if packages.is_empty() {
        bail!("No packages specified");
    }

    let system = SystemPackagesManifest::load_system()?;
    let mut user = SystemPackagesManifest::load_user()?;

    for pkg in &packages {
        let in_system = system.find_package(pkg);
        let in_user = user.find_package(pkg);

        if in_system && !in_user && !plan.should_create_pr() {
            Output::info(format!(
                "'{}' is only in the system manifest; run without --local to create a PR",
                pkg
            ));
        }

        if plan.should_update_local_manifest() {
            if user.remove_package(pkg) {
                Output::success(format!("Removed from user manifest: {}", pkg));
            } else if !in_system {
                Output::warning(format!("Package not found in manifest: {}", pkg));
            }
        } else if plan.dry_run {
            if in_user {
                Output::dry_run(format!("Would remove from user manifest: {}", pkg));
            } else if !in_system {
                Output::dry_run(format!("Package not found in manifest: {}", pkg));
            }
        }
    }

    if plan.should_update_local_manifest() {
        user.save_user()?;
    }

    // Execute removal
    if plan.should_execute_locally() {
        match plan.context {
            ExecutionContext::Host => {
                remove_via_rpm_ostree(&packages)?;
            }
            ExecutionContext::Dev => {
                remove_via_dnf(&packages)?;
            }
            ExecutionContext::Image => {}
        }
    } else if plan.dry_run {
        match plan.context {
            ExecutionContext::Host => {
                Output::dry_run(format!(
                    "Would run: rpm-ostree uninstall {}",
                    packages.join(" ")
                ));
            }
            ExecutionContext::Dev => {
                Output::dry_run(format!("Would run: dnf remove -y {}", packages.join(" ")));
            }
            ExecutionContext::Image => {}
        }
    }

    // Create PR if needed
    if plan.should_create_pr() {
        let mut system_manifest = SystemPackagesManifest::load_system()?;
        let mut removed_from_system = Vec::new();

        for pkg in &packages {
            if system_manifest.remove_package(pkg) {
                removed_from_system.push(pkg.clone());
            }
        }

        if !removed_from_system.is_empty() {
            let manifest_content = serde_json::to_string_pretty(&system_manifest)?;
            plan.maybe_create_pr(
                "dnf",
                "remove",
                &removed_from_system.join(", "),
                "system-packages.json",
                &manifest_content,
            )?;
        } else {
            Output::info("No packages to remove from system manifest, no PR needed");
        }
    }

    Ok(())
}

fn remove_via_rpm_ostree(packages: &[String]) -> Result<()> {
    let mut args = vec!["uninstall"];

    for pkg in packages {
        args.push(pkg.as_str());
    }

    Output::running(format!("rpm-ostree {}", args.join(" ")));

    let status = Command::new("rpm-ostree")
        .args(&args)
        .status()
        .context("Failed to run rpm-ostree")?;

    if !status.success() {
        bail!("rpm-ostree uninstall failed");
    }

    Output::success("Packages staged for removal (reboot required)");
    Ok(())
}

fn remove_via_dnf(packages: &[String]) -> Result<()> {
    let mut args = vec!["remove", "-y"];

    for pkg in packages {
        args.push(pkg.as_str());
    }

    Output::running(format!("dnf {}", args.join(" ")));

    let status = Command::new("dnf")
        .args(&args)
        .status()
        .context("Failed to run dnf")?;

    if !status.success() {
        bail!("dnf remove failed");
    }

    Output::success("Packages removed from toolbox");
    Ok(())
}

// =============================================================================
// Diff Command
// =============================================================================

fn handle_diff(_plan: &ExecutionPlan) -> Result<()> {
    let system = SystemPackagesManifest::load_system()?;
    let user = SystemPackagesManifest::load_user()?;
    let merged = SystemPackagesManifest::merged(&system, &user);

    let mut not_installed = Vec::new();
    let mut installed = Vec::new();

    for pkg in &merged.packages {
        if is_package_installed(pkg) {
            installed.push(pkg.clone());
        } else {
            not_installed.push(pkg.clone());
        }
    }

    Output::subheader(format!("Installed ({}):", installed.len()));
    for pkg in &installed {
        println!("  {} {}", "✓".green(), pkg);
    }

    Output::blank();
    Output::subheader(format!("Not installed ({}):", not_installed.len()));
    for pkg in &not_installed {
        println!("  {} {}", "✗".red(), pkg);
    }

    Output::blank();
    if not_installed.is_empty() {
        Output::success("All manifest packages are installed.");
    } else {
        Output::hint(format!(
            "Run 'bkt dnf sync' to install {} missing package(s).",
            not_installed.len()
        ));
    }

    Ok(())
}

// =============================================================================
// Sync Command
// =============================================================================

fn handle_sync(now: bool, plan: &ExecutionPlan) -> Result<()> {
    plan.validate_domain(CommandDomain::Dnf)?;

    let system = SystemPackagesManifest::load_system()?;
    let user = SystemPackagesManifest::load_user()?;
    let merged = SystemPackagesManifest::merged(&system, &user);

    let mut to_install = Vec::new();

    for pkg in &merged.packages {
        if !is_package_installed(pkg) {
            to_install.push(pkg.clone());
        }
    }

    if to_install.is_empty() {
        Output::success("All manifest packages are already installed.");
        return Ok(());
    }

    Output::info(format!("{} package(s) to install:", to_install.len()));
    for pkg in &to_install {
        Output::list_item(pkg);
    }

    if plan.should_execute_locally() {
        match plan.context {
            ExecutionContext::Host => {
                install_via_rpm_ostree(&to_install, now)?;
            }
            ExecutionContext::Dev => {
                install_via_dnf(&to_install)?;
            }
            ExecutionContext::Image => {}
        }
    } else if plan.dry_run {
        Output::dry_run(format!("Would install {} packages", to_install.len()));
    }

    Ok(())
}

// =============================================================================
// COPR Commands
// =============================================================================

pub fn handle_copr(action: CoprAction, plan: &ExecutionPlan) -> Result<()> {
    match action {
        CoprAction::Enable { name } => handle_copr_enable(name, plan),
        CoprAction::Disable { name } => handle_copr_disable(name, plan),
        CoprAction::List => handle_copr_list(),
    }
}

fn handle_copr_enable(name: String, plan: &ExecutionPlan) -> Result<()> {
    plan.validate_domain(CommandDomain::Dnf)?;

    let system = SystemPackagesManifest::load_system()?;
    let mut user = SystemPackagesManifest::load_user()?;

    // Check if already enabled
    if system
        .find_copr(&name)
        .or_else(|| user.find_copr(&name))
        .is_some_and(|c| c.enabled)
    {
        Output::info(format!("COPR already enabled: {}", name));
        return Ok(());
    }

    // Update manifest
    if plan.should_update_local_manifest() {
        user.upsert_copr(CoprRepo::new(name.clone()));
        user.save_user()?;
        Output::success(format!("Added to user manifest: {}", name));
    } else if plan.dry_run {
        Output::dry_run(format!("Would add COPR to manifest: {}", name));
    }

    // Execute on host
    if plan.should_execute_locally() && plan.context == ExecutionContext::Host {
        Output::running(format!("dnf copr enable -y {}", name));
        let status = Command::new("dnf")
            .args(["copr", "enable", "-y", &name])
            .status()
            .context("Failed to enable COPR")?;

        if !status.success() {
            bail!("Failed to enable COPR: {}", name);
        }
        Output::success(format!("COPR enabled: {}", name));
    } else if plan.dry_run && plan.context == ExecutionContext::Host {
        Output::dry_run(format!("Would run: dnf copr enable -y {}", name));
    }

    // Create PR if needed
    if plan.should_create_pr() {
        let mut system_manifest = SystemPackagesManifest::load_system()?;
        system_manifest.upsert_copr(CoprRepo::new(name.clone()));
        let manifest_content = serde_json::to_string_pretty(&system_manifest)?;

        plan.maybe_create_pr(
            "copr",
            "enable",
            &name,
            "system-packages.json",
            &manifest_content,
        )?;
    }

    Ok(())
}

fn handle_copr_disable(name: String, plan: &ExecutionPlan) -> Result<()> {
    plan.validate_domain(CommandDomain::Dnf)?;

    let system = SystemPackagesManifest::load_system()?;
    let mut user = SystemPackagesManifest::load_user()?;

    let in_system = system.find_copr(&name).is_some();
    let in_user = user.find_copr(&name).is_some();

    if !in_system && !in_user {
        Output::warning(format!("COPR not found in manifest: {}", name));
        return Ok(());
    }

    if in_system && !in_user && !plan.should_create_pr() {
        Output::info(format!(
            "'{}' is only in the system manifest; run without --local to create a PR",
            name
        ));
    }

    // Update manifest
    if plan.should_update_local_manifest() && user.remove_copr(&name) {
        user.save_user()?;
        Output::success(format!("Removed from user manifest: {}", name));
    } else if plan.dry_run && in_user {
        Output::dry_run(format!("Would remove COPR from manifest: {}", name));
    }

    // Execute on host
    if plan.should_execute_locally() && plan.context == ExecutionContext::Host {
        Output::running(format!("dnf copr disable {}", name));
        let status = Command::new("dnf")
            .args(["copr", "disable", &name])
            .status()
            .context("Failed to disable COPR")?;

        if !status.success() {
            bail!("Failed to disable COPR: {}", name);
        }
        Output::success(format!("COPR disabled: {}", name));
    }

    // Create PR if needed
    if plan.should_create_pr() && in_system {
        let mut system_manifest = SystemPackagesManifest::load_system()?;
        if system_manifest.remove_copr(&name) {
            let manifest_content = serde_json::to_string_pretty(&system_manifest)?;
            plan.maybe_create_pr(
                "copr",
                "disable",
                &name,
                "system-packages.json",
                &manifest_content,
            )?;
        }
    }

    Ok(())
}

fn handle_copr_list() -> Result<()> {
    let system = SystemPackagesManifest::load_system()?;
    let user = SystemPackagesManifest::load_user()?;
    let merged = SystemPackagesManifest::merged(&system, &user);

    if merged.copr_repos.is_empty() {
        Output::info("No COPR repositories in manifest.");
        return Ok(());
    }

    Output::subheader("COPR REPOSITORIES:");
    println!("{:<40} {:<8} {:<8} {}", "NAME".cyan(), "ENABLED", "GPG", "SOURCE");
    Output::separator();

    for copr in &merged.copr_repos {
        let source = if user.find_copr(&copr.name).is_some() {
            "user".yellow().to_string()
        } else {
            "system".dimmed().to_string()
        };
        let enabled = if copr.enabled {
            "yes".green().to_string()
        } else {
            "no".red().to_string()
        };
        let gpg = if copr.gpg_check {
            "yes".green().to_string()
        } else {
            "no".yellow().to_string()
        };
        println!("{:<40} {:<8} {:<8} {}", copr.name, enabled, gpg, source);
    }

    Ok(())
}

// =============================================================================
// Helper Functions
// =============================================================================

/// Check if a package is installed on the system.
fn is_package_installed(pkg: &str) -> bool {
    Command::new("rpm")
        .args(["-q", pkg])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}
