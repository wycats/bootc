//! System package management command implementation.
//!
//! `bkt system` manages packages baked into the bootc image. This is a deferred
//! operation: packages are added to the manifest and Containerfile, then a PR
//! is created. The packages don't actually exist until the image rebuilds.
//!
//! # Verb Semantics
//!
//! - `add` — Add to recipe (deferred, appears after image rebuild)
//! - `remove` — Remove from recipe (deferred)
//! - `list` — Show what's in the manifest
//! - `capture` — Capture rpm-ostree layered packages to manifest
//!
//! # Examples
//!
//! ```bash
//! # Add a package to the image (creates PR)
//! bkt system add virt-manager
//!
//! # Add without creating PR (for batch changes)
//! bkt system add --local virt-manager
//!
//! # Capture layered packages to manifest
//! bkt system capture --apply
//! ```

use crate::containerfile::{
    ContainerfileEditor, Section, generate_copr_repos, generate_system_packages,
};
use crate::context::CommandDomain;
use crate::manifest::ephemeral::{ChangeAction, ChangeDomain, EphemeralChange, EphemeralManifest};
use crate::manifest::{CoprRepo, SystemPackagesManifest};
use crate::output::Output;
use crate::pipeline::ExecutionPlan;
use crate::plan::{
    ExecuteContext, ExecutionReport, Operation, Plan, PlanContext, PlanSummary, Plannable, Verb,
};
use crate::validation::validate_dnf_package;
use anyhow::{Result, bail};
use clap::{Args, Subcommand};
use owo_colors::OwoColorize;
use std::process::Command;

#[derive(Debug, Args)]
pub struct SystemArgs {
    #[command(subcommand)]
    pub action: SystemAction,
}

#[derive(Debug, Subcommand)]
pub enum SystemAction {
    /// Add packages to the system image
    ///
    /// Updates manifest + Containerfile and creates a PR.
    /// The package will be available after the image rebuilds.
    Add {
        /// Package names to add
        packages: Vec<String>,
        /// Skip package validation
        #[arg(long)]
        force: bool,
    },
    /// Remove packages from the system image
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
    /// Capture layered packages not in manifest
    ///
    /// Finds packages installed via rpm-ostree that aren't tracked
    /// in system-packages.json and adds them.
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
    /// Enable a COPR repository in the system image
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

pub fn run(args: SystemArgs, plan: &ExecutionPlan) -> Result<()> {
    match args.action {
        SystemAction::Add { packages, force } => handle_add(packages, force, plan),
        SystemAction::Remove { packages } => handle_remove(packages, plan),
        SystemAction::List { format } => handle_list(format),
        SystemAction::Capture { apply } => {
            // Use the Plan-based implementation
            let plan_ctx =
                PlanContext::new(std::env::current_dir().unwrap_or_default(), plan.clone());

            let capture_plan = SystemCaptureCommand.plan(&plan_ctx)?;

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
        SystemAction::Copr { action } => handle_copr(action, plan),
    }
}

// =============================================================================
// Add Command
// =============================================================================

fn handle_add(packages: Vec<String>, force: bool, plan: &ExecutionPlan) -> Result<()> {
    plan.validate_domain(CommandDomain::System)?;

    if packages.is_empty() {
        bail!("No packages specified");
    }

    // Validate that packages exist in repositories
    if !force {
        for pkg in &packages {
            validate_dnf_package(pkg)?;
        }
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
    if plan.pr_mode == crate::context::PrMode::LocalOnly
        && !plan.dry_run
        && !new_packages.is_empty()
    {
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

    // NOTE: No local execution! System packages are deferred until image rebuild.
    // This is the key difference from the old `bkt dnf install` behavior.

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
            "system",
            "add",
            &new_packages.join(", "),
            "system-packages.json",
            &manifest_content,
        )?;
    }

    Ok(())
}

// =============================================================================
// Remove Command
// =============================================================================

fn handle_remove(packages: Vec<String>, plan: &ExecutionPlan) -> Result<()> {
    plan.validate_domain(CommandDomain::System)?;

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

    // Record ephemeral changes if using --local (not in dry-run mode)
    if plan.pr_mode == crate::context::PrMode::LocalOnly && !plan.dry_run {
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

    // NOTE: No local execution! Removal is deferred until image rebuild.

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
                "system",
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

// =============================================================================
// List Command
// =============================================================================

fn handle_list(format: String) -> Result<()> {
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
        println!("{:<40} SOURCE  INSTALLED", "NAME".cyan());
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
            println!("{:<40} {:<7} {}", pkg, source, installed);
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
// COPR Commands
// =============================================================================

fn handle_copr(action: CoprAction, plan: &ExecutionPlan) -> Result<()> {
    match action {
        CoprAction::Enable { name } => handle_copr_enable(name, plan),
        CoprAction::Disable { name } => handle_copr_disable(name, plan),
        CoprAction::List => handle_copr_list(),
    }
}

fn handle_copr_enable(name: String, plan: &ExecutionPlan) -> Result<()> {
    plan.validate_domain(CommandDomain::System)?;

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

    // NOTE: No local execution! COPR is enabled in the image at build time.

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
    plan.validate_domain(CommandDomain::System)?;

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

    // NOTE: No local execution! COPR is disabled in the image at build time.

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

fn handle_copr_list() -> Result<()> {
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

/// Sync the Containerfile sections with the manifest.
fn sync_all_containerfile_sections(manifest: &SystemPackagesManifest) -> Result<bool> {
    let containerfile_path = std::path::Path::new("Containerfile");
    if !containerfile_path.exists() {
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
        Output::warning("Containerfile has no SYSTEM_PACKAGES section - skipping sync");
    }

    // COPR_REPOS
    if editor.has_section(Section::CoprRepos) {
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
        Output::warning("Containerfile has no COPR_REPOS section - skipping sync");
    }

    if !updated_any {
        return Ok(false);
    }

    editor.write()?;
    Ok(true)
}

// ============================================================================
// Plan-based System Capture Implementation
// ============================================================================

/// Command to capture layered packages not in manifest.
pub struct SystemCaptureCommand;

/// Plan for capturing layered packages.
pub struct SystemCapturePlan {
    /// Packages to add to manifest.
    pub to_capture: Vec<String>,
    /// Packages already in manifest.
    pub already_in_manifest: usize,
}

impl Plannable for SystemCaptureCommand {
    type Plan = SystemCapturePlan;

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

        Ok(SystemCapturePlan {
            to_capture,
            already_in_manifest,
        })
    }
}

impl Plan for SystemCapturePlan {
    fn describe(&self) -> PlanSummary {
        let mut summary = PlanSummary::new(format!(
            "System Capture: {} to add, {} already in manifest",
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
fn get_layered_packages() -> Vec<String> {
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
