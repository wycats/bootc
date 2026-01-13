//! DNF/RPM package management command implementation.
//!
//! Provides a unified interface for managing RPM packages across contexts:
//! - Host: Uses rpm-ostree for atomic package layering
//! - Toolbox: Uses dnf for direct package installation
//!
//! Query commands (search, info, provides) pass through to dnf5 directly.

use crate::containerfile::{
    ContainerfileEditor, Section, generate_copr_repos, generate_host_shims,
    generate_system_packages,
};
use crate::context::{CommandDomain, ExecutionContext, PrMode};
use crate::manifest::ephemeral::{ChangeAction, ChangeDomain, EphemeralChange, EphemeralManifest};
use crate::manifest::{CoprRepo, ShimsManifest, SystemPackagesManifest, ToolboxPackagesManifest};
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
        /// Skip package validation
        #[arg(long)]
        force: bool,
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
    /// Capture layered packages not in manifest
    Capture {
        /// Apply immediately (add packages to manifest)
        #[arg(long)]
        apply: bool,
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
        DnfAction::Install {
            packages,
            now,
            force,
        } => handle_install(packages, now, force, plan),
        DnfAction::Remove { packages } => handle_remove(packages, plan),
        DnfAction::List { format } => handle_list(format, plan),
        DnfAction::Search { query } => handle_search(query),
        DnfAction::Info { package } => handle_info(package),
        DnfAction::Provides { path } => handle_provides(path),
        DnfAction::Diff => handle_diff(plan),
        DnfAction::Sync { now } => {
            plan.validate_domain(CommandDomain::Dnf)?;

            // Use the new Plan-based implementation
            let plan_ctx =
                PlanContext::new(std::env::current_dir().unwrap_or_default(), plan.clone());

            let sync_plan = DnfSyncCommand {
                now,
                context: plan.context,
            }
            .plan(&plan_ctx)?;

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
        DnfAction::Capture { apply } => {
            // Use the new Plan-based implementation
            let plan_ctx =
                PlanContext::new(std::env::current_dir().unwrap_or_default(), plan.clone());

            let capture_plan = DnfCaptureCommand.plan(&plan_ctx)?;

            if capture_plan.is_empty() {
                Output::success("All layered packages are already in the manifest.");
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

fn handle_list(format: String, plan: &ExecutionPlan) -> Result<()> {
    if plan.context == ExecutionContext::Dev {
        let manifest = ToolboxPackagesManifest::load_user()?;

        if format == "json" {
            println!("{}", serde_json::to_string_pretty(&manifest)?);
            return Ok(());
        }

        if manifest.packages.is_empty()
            && manifest.groups.is_empty()
            && manifest.copr_repos.is_empty()
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

        return Ok(());
    }

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
        println!("{:<40} SOURCE", "NAME".cyan());
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
        println!("{:<40} ENABLED GPG", "NAME".cyan());
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

fn handle_install(
    packages: Vec<String>,
    now: bool,
    force: bool,
    plan: &ExecutionPlan,
) -> Result<()> {
    // Validate context for mutating operations
    plan.validate_domain(CommandDomain::Dnf)?;

    if packages.is_empty() {
        bail!("No packages specified");
    }

    // Validate that packages exist in repositories
    if !force {
        for pkg in &packages {
            validate_dnf_package(pkg)?;
        }
    }

    if plan.context == ExecutionContext::Dev {
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

        if plan.should_update_local_manifest() {
            for pkg in &new_packages {
                manifest.add_package(pkg.clone());
            }
            manifest.save_user()?;
            Output::success(format!(
                "Added {} package(s) to user manifest",
                new_packages.len()
            ));
        } else if plan.dry_run {
            for pkg in &new_packages {
                Output::dry_run(format!("Would add to manifest: {}", pkg));
            }
        }

        if plan.should_execute_locally() {
            install_via_dnf(&packages)?;
        } else if plan.dry_run {
            Output::dry_run(format!("Would run: dnf install -y {}", packages.join(" ")));
        }

        return Ok(());
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
        Output::success(format!(
            "Added {} package(s) to user manifest",
            new_packages.len()
        ));
    } else if plan.dry_run {
        for pkg in &new_packages {
            Output::dry_run(format!("Would add to manifest: {}", pkg));
        }
    }

    // Record ephemeral changes if using --local (not in dry-run mode)
    if plan.pr_mode == PrMode::LocalOnly && !plan.dry_run && !new_packages.is_empty() {
        let mut ephemeral = EphemeralManifest::load_validated()?;
        for pkg in &new_packages {
            ephemeral.record(EphemeralChange::new(
                ChangeDomain::Dnf,
                ChangeAction::Add,
                pkg,
            ));
        }
        ephemeral.save()?;
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

        // Sync Containerfile before creating PR so both files are committed together
        sync_all_containerfile_sections(&system_manifest)?;

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

    if plan.context == ExecutionContext::Dev {
        let mut manifest = ToolboxPackagesManifest::load_user()?;

        for pkg in &packages {
            if plan.should_update_local_manifest() {
                if manifest.remove_package(pkg) {
                    Output::success(format!("Removed from user manifest: {}", pkg));
                } else {
                    Output::warning(format!("Package not found in manifest: {}", pkg));
                }
            } else if plan.dry_run {
                if manifest.find_package(pkg) {
                    Output::dry_run(format!("Would remove from user manifest: {}", pkg));
                } else {
                    Output::dry_run(format!("Package not found in manifest: {}", pkg));
                }
            }
        }

        if plan.should_update_local_manifest() {
            manifest.save_user()?;
        }

        if plan.should_execute_locally() {
            remove_via_dnf(&packages)?;
        } else if plan.dry_run {
            Output::dry_run(format!("Would run: dnf remove -y {}", packages.join(" ")));
        }

        return Ok(());
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

    // Record ephemeral changes if using --local (not in dry-run mode)
    if plan.pr_mode == PrMode::LocalOnly && !plan.dry_run {
        let mut ephemeral = EphemeralManifest::load_validated()?;
        for pkg in &packages {
            ephemeral.record(EphemeralChange::new(
                ChangeDomain::Dnf,
                ChangeAction::Remove,
                pkg,
            ));
        }
        ephemeral.save()?;
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

            // Sync Containerfile before creating PR so both files are committed together
            sync_all_containerfile_sections(&system_manifest)?;

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

fn handle_diff(plan: &ExecutionPlan) -> Result<()> {
    if plan.context == ExecutionContext::Dev {
        let manifest = ToolboxPackagesManifest::load_user()?;

        if manifest.packages.is_empty() {
            Output::info("Toolbox manifest is empty.");
            return Ok(());
        }

        let mut not_installed = Vec::new();
        let mut installed = Vec::new();

        for pkg in &manifest.packages {
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
                "Run 'bkt dev update' to install {} missing package(s).",
                not_installed.len()
            ));
        }

        return Ok(());
    }

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
// COPR Commands
// =============================================================================

pub fn handle_copr(action: CoprAction, plan: &ExecutionPlan) -> Result<()> {
    match action {
        CoprAction::Enable { name } => handle_copr_enable(name, plan),
        CoprAction::Disable { name } => handle_copr_disable(name, plan),
        CoprAction::List => handle_copr_list(plan),
    }
}

fn handle_copr_enable(name: String, plan: &ExecutionPlan) -> Result<()> {
    plan.validate_domain(CommandDomain::Dnf)?;

    if plan.context == ExecutionContext::Dev {
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

        if plan.should_update_local_manifest() {
            let mut system = manifest.as_system_manifest();
            system.upsert_copr(CoprRepo::new(name.clone()));
            manifest.update_from(&system);
            manifest.save_user()?;
            Output::success(format!("Added to user manifest: {}", name));
        } else if plan.dry_run {
            Output::dry_run(format!("Would add COPR to manifest: {}", name));
        }

        if plan.should_execute_locally() {
            Output::running(format!("dnf copr enable -y {}", name));
            let status = Command::new("dnf")
                .args(["copr", "enable", "-y", &name])
                .status()
                .context("Failed to enable COPR")?;

            if !status.success() {
                bail!("Failed to enable COPR: {}", name);
            }

            Output::success(format!("COPR enabled: {}", name));
        } else if plan.dry_run {
            Output::dry_run(format!("Would run: dnf copr enable -y {}", name));
        }

        return Ok(());
    }

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

        // Sync Containerfile before creating PR so both files are committed together
        sync_all_containerfile_sections(&system_manifest)?;

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

    if plan.context == ExecutionContext::Dev {
        let mut manifest = ToolboxPackagesManifest::load_user()?;

        let in_manifest = manifest.copr_repos.iter().any(|c| c.name == name);
        if !in_manifest {
            Output::warning(format!("COPR not found in manifest: {}", name));
            return Ok(());
        }

        if plan.should_update_local_manifest() {
            let mut system = manifest.as_system_manifest();
            if system.remove_copr(&name) {
                manifest.update_from(&system);
                manifest.save_user()?;
                Output::success(format!("Removed from user manifest: {}", name));
            }
        } else if plan.dry_run {
            Output::dry_run(format!("Would remove COPR from manifest: {}", name));
        }

        if plan.should_execute_locally() {
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

        return Ok(());
    }

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
            // Sync Containerfile before creating PR so both files are committed together
            sync_all_containerfile_sections(&system_manifest)?;

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

fn handle_copr_list(plan: &ExecutionPlan) -> Result<()> {
    if plan.context == ExecutionContext::Dev {
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

        return Ok(());
    }

    let system = SystemPackagesManifest::load_system()?;
    let user = SystemPackagesManifest::load_user()?;
    let merged = SystemPackagesManifest::merged(&system, &user);

    if merged.copr_repos.is_empty() {
        Output::info("No COPR repositories in manifest.");
        return Ok(());
    }

    Output::subheader("COPR REPOSITORIES:");
    println!("{:<40} {:<8} {:<8} SOURCE", "NAME".cyan(), "ENABLED", "GPG");
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

// ============================================================================
// Plan-based DNF Sync Implementation
// ============================================================================

/// Command to sync DNF packages from manifests.
pub struct DnfSyncCommand {
    /// Whether to apply live (rpm-ostree only).
    pub now: bool,
    /// Execution context (Host uses rpm-ostree, Dev uses dnf).
    pub context: ExecutionContext,
}

/// Plan for syncing DNF packages.
pub struct DnfSyncPlan {
    /// Packages to install.
    pub to_install: Vec<String>,
    /// Packages already installed.
    pub already_installed: usize,
    /// Whether to apply live (rpm-ostree only).
    pub now: bool,
    /// Execution context.
    pub context: ExecutionContext,
}

impl Plannable for DnfSyncCommand {
    type Plan = DnfSyncPlan;

    fn plan(&self, _ctx: &PlanContext) -> Result<Self::Plan> {
        let merged = if self.context == ExecutionContext::Dev {
            ToolboxPackagesManifest::load_user()?.as_system_manifest()
        } else {
            let system = SystemPackagesManifest::load_system()?;
            let user = SystemPackagesManifest::load_user()?;
            SystemPackagesManifest::merged(&system, &user)
        };

        let mut to_install = Vec::new();
        let mut already_installed = 0;

        for pkg in merged.packages {
            if is_package_installed(&pkg) {
                already_installed += 1;
            } else {
                to_install.push(pkg);
            }
        }

        Ok(DnfSyncPlan {
            to_install,
            already_installed,
            now: self.now,
            context: self.context,
        })
    }
}

impl Plan for DnfSyncPlan {
    fn describe(&self) -> PlanSummary {
        let method = match self.context {
            ExecutionContext::Host => {
                if self.now {
                    "rpm-ostree --apply-live"
                } else {
                    "rpm-ostree (reboot required)"
                }
            }
            ExecutionContext::Dev => "dnf",
            ExecutionContext::Image => "image-only",
        };

        let mut summary = PlanSummary::new(format!(
            "DNF Sync: {} to install, {} already installed (via {})",
            self.to_install.len(),
            self.already_installed,
            method
        ));

        for pkg in &self.to_install {
            summary.add_operation(Operation::new(Verb::Install, format!("package:{}", pkg)));
        }

        summary
    }

    fn execute(self, _ctx: &mut ExecuteContext) -> Result<ExecutionReport> {
        let mut report = ExecutionReport::new();

        if self.to_install.is_empty() {
            return Ok(report);
        }

        // Install all packages in one batch for efficiency
        let result = match self.context {
            ExecutionContext::Host => install_via_rpm_ostree(&self.to_install, self.now),
            ExecutionContext::Dev => install_via_dnf(&self.to_install),
            ExecutionContext::Image => {
                // No-op for image context
                Ok(())
            }
        };

        match result {
            Ok(()) => {
                // Record success for all packages
                for pkg in self.to_install {
                    report.record_success(Verb::Install, format!("package:{}", pkg));
                }
            }
            Err(e) => {
                // Record failure for all packages
                for pkg in self.to_install {
                    report.record_failure(Verb::Install, format!("package:{}", pkg), e.to_string());
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
// Plan-based DNF Capture Implementation
// ============================================================================

/// Command to capture layered packages not in manifest.
pub struct DnfCaptureCommand;

/// Plan for capturing layered packages.
pub struct DnfCapturePlan {
    /// Packages to add to manifest.
    pub to_capture: Vec<String>,
    /// Packages already in manifest.
    pub already_in_manifest: usize,
}

impl Plannable for DnfCaptureCommand {
    type Plan = DnfCapturePlan;

    fn plan(&self, _ctx: &PlanContext) -> Result<Self::Plan> {
        // Get layered packages from rpm-ostree
        let layered = get_layered_packages();

        // Load manifest to check what's already tracked
        let system = SystemPackagesManifest::load_system()?;
        let user = SystemPackagesManifest::load_user()?;
        let merged = SystemPackagesManifest::merged(&system, &user);

        let mut to_capture = Vec::new();
        let mut already_in_manifest = 0;

        for pkg in layered {
            if merged.packages.contains(&pkg) {
                already_in_manifest += 1;
            } else {
                to_capture.push(pkg);
            }
        }

        // Sort for consistent output
        to_capture.sort();

        Ok(DnfCapturePlan {
            to_capture,
            already_in_manifest,
        })
    }
}

impl Plan for DnfCapturePlan {
    fn describe(&self) -> PlanSummary {
        let mut summary = PlanSummary::new(format!(
            "DNF Capture: {} to add, {} already in manifest",
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

        // Load user manifest and add packages
        let mut user = SystemPackagesManifest::load_user()?;

        for pkg in &self.to_capture {
            if user.add_package(pkg.clone()) {
                report.record_success(Verb::Capture, format!("package:{}", pkg));
            } else {
                // Already exists (shouldn't happen, but handle gracefully)
                report.record_success(Verb::Skip, format!("package:{} (already in manifest)", pkg));
            }
        }

        // Save the updated manifest
        user.save_user()?;

        Ok(report)
    }

    fn is_empty(&self) -> bool {
        self.to_capture.is_empty()
    }
}

/// Get layered packages from rpm-ostree status.
/// Get layered packages from rpm-ostree status.
///
/// Returns a list of package names that have been explicitly layered
/// on the current rpm-ostree deployment via `rpm-ostree install`.
pub fn get_layered_packages() -> Vec<String> {
    let output = Command::new("rpm-ostree")
        .args(["status", "--json"])
        .output();

    let output = match output {
        Ok(o) if o.status.success() => o,
        _ => return Vec::new(),
    };

    let json: serde_json::Value = match serde_json::from_slice(&output.stdout) {
        Ok(j) => j,
        Err(_) => return Vec::new(),
    };

    let deployments = match json.get("deployments").and_then(|d| d.as_array()) {
        Some(d) => d,
        None => return Vec::new(),
    };

    // Find booted deployment
    let booted = match deployments
        .iter()
        .find(|d| d.get("booted").and_then(|b| b.as_bool()).unwrap_or(false))
    {
        Some(b) => b,
        None => return Vec::new(),
    };

    // Get requested-packages (the packages the user explicitly layered)
    booted
        .get("requested-packages")
        .or_else(|| booted.get("requested_packages"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// Sync the Containerfile SYSTEM_PACKAGES section with the manifest.
///
/// This updates the managed section in the Containerfile to match
/// the packages in the manifest. Returns Ok(true) if the Containerfile
/// was modified, Ok(false) if no changes were needed.
fn sync_all_containerfile_sections(manifest: &SystemPackagesManifest) -> Result<bool> {
    let containerfile_path = std::path::Path::new("Containerfile");
    if !containerfile_path.exists() {
        // No Containerfile in current directory, skip sync
        return Ok(false);
    }

    let mut editor = ContainerfileEditor::load(containerfile_path)?;
    let mut updated_any = false;

    // SYSTEM_PACKAGES
    if editor.has_section(Section::SystemPackages) {
        let new_content = generate_system_packages(&manifest.packages);
        editor.update_section(Section::SystemPackages, new_content);
        Output::success("Synced Containerfile SYSTEM_PACKAGES section");
        updated_any = true;
    } else {
        // Preserve prior behavior: warn when the expected SYSTEM_PACKAGES section is missing.
        Output::warning("Containerfile has no SYSTEM_PACKAGES section - skipping sync");
    }

    // COPR_REPOS
    if editor.has_section(Section::CoprRepos) {
        // Extract repo names from CoprRepo structs (only enabled ones)
        let repo_names: Vec<String> = manifest
            .copr_repos
            .iter()
            .filter(|c| c.enabled)
            .map(|c| c.name.clone())
            .collect();

        let new_content = generate_copr_repos(&repo_names);
        editor.update_section(Section::CoprRepos, new_content);
        Output::success("Synced Containerfile COPR_REPOS section");
        updated_any = true;
    } else if manifest.copr_repos.iter().any(|c| c.enabled) {
        // Only warn if COPRs are actually configured/enabled; otherwise this is a harmless omission.
        Output::warning("Containerfile has no COPR_REPOS section - skipping sync");
    }

    // HOST_SHIMS
    if editor.has_section(Section::HostShims) {
        // Load the merged shims manifest (repo + user)
        // We use load_repo() instead of load_system() because we're generating
        // the Containerfile from the repo's manifests directory.
        let repo_shims = ShimsManifest::load_repo()?;
        let user_shims = ShimsManifest::load_user()?;
        let merged_shims = ShimsManifest::merged(&repo_shims, &user_shims);

        let new_content = generate_host_shims(&merged_shims.shims);
        editor.update_section(Section::HostShims, new_content);
        Output::success("Synced Containerfile HOST_SHIMS section");
        updated_any = true;
    }
    // Note: No warning if HOST_SHIMS section is missing - it's optional

    if !updated_any {
        return Ok(false);
    }

    editor.write()?;
    Ok(true)
}
