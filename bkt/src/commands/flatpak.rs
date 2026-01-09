//! Flatpak command implementation.

use crate::context::{CommandDomain, PrMode};
use crate::manifest::ephemeral::{ChangeAction, ChangeDomain, EphemeralChange, EphemeralManifest};
use crate::manifest::{FlatpakApp, FlatpakAppsManifest, FlatpakScope};
use crate::output::Output;
use crate::pipeline::ExecutionPlan;
use crate::plan::{
    ExecuteContext, ExecutionReport, Operation, Plan, PlanContext, PlanSummary, Plannable, Verb,
};
use crate::validation::validate_flatpak_app;
use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use owo_colors::OwoColorize;
use std::process::Command;

#[derive(Debug, Args)]
pub struct FlatpakArgs {
    #[command(subcommand)]
    pub action: FlatpakAction,
}

#[derive(Debug, Subcommand)]
pub enum FlatpakAction {
    /// Add a Flatpak app to the manifest
    Add {
        /// Application ID (e.g., org.gnome.Calculator)
        app_id: String,
        /// Remote name (default: flathub)
        #[arg(short, long, default_value = "flathub")]
        remote: String,
        /// Installation scope (system or user)
        #[arg(short, long, default_value = "system")]
        scope: String,
        /// Skip validation that app exists on remote
        #[arg(long)]
        force: bool,
    },
    /// Remove a Flatpak app from the manifest
    Remove {
        /// Application ID to remove
        app_id: String,
    },
    /// List all Flatpak apps in the manifest
    List {
        /// Output format (table, json)
        #[arg(short, long, default_value = "table")]
        format: String,
    },
    /// Sync: install apps from manifest
    Sync,
    /// Capture installed flatpaks to manifest
    Capture {
        /// Show what would be done without making changes
        #[arg(long)]
        dry_run: bool,
        /// Apply the plan immediately
        #[arg(long)]
        apply: bool,
    },
}

/// Install a flatpak app using the flatpak CLI.
fn install_flatpak(app: &FlatpakApp) -> Result<bool> {
    let scope_flag = match app.scope {
        FlatpakScope::System => "--system",
        FlatpakScope::User => "--user",
    };

    let status = Command::new("flatpak")
        .args([
            "install",
            "-y",
            "--noninteractive",
            "--or-update",
            scope_flag,
            &app.remote,
            &app.id,
        ])
        .status()
        .context("Failed to run flatpak install")?;

    Ok(status.success())
}

/// Uninstall a flatpak app.
fn uninstall_flatpak(app_id: &str, scope: FlatpakScope) -> Result<bool> {
    let scope_flag = match scope {
        FlatpakScope::System => "--system",
        FlatpakScope::User => "--user",
    };

    let status = Command::new("flatpak")
        .args(["uninstall", "-y", "--noninteractive", scope_flag, app_id])
        .status()
        .context("Failed to run flatpak uninstall")?;

    Ok(status.success())
}

/// Check if a flatpak is installed.
fn is_installed(app_id: &str) -> bool {
    Command::new("flatpak")
        .args(["info", app_id])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn run(args: FlatpakArgs, plan: &ExecutionPlan) -> Result<()> {
    match args.action {
        FlatpakAction::Add {
            app_id,
            remote,
            scope,
            force,
        } => {
            // Validate that flatpak operations are allowed in this context
            plan.validate_domain(CommandDomain::Flatpak)?;

            // Validate that the app exists on the remote
            if !force {
                validate_flatpak_app(&app_id, &remote)?;
            }

            let scope: FlatpakScope = scope.parse()?;

            // Check if already in manifest
            let system = FlatpakAppsManifest::load_system()?;
            let mut user = FlatpakAppsManifest::load_user()?;

            let already_exists = system.find(&app_id).is_some() || user.find(&app_id).is_some();

            if already_exists {
                Output::warning(format!("Flatpak already in manifest: {}", app_id));
            } else if plan.should_update_local_manifest() {
                let app = FlatpakApp {
                    id: app_id.clone(),
                    remote: remote.clone(),
                    scope,
                };
                user.upsert(app.clone());
                user.save_user()?;
                Output::success(format!(
                    "Added to user manifest: {} ({}, {})",
                    app_id, remote, scope
                ));
            } else if plan.dry_run {
                Output::dry_run(format!(
                    "Would add to manifest: {} ({}, {})",
                    app_id, remote, scope
                ));
            }

            // Record ephemeral change if using --local (not in dry-run mode)
            if plan.pr_mode == PrMode::LocalOnly && !plan.dry_run && !already_exists {
                let mut ephemeral = EphemeralManifest::load_validated()?;
                ephemeral.record(EphemeralChange::new(
                    ChangeDomain::Flatpak,
                    ChangeAction::Add,
                    &app_id,
                ));
                ephemeral.save()?;
            }

            // Install the flatpak
            if plan.should_execute_locally() && !is_installed(&app_id) {
                let spinner = Output::spinner(format!("Installing {}...", app_id));
                let app = FlatpakApp {
                    id: app_id.clone(),
                    remote: remote.clone(),
                    scope,
                };
                if install_flatpak(&app)? {
                    spinner.finish_success(format!("Installed {}", app_id));
                } else {
                    spinner.finish_error(format!("Failed to install {}", app_id));
                }
            } else if plan.dry_run && !is_installed(&app_id) {
                Output::dry_run(format!("Would install: {}", app_id));
            } else if is_installed(&app_id) {
                Output::info(format!("Already installed: {}", app_id));
            }

            // Create PR if needed
            if plan.should_create_pr() && !already_exists {
                let mut system_manifest = FlatpakAppsManifest::load_system()?;
                let app_for_pr = FlatpakApp {
                    id: app_id.clone(),
                    remote: remote.clone(),
                    scope,
                };
                system_manifest.upsert(app_for_pr);
                let manifest_content = serde_json::to_string_pretty(&system_manifest)?;

                plan.maybe_create_pr(
                    "flatpak",
                    "add",
                    &app_id,
                    "flatpak-apps.json",
                    &manifest_content,
                )?;
            }
        }
        FlatpakAction::Remove { app_id } => {
            // Validate that flatpak operations are allowed in this context
            plan.validate_domain(CommandDomain::Flatpak)?;

            let mut user = FlatpakAppsManifest::load_user()?;
            let system = FlatpakAppsManifest::load_system()?;

            // Check if it's in system manifest
            let in_system = system.find(&app_id).is_some();
            if in_system && user.find(&app_id).is_none() && !plan.should_create_pr() {
                Output::info(format!(
                    "'{}' is only in the system manifest; run without --local to create a PR",
                    app_id
                ));
            }

            if plan.should_update_local_manifest() {
                if user.remove(&app_id) {
                    user.save_user()?;
                    Output::success(format!("Removed from user manifest: {}", app_id));
                } else if !in_system {
                    Output::warning(format!("Flatpak not found in manifest: {}", app_id));
                }
            } else if plan.dry_run {
                if user.find(&app_id).is_some() {
                    Output::dry_run(format!("Would remove from user manifest: {}", app_id));
                } else if !in_system {
                    Output::dry_run(format!("Flatpak not found in manifest: {}", app_id));
                }
            }

            // Record ephemeral change if using --local (not in dry-run mode)
            if plan.pr_mode == PrMode::LocalOnly && !plan.dry_run {
                let mut ephemeral = EphemeralManifest::load_validated()?;
                ephemeral.record(EphemeralChange::new(
                    ChangeDomain::Flatpak,
                    ChangeAction::Remove,
                    &app_id,
                ));
                ephemeral.save()?;
            }

            // Optionally uninstall
            if plan.should_execute_locally() && is_installed(&app_id) {
                let spinner = Output::spinner(format!("Uninstalling {}...", app_id));
                // Try system first, then user
                if uninstall_flatpak(&app_id, FlatpakScope::System)?
                    || uninstall_flatpak(&app_id, FlatpakScope::User)?
                {
                    spinner.finish_success(format!("Uninstalled {}", app_id));
                } else {
                    spinner.finish_warning(format!("May need manual removal: {}", app_id));
                }
            } else if plan.dry_run && is_installed(&app_id) {
                Output::dry_run(format!("Would uninstall: {}", app_id));
            }

            // Create PR if needed
            if plan.should_create_pr() {
                let mut system_manifest = FlatpakAppsManifest::load_system()?;
                if system_manifest.remove(&app_id) {
                    let manifest_content = serde_json::to_string_pretty(&system_manifest)?;

                    plan.maybe_create_pr(
                        "flatpak",
                        "remove",
                        &app_id,
                        "flatpak-apps.json",
                        &manifest_content,
                    )?;
                } else {
                    Output::info(format!("'{}' not in system manifest, no PR needed", app_id));
                }
            }
        }
        FlatpakAction::List { format } => {
            let system = FlatpakAppsManifest::load_system()?;
            let user = FlatpakAppsManifest::load_user()?;
            let merged = FlatpakAppsManifest::merged(&system, &user);

            if format == "json" {
                println!("{}", serde_json::to_string_pretty(&merged)?);
            } else {
                if merged.apps.is_empty() {
                    Output::info("No flatpak apps in manifest.");
                    return Ok(());
                }

                Output::subheader("FLATPAK APPS:");
                println!(
                    "{:<50} {:<12} {:<8} {} {}",
                    "ID".cyan(),
                    "REMOTE".cyan(),
                    "SCOPE".cyan(),
                    "SOURCE".cyan(),
                    "INSTALLED".cyan()
                );
                Output::separator();

                for app in &merged.apps {
                    let source = if user.find(&app.id).is_some() {
                        "user".yellow().to_string()
                    } else {
                        "system".dimmed().to_string()
                    };
                    let installed = if is_installed(&app.id) {
                        "✓".green().to_string()
                    } else {
                        "✗".red().to_string()
                    };
                    println!(
                        "{:<50} {:<12} {:<8} {:>8} {}",
                        app.id, app.remote, app.scope, source, installed
                    );
                }

                Output::blank();
                Output::info(format!(
                    "{} apps ({} system, {} user)",
                    merged.apps.len(),
                    system.apps.len(),
                    user.apps.len()
                ));
            }
        }
        FlatpakAction::Sync => {
            // Validate that flatpak operations are allowed in this context
            plan.validate_domain(CommandDomain::Flatpak)?;

            // Use the new Plan-based implementation
            let plan_ctx =
                PlanContext::new(std::env::current_dir().unwrap_or_default(), plan.clone());

            let sync_plan = FlatpakSyncCommand.plan(&plan_ctx)?;

            if sync_plan.is_empty() {
                Output::success("All flatpaks are already installed.");
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
        }
        FlatpakAction::Capture { dry_run, apply } => {
            // Note: No domain validation needed for capture since it only:
            // 1. Reads installed flatpaks (same as `flatpak list` which works anywhere)
            // 2. Writes to the local manifest file

            // Use the Plan-based capture implementation
            let plan_ctx = PlanContext::new(
                std::env::current_dir().unwrap_or_default(),
                plan.with_dry_run(dry_run),
            );

            let capture_plan = FlatpakCaptureCommand.plan(&plan_ctx)?;

            if capture_plan.is_empty() {
                Output::success("All installed flatpaks are already in the manifest.");
                return Ok(());
            }

            // Always show the plan
            print!("{}", capture_plan.describe());

            if dry_run || !apply {
                if !apply {
                    Output::hint("Use --apply to execute this plan.");
                }
                return Ok(());
            }

            // Execute the plan
            let mut exec_ctx = ExecuteContext::new(plan.with_dry_run(false));
            let report = capture_plan.execute(&mut exec_ctx)?;
            print!("{}", report);
        }
    }
    Ok(())
}

// ============================================================================
// Plan-based Flatpak Sync Implementation
// ============================================================================

/// A flatpak that needs to be installed.
#[derive(Debug, Clone)]
pub struct FlatpakToInstall {
    /// The flatpak app.
    pub app: FlatpakApp,
}

/// Command to sync flatpaks from manifests.
pub struct FlatpakSyncCommand;

/// Plan for syncing flatpaks.
pub struct FlatpakSyncPlan {
    /// Flatpaks to install.
    pub to_install: Vec<FlatpakToInstall>,
    /// Flatpaks already installed.
    pub already_installed: usize,
}

impl Plannable for FlatpakSyncCommand {
    type Plan = FlatpakSyncPlan;

    fn plan(&self, _ctx: &PlanContext) -> Result<Self::Plan> {
        // Load and merge manifests (read-only, no side effects)
        let system = FlatpakAppsManifest::load_system()?;
        let user = FlatpakAppsManifest::load_user()?;
        let merged = FlatpakAppsManifest::merged(&system, &user);

        let mut to_install = Vec::new();
        let mut already_installed = 0;

        for app in merged.apps {
            if is_installed(&app.id) {
                already_installed += 1;
            } else {
                to_install.push(FlatpakToInstall { app });
            }
        }

        Ok(FlatpakSyncPlan {
            to_install,
            already_installed,
        })
    }
}

impl Plan for FlatpakSyncPlan {
    fn describe(&self) -> PlanSummary {
        let mut summary = PlanSummary::new(format!(
            "Flatpak Sync: {} to install, {} already installed",
            self.to_install.len(),
            self.already_installed
        ));

        for item in &self.to_install {
            summary.add_operation(Operation::with_details(
                Verb::Install,
                format!("flatpak:{}", item.app.id),
                format!("{} ({})", item.app.remote, item.app.scope),
            ));
        }

        summary
    }

    fn execute(self, _ctx: &mut ExecuteContext) -> Result<ExecutionReport> {
        let mut report = ExecutionReport::new();

        for item in self.to_install {
            match install_flatpak(&item.app) {
                Ok(true) => {
                    report.record_success(Verb::Install, format!("flatpak:{}", item.app.id));
                }
                Ok(false) => {
                    report.record_failure(
                        Verb::Install,
                        format!("flatpak:{}", item.app.id),
                        "flatpak install failed",
                    );
                }
                Err(e) => {
                    report.record_failure(
                        Verb::Install,
                        format!("flatpak:{}", item.app.id),
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
// Plan-based Flatpak Capture Implementation
// ============================================================================

/// A flatpak installed on the system but not in manifest.
#[derive(Debug, Clone)]
pub struct InstalledFlatpak {
    /// Scope: "system" or "user"
    pub installation: String,
    /// The app ID (e.g., org.gnome.Calculator)
    pub id: String,
    /// The remote/origin (e.g., flathub)
    pub origin: String,
}

/// Get list of installed flatpaks from the system.
fn get_installed_flatpaks() -> Vec<InstalledFlatpak> {
    let output = Command::new("flatpak")
        .args(["list", "--app", "--columns=installation,application,origin"])
        .output();

    match output {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            let mut apps = Vec::new();

            for line in stdout.lines() {
                let parts: Vec<&str> = line.split('\t').collect();
                if parts.len() >= 2 {
                    apps.push(InstalledFlatpak {
                        installation: parts.first().unwrap_or(&"system").to_string(),
                        id: parts.get(1).unwrap_or(&"").to_string(),
                        origin: parts.get(2).unwrap_or(&"flathub").to_string(),
                    });
                }
            }

            apps
        }
        _ => Vec::new(),
    }
}

/// A flatpak to capture (add to manifest).
#[derive(Debug, Clone)]
pub struct FlatpakToCapture {
    /// The flatpak app to add.
    pub app: FlatpakApp,
}

/// Command to capture installed flatpaks to manifest.
pub struct FlatpakCaptureCommand;

/// Plan for capturing flatpaks.
pub struct FlatpakCapturePlan {
    /// Flatpaks to add to manifest.
    pub to_capture: Vec<FlatpakToCapture>,
    /// Flatpaks already in manifest.
    pub already_in_manifest: usize,
}

impl Plannable for FlatpakCaptureCommand {
    type Plan = FlatpakCapturePlan;

    fn plan(&self, _ctx: &PlanContext) -> Result<Self::Plan> {
        // Get currently installed flatpaks on the system
        let installed = get_installed_flatpaks();

        // Load manifests to see what's already tracked
        let system = FlatpakAppsManifest::load_system()?;
        let user = FlatpakAppsManifest::load_user()?;
        let merged = FlatpakAppsManifest::merged(&system, &user);

        let mut to_capture = Vec::new();
        let mut already_in_manifest = 0;

        for flatpak in installed {
            if merged.find(&flatpak.id).is_some() {
                already_in_manifest += 1;
            } else {
                // Convert installation string to FlatpakScope
                let scope = if flatpak.installation == "user" {
                    FlatpakScope::User
                } else {
                    FlatpakScope::System
                };

                to_capture.push(FlatpakToCapture {
                    app: FlatpakApp {
                        id: flatpak.id,
                        remote: if flatpak.origin.is_empty() {
                            "flathub".to_string()
                        } else {
                            flatpak.origin
                        },
                        scope,
                    },
                });
            }
        }

        // Sort for consistent output
        to_capture.sort_by(|a, b| a.app.id.cmp(&b.app.id));

        Ok(FlatpakCapturePlan {
            to_capture,
            already_in_manifest,
        })
    }
}

impl Plan for FlatpakCapturePlan {
    fn describe(&self) -> PlanSummary {
        let mut summary = PlanSummary::new(format!(
            "Flatpak Capture: {} to add, {} already in manifest",
            self.to_capture.len(),
            self.already_in_manifest
        ));

        for item in &self.to_capture {
            summary.add_operation(Operation::with_details(
                Verb::Capture,
                format!("flatpak:{}", item.app.id),
                format!("{} ({})", item.app.remote, item.app.scope),
            ));
        }

        summary
    }

    fn execute(self, _ctx: &mut ExecuteContext) -> Result<ExecutionReport> {
        let mut report = ExecutionReport::new();

        // Load user manifest (we add captured flatpaks there)
        let mut user = FlatpakAppsManifest::load_user()?;

        for item in self.to_capture {
            user.upsert(item.app.clone());
            report.record_success(Verb::Capture, format!("flatpak:{}", item.app.id));
        }

        // Save the updated manifest
        user.save_user()?;

        Ok(report)
    }

    fn is_empty(&self) -> bool {
        self.to_capture.is_empty()
    }
}
