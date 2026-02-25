//! Flatpak command implementation.

use crate::command_runner::{CommandOptions, CommandRunner};
use crate::context::{CommandDomain, run_command};
use crate::manifest::{
    FlatpakApp, FlatpakAppsManifest, FlatpakOverrides, FlatpakRemotesManifest, FlatpakScope,
};
use crate::output::Output;
use crate::pipeline::ExecutionPlan;
use crate::plan::{
    ExecuteContext, ExecutionReport, Operation, Plan, PlanContext, PlanSummary, PlanWarning,
    Plannable, Verb,
};
use crate::validation::validate_flatpak_app;
use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use owo_colors::OwoColorize;

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

fn install_flatpak(app: &FlatpakApp, runner: &dyn CommandRunner) -> Result<bool> {
    let scope_flag = match app.scope {
        FlatpakScope::System => "--system",
        FlatpakScope::User => "--user",
    };

    let status = runner
        .run_status(
            "flatpak",
            &[
                "install",
                "-y",
                "--noninteractive",
                "--or-update",
                scope_flag,
                &app.remote,
                &app.id,
            ],
            &CommandOptions::default(),
        )
        .context("Failed to run flatpak install")?;

    Ok(status.success())
}

fn uninstall_flatpak(
    app_id: &str,
    scope: FlatpakScope,
    runner: &dyn CommandRunner,
) -> Result<bool> {
    let scope_flag = match scope {
        FlatpakScope::System => "--system",
        FlatpakScope::User => "--user",
    };

    let status = runner
        .run_status(
            "flatpak",
            &["uninstall", "-y", "--noninteractive", scope_flag, app_id],
            &CommandOptions::default(),
        )
        .context("Failed to run flatpak uninstall")?;

    Ok(status.success())
}

fn is_installed(app_id: &str, runner: &dyn CommandRunner) -> bool {
    runner
        .run_output("flatpak", &["info", app_id], &CommandOptions::default())
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Apply overrides to a flatpak app.
///
/// Uses `flatpak override --user` or `--system` depending on scope.
fn apply_overrides(
    app_id: &str,
    scope: FlatpakScope,
    overrides: &[String],
    runner: &dyn CommandRunner,
) -> Result<bool> {
    if overrides.is_empty() {
        return Ok(true);
    }

    let scope_flag = match scope {
        FlatpakScope::System => "--system",
        FlatpakScope::User => "--user",
    };

    let mut args = vec!["override", scope_flag];
    args.push(app_id);

    // Add each override flag
    for flag in overrides {
        args.push(flag);
    }

    let status = runner
        .run_status("flatpak", &args, &CommandOptions::default())
        .context("Failed to run flatpak override")?;

    Ok(status.success())
}

pub fn run(args: FlatpakArgs, plan: &ExecutionPlan) -> Result<()> {
    let runner = plan.runner();

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
                validate_flatpak_app(runner, &app_id, &remote)?;
            }

            let scope: FlatpakScope = scope.parse()?;

            // Check if already in manifest
            let mut manifest = FlatpakAppsManifest::load_repo()?;

            let already_exists = manifest.find(&app_id).is_some();

            if already_exists {
                Output::warning(format!("Flatpak already in manifest: {}", app_id));
            } else if plan.should_update_manifest() {
                let app = FlatpakApp {
                    id: app_id.clone(),
                    remote: remote.clone(),
                    scope,
                    branch: None,
                    commit: None,
                    overrides: None,
                };
                manifest.upsert(app.clone());
                manifest.save_repo()?;
                Output::success(format!(
                    "Added to manifest: {} ({}, {})",
                    app_id, remote, scope
                ));
            } else if plan.dry_run {
                Output::dry_run(format!(
                    "Would add to manifest: {} ({}, {})",
                    app_id, remote, scope
                ));
            }

            // Install the flatpak
            if plan.should_execute_locally() && !is_installed(&app_id, runner) {
                let spinner = Output::spinner(format!("Installing {}...", app_id));
                let app = FlatpakApp {
                    id: app_id.clone(),
                    remote: remote.clone(),
                    scope,
                    branch: None,
                    commit: None,
                    overrides: None,
                };
                if install_flatpak(&app, runner)? {
                    spinner.finish_success(format!("Installed {}", app_id));
                } else {
                    spinner.finish_error(format!("Failed to install {}", app_id));
                }
            } else if plan.dry_run && !is_installed(&app_id, runner) {
                Output::dry_run(format!("Would install: {}", app_id));
            } else if is_installed(&app_id, runner) {
                Output::info(format!("Already installed: {}", app_id));
            }

            // Create PR if needed
            if plan.should_create_pr() && !already_exists {
                let mut system_manifest = FlatpakAppsManifest::load_repo()?;
                let app_for_pr = FlatpakApp {
                    id: app_id.clone(),
                    remote: remote.clone(),
                    scope,
                    branch: None,
                    commit: None,
                    overrides: None,
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

            let mut manifest = FlatpakAppsManifest::load_repo()?;

            let in_manifest = manifest.find(&app_id).is_some();

            if plan.should_update_manifest() {
                if manifest.remove(&app_id) {
                    manifest.save_repo()?;
                    Output::success(format!("Removed from manifest: {}", app_id));
                } else {
                    Output::warning(format!("Flatpak not found in manifest: {}", app_id));
                }
            } else if plan.dry_run {
                if in_manifest {
                    Output::dry_run(format!("Would remove from manifest: {}", app_id));
                } else {
                    Output::dry_run(format!("Flatpak not found in manifest: {}", app_id));
                }
            }

            // Optionally uninstall
            if plan.should_execute_locally() && is_installed(&app_id, runner) {
                let spinner = Output::spinner(format!("Uninstalling {}...", app_id));
                // Try system first, then user
                if uninstall_flatpak(&app_id, FlatpakScope::System, runner)?
                    || uninstall_flatpak(&app_id, FlatpakScope::User, runner)?
                {
                    spinner.finish_success(format!("Uninstalled {}", app_id));
                } else {
                    spinner.finish_warning(format!("May need manual removal: {}", app_id));
                }
            } else if plan.dry_run && is_installed(&app_id, runner) {
                Output::dry_run(format!("Would uninstall: {}", app_id));
            }

            // Create PR if needed
            if plan.should_create_pr() {
                let mut system_manifest = FlatpakAppsManifest::load_repo()?;
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
                    Output::info(format!("'{}' not in manifest, no PR needed", app_id));
                }
            }
        }
        FlatpakAction::List { format } => {
            let merged = FlatpakAppsManifest::load_repo()?;

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
                    let source = "manifest".dimmed().to_string();
                    let installed = if is_installed(&app.id, runner) {
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
                Output::info(format!("{} apps in manifest", merged.apps.len()));
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

    fn plan(&self, ctx: &PlanContext) -> Result<Self::Plan> {
        // Load manifest (read-only, no side effects)
        let merged = FlatpakAppsManifest::load_repo()?;

        let mut to_install = Vec::new();
        let mut already_installed = 0;

        let runner = ctx.execution_plan().runner();

        for app in merged.apps {
            if is_installed(&app.id, runner) {
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

    fn execute(self, ctx: &mut ExecuteContext) -> Result<ExecutionReport> {
        let mut report = ExecutionReport::new();

        for item in self.to_install {
            let install_result = {
                let runner = ctx.execution_plan().runner();
                install_flatpak(&item.app, runner)
            };

            match install_result {
                Ok(true) => {
                    report.record_success_and_notify(
                        ctx,
                        Verb::Install,
                        format!("flatpak:{}", item.app.id),
                    );

                    // Apply overrides if present
                    if let Some(ref overrides) = item.app.overrides
                        && !overrides.is_empty()
                    {
                        let overrides_result = {
                            let runner = ctx.execution_plan().runner();
                            apply_overrides(&item.app.id, item.app.scope, overrides, runner)
                        };

                        match overrides_result {
                            Ok(true) => {
                                report.record_success_and_notify(
                                    ctx,
                                    Verb::Configure,
                                    format!("flatpak:{}:overrides", item.app.id),
                                );
                            }
                            Ok(false) => {
                                report.record_failure_and_notify(
                                    ctx,
                                    Verb::Configure,
                                    format!("flatpak:{}:overrides", item.app.id),
                                    "flatpak override failed",
                                );
                            }
                            Err(e) => {
                                report.record_failure_and_notify(
                                    ctx,
                                    Verb::Configure,
                                    format!("flatpak:{}:overrides", item.app.id),
                                    e.to_string(),
                                );
                            }
                        }
                    }
                }
                Ok(false) => {
                    report.record_failure_and_notify(
                        ctx,
                        Verb::Install,
                        format!("flatpak:{}", item.app.id),
                        "flatpak install failed",
                    );
                }
                Err(e) => {
                    report.record_failure_and_notify(
                        ctx,
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
    /// The branch (e.g. stable)
    pub branch: String,
    /// The commit hash
    pub commit: String,
}

/// Get list of installed flatpaks from the system.
///
/// When running inside a toolbox, this delegates to the host via flatpak-spawn.
/// If flatpak-spawn is not available or fails, returns an empty vector and logs a warning.
pub fn get_installed_flatpaks() -> Vec<InstalledFlatpak> {
    let output = run_command(
        "flatpak",
        &[
            "list",
            "--app",
            "--columns=installation,application,origin,branch,active",
        ],
    );

    match output {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            let mut apps = Vec::new();

            for line in stdout.lines() {
                let mut parts = line.split_whitespace();
                let installation = parts.next().unwrap_or("system");
                let id = parts.next().unwrap_or("");
                if id.is_empty() {
                    continue;
                }

                apps.push(InstalledFlatpak {
                    installation: installation.to_string(),
                    id: id.to_string(),
                    origin: parts.next().unwrap_or("flathub").to_string(),
                    branch: parts.next().unwrap_or("stable").to_string(),
                    commit: parts.next().unwrap_or("").to_string(),
                });
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
    /// Whether the remote is unmanaged (not in flatpak-remotes.json).
    /// Used for warning display; will be used for filtering in future.
    #[allow(dead_code)]
    pub unmanaged_remote: bool,
}

/// Command to capture installed flatpaks to manifest.
pub struct FlatpakCaptureCommand;

/// Plan for capturing flatpaks.
pub struct FlatpakCapturePlan {
    /// Flatpaks to add to manifest.
    pub to_capture: Vec<FlatpakToCapture>,
    /// Flatpaks already in manifest.
    pub already_in_manifest: usize,
    /// Warnings about unmanaged remotes.
    pub warnings: Vec<PlanWarning>,
}

impl Plannable for FlatpakCaptureCommand {
    type Plan = FlatpakCapturePlan;

    fn plan(&self, _ctx: &PlanContext) -> Result<Self::Plan> {
        // Get currently installed flatpaks on the system
        let installed = get_installed_flatpaks();

        // Load manifests to see what's already tracked
        let merged = FlatpakAppsManifest::load_repo()?;

        // Load remotes manifest to check for unmanaged remotes
        let remotes = FlatpakRemotesManifest::load_cwd().unwrap_or_default();

        let mut to_capture = Vec::new();
        let mut already_in_manifest = 0;
        let mut warnings = Vec::new();

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

                let remote = if flatpak.origin.is_empty() {
                    "flathub".to_string()
                } else {
                    flatpak.origin
                };

                // Check if the remote is managed
                let unmanaged_remote = !remotes.has_remote(&remote);
                if unmanaged_remote {
                    warnings.push(PlanWarning::new(
                        format!("flatpak:{}", flatpak.id),
                        format!(
                            "remote '{}' is not in flatpak-remotes.json; \
                             this app may have been installed from a .flatpak bundle",
                            remote
                        ),
                    ));
                }

                // Read any existing overrides for this app
                let overrides = FlatpakOverrides::load_for_app(&flatpak.id, scope)
                    .map(|o| o.to_cli_flags())
                    .filter(|flags| !flags.is_empty());

                to_capture.push(FlatpakToCapture {
                    app: FlatpakApp {
                        id: flatpak.id,
                        remote,
                        scope,
                        branch: Some(flatpak.branch),
                        commit: if flatpak.commit.is_empty() {
                            None
                        } else {
                            Some(flatpak.commit)
                        },
                        overrides,
                    },
                    unmanaged_remote,
                });
            }
        }

        // Sort for consistent output
        to_capture.sort_by(|a, b| a.app.id.cmp(&b.app.id));

        Ok(FlatpakCapturePlan {
            to_capture,
            already_in_manifest,
            warnings,
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

        // Add warnings about unmanaged remotes
        for warning in &self.warnings {
            summary.add_warning(warning.clone());
        }

        summary
    }

    fn execute(self, _ctx: &mut ExecuteContext) -> Result<ExecutionReport> {
        let mut report = ExecutionReport::new();

        // Load manifest (we add captured flatpaks there)
        let mut manifest = FlatpakAppsManifest::load_repo()?;

        for item in self.to_capture {
            manifest.upsert(item.app.clone());
            report.record_success(Verb::Capture, format!("flatpak:{}", item.app.id));
        }

        // Save the updated manifest
        manifest.save_repo()?;

        Ok(report)
    }

    fn is_empty(&self) -> bool {
        self.to_capture.is_empty()
    }
}
