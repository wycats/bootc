//! Development toolbox command implementation.
//!
//! `bkt dev` manages packages in the development toolbox. This is an immediate
//! operation: packages are installed now via dnf, then recorded in the manifest
//! for future toolbox setups.
//!
//! # Verb Semantics
//!
//! - `install` — Install now (dnf) + record in manifest
//! - `remove` — Remove now + update manifest
//! - `list` — Show what's in the manifest
//! - `sync` — Install all packages from manifest
//! - `capture` — Capture installed packages to manifest
//!
//! # Examples
//!
//! ```bash
//! # Install a package (runs dnf + updates manifest)
//! bkt dev install gcc
//!
//! # Install without running dnf (just update manifest)
//! bkt dev install --manifest-only gcc
//!
//! # Sync toolbox to manifest
//! bkt dev sync
//!
//! # Enter the development toolbox
//! bkt dev enter
//! ```

use crate::context::is_in_toolbox;
use crate::manifest::{CoprRepo, ToolboxPackagesManifest};
use crate::output::Output;
use crate::pipeline::ExecutionPlan;
use crate::plan::{
    ExecuteContext, ExecutionReport, Operation, Plan, PlanContext, PlanSummary, Plannable, Verb,
};
use crate::validation::validate_dnf_package;
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
    /// Install packages in the toolbox
    ///
    /// Runs `dnf install` in the toolbox and updates the manifest.
    Install {
        /// Package names to install
        packages: Vec<String>,
        /// Only update manifest, skip dnf execution
        #[arg(long)]
        manifest_only: bool,
        /// Skip package validation
        #[arg(long)]
        force: bool,
    },
    /// Remove packages from the toolbox
    Remove {
        /// Package names to remove
        packages: Vec<String>,
        /// Only update manifest, skip dnf execution
        #[arg(long)]
        manifest_only: bool,
    },
    /// List managed packages from manifest
    List {
        /// Output format (table, json)
        #[arg(short, long, default_value = "table")]
        format: String,
    },
    /// Sync: install all packages from manifest
    Sync,
    /// Capture installed packages not in manifest
    Capture {
        /// Apply immediately (add packages to manifest)
        #[arg(long)]
        apply: bool,
    },
    /// Manage COPR repositories in toolbox
    Copr {
        #[command(subcommand)]
        action: CoprAction,
    },
    /// Enter the development toolbox
    Enter {
        /// Toolbox name (default: bootc-dev)
        #[arg(short, long)]
        name: Option<String>,
    },
    /// Show status of toolbox packages
    Status,
    /// Show difference between manifest and installed packages
    Diff,
}

#[derive(Debug, Subcommand)]
pub enum CoprAction {
    /// Enable a COPR repository in the toolbox
    Enable {
        /// COPR name (e.g., atim/starship)
        name: String,
        /// Only update manifest, skip dnf execution
        #[arg(long)]
        manifest_only: bool,
    },
    /// Disable a COPR repository
    Disable {
        /// COPR name
        name: String,
        /// Only update manifest, skip dnf execution
        #[arg(long)]
        manifest_only: bool,
    },
    /// List COPR repositories in manifest
    List,
}

/// Run the dev command.
pub fn run(args: DevArgs, plan: &ExecutionPlan) -> Result<()> {
    match args.action {
        DevAction::Install {
            packages,
            manifest_only,
            force,
        } => handle_install(packages, manifest_only, force, plan),
        DevAction::Remove {
            packages,
            manifest_only,
        } => handle_remove(packages, manifest_only, plan),
        DevAction::List { format } => handle_list(format),
        DevAction::Sync => handle_sync(plan),
        DevAction::Capture { apply } => handle_capture(apply, plan),
        DevAction::Copr { action } => handle_copr(action, plan),
        DevAction::Enter { name } => handle_enter(name),
        DevAction::Status => handle_status(plan),
        DevAction::Diff => handle_diff(plan),
    }
}

// =============================================================================
// Install Command
// =============================================================================

fn handle_install(
    packages: Vec<String>,
    manifest_only: bool,
    force: bool,
    plan: &ExecutionPlan,
) -> Result<()> {
    if packages.is_empty() {
        bail!("No packages specified");
    }

    // Validate that packages exist in repositories
    if !force {
        for pkg in &packages {
            validate_dnf_package(pkg)?;
        }
    }

    let mut manifest = ToolboxPackagesManifest::load_user()?;

    let mut new_packages = Vec::new();
    let mut already_in_manifest = Vec::new();

    for pkg in &packages {
        if manifest.find_package(pkg) {
            already_in_manifest.push(pkg.clone());
        } else {
            new_packages.push(pkg.clone());
        }
    }

    for pkg in &already_in_manifest {
        Output::info(format!("Already in manifest: {}", pkg));
    }

    if new_packages.is_empty() && already_in_manifest.len() == packages.len() {
        Output::success("All packages already in manifest.");
        return Ok(());
    }

    // Update manifest
    if !plan.dry_run {
        for pkg in &new_packages {
            manifest.add_package(pkg.clone());
        }
        manifest.save_user()?;
        Output::success(format!(
            "Added {} package(s) to toolbox manifest",
            new_packages.len()
        ));
    } else {
        for pkg in &new_packages {
            Output::dry_run(format!("Would add to manifest: {}", pkg));
        }
    }

    // Execute dnf if not manifest-only
    if !manifest_only && !plan.dry_run {
        install_via_dnf(&packages)?;
    } else if !manifest_only && plan.dry_run {
        Output::dry_run(format!("Would run: dnf install -y {}", packages.join(" ")));
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

fn handle_remove(packages: Vec<String>, manifest_only: bool, plan: &ExecutionPlan) -> Result<()> {
    if packages.is_empty() {
        bail!("No packages specified");
    }

    let mut manifest = ToolboxPackagesManifest::load_user()?;

    for pkg in &packages {
        if !plan.dry_run {
            if manifest.remove_package(pkg) {
                Output::success(format!("Removed from manifest: {}", pkg));
            } else {
                Output::warning(format!("Package not found in manifest: {}", pkg));
            }
        } else if manifest.find_package(pkg) {
            Output::dry_run(format!("Would remove from manifest: {}", pkg));
        } else {
            Output::dry_run(format!("Package not found in manifest: {}", pkg));
        }
    }

    if !plan.dry_run {
        manifest.save_user()?;
    }

    // Execute dnf if not manifest-only
    if !manifest_only && !plan.dry_run {
        remove_via_dnf(&packages)?;
    } else if !manifest_only && plan.dry_run {
        Output::dry_run(format!("Would run: dnf remove -y {}", packages.join(" ")));
    }

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
// List Command
// =============================================================================

fn handle_list(format: String) -> Result<()> {
    let manifest = ToolboxPackagesManifest::load_user()?;

    if format == "json" {
        println!("{}", serde_json::to_string_pretty(&manifest)?);
        return Ok(());
    }

    if manifest.packages.is_empty() && manifest.groups.is_empty() && manifest.copr_repos.is_empty()
    {
        Output::info("No packages in manifest.");
        return Ok(());
    }

    if !manifest.packages.is_empty() {
        Output::subheader("PACKAGES:");
        println!("{:<40} STATUS", "NAME".cyan());
        Output::separator();
        for pkg in &manifest.packages {
            let installed = if is_package_installed(pkg) {
                "✓".green().to_string()
            } else {
                "✗".red().to_string()
            };
            println!("{:<40} {}", pkg, installed);
        }
        Output::blank();
    }

    if !manifest.groups.is_empty() {
        Output::subheader("GROUPS:");
        for group in &manifest.groups {
            Output::list_item(group);
        }
        Output::blank();
    }

    if !manifest.copr_repos.is_empty() {
        Output::subheader("COPR REPOSITORIES:");
        println!("{:<40} ENABLED GPG", "NAME".cyan());
        Output::separator();
        for copr in &manifest.copr_repos {
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
        manifest.packages.len(),
        manifest.groups.len(),
        manifest.copr_repos.len()
    ));

    Ok(())
}

// =============================================================================
// Sync Command
// =============================================================================

fn handle_sync(plan: &ExecutionPlan) -> Result<()> {
    let plan_ctx = PlanContext::new(std::env::current_dir().unwrap_or_default(), plan.clone());

    let sync_plan = DevSyncCommand.plan(&plan_ctx)?;

    if sync_plan.is_empty() {
        Output::success("All manifest packages are already installed.");
        return Ok(());
    }

    // Always show the plan
    print!("{}", sync_plan.describe());

    if plan.dry_run {
        return Ok(());
    }

    // Execute the plan
    let mut exec_ctx = ExecuteContext::new(plan.clone());
    let report = sync_plan.execute(&mut exec_ctx)?;
    print!("{}", report);

    Ok(())
}

// =============================================================================
// Capture Command
// =============================================================================

fn handle_capture(apply: bool, plan: &ExecutionPlan) -> Result<()> {
    let plan_ctx = PlanContext::new(std::env::current_dir().unwrap_or_default(), plan.clone());

    let capture_plan = DevCaptureCommand.plan(&plan_ctx)?;

    if capture_plan.is_empty() {
        Output::success("All installed packages are already in the manifest.");
        return Ok(());
    }

    // Always show the plan
    print!("{}", capture_plan.describe());

    if plan.dry_run || !apply {
        if !apply {
            Output::hint("Use --apply to execute this plan.");
        }
        return Ok(());
    }

    // Execute the plan
    let mut exec_ctx = ExecuteContext::new(plan.clone());
    let report = capture_plan.execute(&mut exec_ctx)?;
    print!("{}", report);

    Ok(())
}

// =============================================================================
// COPR Commands
// =============================================================================

fn handle_copr(action: CoprAction, plan: &ExecutionPlan) -> Result<()> {
    match action {
        CoprAction::Enable {
            name,
            manifest_only,
        } => handle_copr_enable(name, manifest_only, plan),
        CoprAction::Disable {
            name,
            manifest_only,
        } => handle_copr_disable(name, manifest_only, plan),
        CoprAction::List => handle_copr_list(),
    }
}

fn handle_copr_enable(name: String, manifest_only: bool, plan: &ExecutionPlan) -> Result<()> {
    let mut manifest = ToolboxPackagesManifest::load_user()?;

    if manifest
        .copr_repos
        .iter()
        .find(|c| c.name == name)
        .is_some_and(|c| c.enabled)
    {
        Output::info(format!("COPR already enabled: {}", name));
        return Ok(());
    }

    if !plan.dry_run {
        let mut system = manifest.as_system_manifest();
        system.upsert_copr(CoprRepo::new(name.clone()));
        manifest.update_from(&system);
        manifest.save_user()?;
        Output::success(format!("Added to manifest: {}", name));
    } else {
        Output::dry_run(format!("Would add COPR to manifest: {}", name));
    }

    if !manifest_only && !plan.dry_run {
        Output::running(format!("dnf copr enable -y {}", name));
        let status = Command::new("dnf")
            .args(["copr", "enable", "-y", &name])
            .status()
            .context("Failed to enable COPR")?;

        if !status.success() {
            bail!("Failed to enable COPR: {}", name);
        }

        Output::success(format!("COPR enabled: {}", name));
    } else if !manifest_only && plan.dry_run {
        Output::dry_run(format!("Would run: dnf copr enable -y {}", name));
    }

    Ok(())
}

fn handle_copr_disable(name: String, manifest_only: bool, plan: &ExecutionPlan) -> Result<()> {
    let mut manifest = ToolboxPackagesManifest::load_user()?;

    let in_manifest = manifest.copr_repos.iter().any(|c| c.name == name);
    if !in_manifest {
        Output::warning(format!("COPR not found in manifest: {}", name));
        return Ok(());
    }

    if !plan.dry_run {
        let mut system = manifest.as_system_manifest();
        if system.remove_copr(&name) {
            manifest.update_from(&system);
            manifest.save_user()?;
            Output::success(format!("Removed from manifest: {}", name));
        }
    } else {
        Output::dry_run(format!("Would remove COPR from manifest: {}", name));
    }

    if !manifest_only && !plan.dry_run {
        Output::running(format!("dnf copr disable {}", name));
        let status = Command::new("dnf")
            .args(["copr", "disable", &name])
            .status()
            .context("Failed to disable COPR")?;

        if !status.success() {
            bail!("Failed to disable COPR: {}", name);
        }
        Output::success(format!("COPR disabled: {}", name));
    } else if !manifest_only && plan.dry_run {
        Output::dry_run(format!("Would run: dnf copr disable {}", name));
    }

    Ok(())
}

fn handle_copr_list() -> Result<()> {
    let manifest = ToolboxPackagesManifest::load_user()?;

    if manifest.copr_repos.is_empty() {
        Output::info("No COPR repositories in manifest.");
        return Ok(());
    }

    Output::subheader("COPR REPOSITORIES:");
    println!("{:<40} {:<8} {:<8}", "NAME".cyan(), "ENABLED", "GPG");
    Output::separator();

    for copr in &manifest.copr_repos {
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

    Ok(())
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
        Output::hint("Add packages with: bkt dev install <package>");
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
            Output::hint("To install missing packages: bkt dev sync");
        }
    }

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
            Output::hint("To install missing packages: bkt dev sync");
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

// ============================================================================
// Plan-based Dev Sync Implementation
// ============================================================================

/// Command to sync toolbox packages from manifest.
pub struct DevSyncCommand;

/// Plan for syncing toolbox packages.
pub struct DevSyncPlan {
    /// Packages to install.
    pub to_install: Vec<String>,
    /// Packages already installed.
    pub already_installed: usize,
}

impl Plannable for DevSyncCommand {
    type Plan = DevSyncPlan;

    fn plan(&self, _ctx: &PlanContext) -> Result<Self::Plan> {
        let manifest = ToolboxPackagesManifest::load_user()?;

        let mut to_install = Vec::new();
        let mut already_installed = 0;

        for pkg in manifest.packages {
            if is_package_installed(&pkg) {
                already_installed += 1;
            } else {
                to_install.push(pkg);
            }
        }

        Ok(DevSyncPlan {
            to_install,
            already_installed,
        })
    }
}

impl Plan for DevSyncPlan {
    fn describe(&self) -> PlanSummary {
        let mut summary = PlanSummary::new(format!(
            "Dev Sync: {} to install, {} already installed (via dnf)",
            self.to_install.len(),
            self.already_installed,
        ));

        for pkg in &self.to_install {
            summary.add_operation(Operation::new(Verb::Install, format!("package:{}", pkg)));
        }

        summary
    }

    fn execute(self, ctx: &mut ExecuteContext) -> Result<ExecutionReport> {
        let mut report = ExecutionReport::new();

        if self.to_install.is_empty() {
            return Ok(report);
        }

        // Install all packages in one batch for efficiency
        let result = install_via_dnf(&self.to_install);

        match result {
            Ok(()) => {
                for pkg in self.to_install {
                    report.record_success_and_notify(
                        ctx,
                        Verb::Install,
                        format!("package:{}", pkg),
                    );
                }
            }
            Err(e) => {
                for pkg in self.to_install {
                    report.record_failure_and_notify(
                        ctx,
                        Verb::Install,
                        format!("package:{}", pkg),
                        e.to_string(),
                    );
                }
            }
        }

        Ok(report)
    }

    fn is_empty(&self) -> bool {
        self.to_install.is_empty()
    }
}

// ============================================================================
// Plan-based Dev Capture Implementation
// ============================================================================

/// Command to capture toolbox packages not in manifest.
pub struct DevCaptureCommand;

/// Plan for capturing toolbox packages.
pub struct DevCapturePlan {
    /// Packages to add to manifest.
    pub to_capture: Vec<String>,
    /// Packages already in manifest.
    pub already_in_manifest: usize,
}

impl Plannable for DevCaptureCommand {
    type Plan = DevCapturePlan;

    fn plan(&self, _ctx: &PlanContext) -> Result<Self::Plan> {
        // Get explicitly installed packages (not auto-installed dependencies)
        let installed = get_user_installed_packages();

        // Load manifest to check what's already tracked
        let manifest = ToolboxPackagesManifest::load_user()?;

        let mut to_capture = Vec::new();
        let mut already_in_manifest = 0;

        for pkg in installed {
            if manifest.packages.contains(&pkg) {
                already_in_manifest += 1;
            } else {
                to_capture.push(pkg);
            }
        }

        // Sort for consistent output
        to_capture.sort();

        Ok(DevCapturePlan {
            to_capture,
            already_in_manifest,
        })
    }
}

impl Plan for DevCapturePlan {
    fn describe(&self) -> PlanSummary {
        let mut summary = PlanSummary::new(format!(
            "Dev Capture: {} to add, {} already in manifest",
            self.to_capture.len(),
            self.already_in_manifest
        ));

        for pkg in &self.to_capture {
            summary.add_operation(Operation::new(Verb::Capture, format!("package:{}", pkg)));
        }

        summary
    }

    fn execute(self, _ctx: &mut ExecuteContext) -> Result<ExecutionReport> {
        let mut report = ExecutionReport::new();

        if self.to_capture.is_empty() {
            return Ok(report);
        }

        // Load manifest and add packages
        let mut manifest = ToolboxPackagesManifest::load_user()?;

        for pkg in &self.to_capture {
            if !manifest.find_package(pkg) {
                manifest.add_package(pkg.clone());
                report.record_success(Verb::Capture, format!("package:{}", pkg));
            } else {
                report.record_success(Verb::Skip, format!("package:{} (already in manifest)", pkg));
            }
        }

        // Save the updated manifest
        manifest.save_user()?;

        Ok(report)
    }

    fn is_empty(&self) -> bool {
        self.to_capture.is_empty()
    }
}

/// Get user-installed packages from dnf history.
///
/// This returns packages that were explicitly installed by the user,
/// not auto-installed as dependencies.
fn get_user_installed_packages() -> Vec<String> {
    // Use dnf repoquery to find user-installed packages
    let output = Command::new("dnf")
        .args(["repoquery", "--userinstalled", "--qf", "%{name}"])
        .output();

    let output = match output {
        Ok(o) if o.status.success() => o,
        _ => return Vec::new(),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .filter(|line| !line.is_empty())
        .map(String::from)
        .collect()
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
        #[cfg(target_os = "linux")]
        {
            let _ = is_package_installed("bash");
        }
    }
}
