//! GNOME extension command implementation.

use crate::manifest::GnomeExtensionsManifest;
use crate::output::Output;
use crate::pipeline::ExecutionPlan;
use crate::plan::{
    ExecuteContext, ExecutionReport, Operation, Plan, PlanContext, PlanSummary, Plannable, Verb,
};
use crate::pr::{PrChange, run_pr_workflow};
use crate::validation::validate_gnome_extension;
use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use owo_colors::OwoColorize;
use std::process::Command;

#[derive(Debug, Args)]
pub struct ExtensionArgs {
    #[command(subcommand)]
    pub action: ExtensionAction,
}

#[derive(Debug, Subcommand)]
pub enum ExtensionAction {
    /// Add a GNOME extension to the manifest
    Add {
        /// Extension UUID (e.g., dash-to-dock@micxgx.gmail.com)
        uuid: String,
        /// Create a PR with the change
        #[arg(long)]
        pr: bool,
        /// Skip pre-flight checks for PR workflow
        #[arg(long)]
        skip_preflight: bool,
        /// Skip validation that extension exists
        #[arg(long)]
        force: bool,
    },
    /// Remove a GNOME extension from the manifest
    Remove {
        /// Extension UUID to remove
        uuid: String,
        /// Create a PR with the change
        #[arg(long)]
        pr: bool,
        /// Skip pre-flight checks for PR workflow
        #[arg(long)]
        skip_preflight: bool,
    },
    /// List all GNOME extensions in the manifest
    List {
        /// Output format (table, json)
        #[arg(short, long, default_value = "table")]
        format: String,
    },
    /// Sync: enable extensions from manifest
    Sync {
        /// Show what would be done without making changes
        #[arg(long)]
        dry_run: bool,
    },
    /// Capture enabled extensions to manifest
    Capture {
        /// Show what would be done without making changes
        #[arg(long)]
        dry_run: bool,
        /// Apply the plan immediately
        #[arg(long)]
        apply: bool,
    },
}

/// Check if an extension is installed.
fn is_installed(uuid: &str) -> bool {
    Command::new("gnome-extensions")
        .args(["info", uuid])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Check if an extension is enabled.
fn is_enabled(uuid: &str) -> bool {
    Command::new("gnome-extensions")
        .args(["info", uuid])
        .output()
        .map(|o| {
            o.status.success() && String::from_utf8_lossy(&o.stdout).contains("State: ENABLED")
        })
        .unwrap_or(false)
}

/// Enable an extension.
fn enable_extension(uuid: &str) -> Result<bool> {
    let status = Command::new("gnome-extensions")
        .args(["enable", uuid])
        .status()
        .context("Failed to run gnome-extensions enable")?;
    Ok(status.success())
}

/// Disable an extension.
#[allow(dead_code)]
fn disable_extension(uuid: &str) -> Result<bool> {
    let status = Command::new("gnome-extensions")
        .args(["disable", uuid])
        .status()
        .context("Failed to run gnome-extensions disable")?;
    Ok(status.success())
}

pub fn run(args: ExtensionArgs, _plan: &ExecutionPlan) -> Result<()> {
    // TODO: Migrate to use `ExecutionPlan` instead of per-command flags.
    // The `_plan` parameter is intentionally unused and reserved for future use
    // after this migration.
    match args.action {
        ExtensionAction::Add {
            uuid,
            pr,
            skip_preflight,
            force,
        } => {
            // Validate that the extension exists on extensions.gnome.org
            if !force {
                validate_gnome_extension(&uuid)?;
            }

            let system = GnomeExtensionsManifest::load_system()?;
            let mut user = GnomeExtensionsManifest::load_user()?;

            if system.contains(&uuid) || user.contains(&uuid) {
                Output::warning(format!("Extension already in manifest: {}", uuid));
            } else {
                user.add(uuid.clone());
                user.save_user()?;
                Output::success(format!("Added to user manifest: {}", uuid));
            }

            // Enable if installed
            if is_installed(&uuid) {
                if !is_enabled(&uuid) {
                    let spinner = Output::spinner(format!("Enabling {}...", uuid));
                    if enable_extension(&uuid)? {
                        spinner.finish_success(format!("Enabled {}", uuid));
                    } else {
                        spinner.finish_error(format!("Failed to enable {}", uuid));
                    }
                } else {
                    Output::info(format!("Already enabled: {}", uuid));
                }
            } else {
                Output::hint(
                    "Extension not installed. Install via Extension Manager or extensions.gnome.org",
                );
            }

            if pr {
                let mut system_manifest = GnomeExtensionsManifest::load_system()?;
                system_manifest.add(uuid.clone());
                let manifest_content = serde_json::to_string_pretty(&system_manifest)?;

                let change = PrChange {
                    manifest_type: "extension".to_string(),
                    action: "add".to_string(),
                    name: uuid.clone(),
                    manifest_file: "gnome-extensions.json".to_string(),
                };
                run_pr_workflow(&change, &manifest_content, skip_preflight)?;
            }
        }
        ExtensionAction::Remove {
            uuid,
            pr,
            skip_preflight,
        } => {
            let mut user = GnomeExtensionsManifest::load_user()?;
            let system = GnomeExtensionsManifest::load_system()?;

            let in_system = system.contains(&uuid);
            if in_system && !user.contains(&uuid) {
                Output::info(format!(
                    "'{}' is in the system manifest; use --pr to remove from source",
                    uuid
                ));
            }

            if user.remove(&uuid) {
                user.save_user()?;
                Output::success(format!("Removed from user manifest: {}", uuid));
            } else if !in_system {
                Output::warning(format!("Extension not found in manifest: {}", uuid));
            }

            // Disable if enabled
            if is_enabled(&uuid) {
                let spinner = Output::spinner(format!("Disabling {}...", uuid));
                if disable_extension(&uuid)? {
                    spinner.finish_success(format!("Disabled {}", uuid));
                } else {
                    spinner.finish_error(format!("Failed to disable {}", uuid));
                }
            }

            if pr {
                let mut system_manifest = GnomeExtensionsManifest::load_system()?;
                if system_manifest.remove(&uuid) {
                    let manifest_content = serde_json::to_string_pretty(&system_manifest)?;

                    let change = PrChange {
                        manifest_type: "extension".to_string(),
                        action: "remove".to_string(),
                        name: uuid.clone(),
                        manifest_file: "gnome-extensions.json".to_string(),
                    };
                    run_pr_workflow(&change, &manifest_content, skip_preflight)?;
                } else {
                    Output::info(format!("'{}' not in system manifest, no PR needed", uuid));
                }
            }
        }
        ExtensionAction::List { format } => {
            let system = GnomeExtensionsManifest::load_system()?;
            let user = GnomeExtensionsManifest::load_user()?;
            let merged = GnomeExtensionsManifest::merged(&system, &user);

            if format == "json" {
                println!("{}", serde_json::to_string_pretty(&merged)?);
            } else {
                if merged.extensions.is_empty() {
                    Output::info("No extensions in manifest.");
                    return Ok(());
                }

                Output::subheader("GNOME EXTENSIONS:");
                println!(
                    "{:<50} {:<10} {}",
                    "UUID".cyan(),
                    "SOURCE".cyan(),
                    "STATUS".cyan()
                );
                Output::separator();

                for uuid in &merged.extensions {
                    let source = if user.contains(uuid) {
                        "user".yellow().to_string()
                    } else {
                        "system".dimmed().to_string()
                    };
                    let status = if is_enabled(uuid) {
                        format!("{} enabled", "✓".green())
                    } else if is_installed(uuid) {
                        format!("{} disabled", "○".yellow())
                    } else {
                        format!("{} not installed", "✗".red())
                    };
                    println!("{:<50} {:<10} {}", uuid, source, status);
                }

                Output::blank();
                Output::info(format!(
                    "{} extensions ({} system, {} user)",
                    merged.extensions.len(),
                    system.extensions.len(),
                    user.extensions.len()
                ));
            }
        }
        ExtensionAction::Sync { dry_run } => {
            // Use the new Plan-based implementation
            let cwd = std::env::current_dir()?;
            let plan_ctx = PlanContext::new(
                cwd,
                ExecutionPlan {
                    dry_run,
                    ..Default::default()
                },
            );

            let plan = ExtensionSyncCommand.plan(&plan_ctx)?;

            if plan.is_empty() {
                Output::success("All extensions are already enabled.");
                return Ok(());
            }

            // Always show the plan
            print!("{}", plan.describe());

            if dry_run {
                return Ok(());
            }

            // Execute the plan
            let mut exec_ctx = ExecuteContext::new(ExecutionPlan {
                dry_run: false,
                ..Default::default()
            });
            let report = plan.execute(&mut exec_ctx)?;
            print!("{}", report);
        }
        ExtensionAction::Capture { dry_run, apply } => {
            // Use the Plan-based capture implementation
            let cwd = std::env::current_dir()?;
            let plan_ctx = PlanContext::new(
                cwd,
                ExecutionPlan {
                    dry_run,
                    ..Default::default()
                },
            );

            let plan = ExtensionCaptureCommand.plan(&plan_ctx)?;

            if plan.is_empty() {
                Output::success("All enabled extensions are already in the manifest.");
                return Ok(());
            }

            // Always show the plan
            print!("{}", plan.describe());

            if dry_run || !apply {
                if !apply {
                    Output::hint("Use --apply to execute this plan.");
                }
                return Ok(());
            }

            // Execute the plan
            let mut exec_ctx = ExecuteContext::new(ExecutionPlan {
                dry_run: false,
                ..Default::default()
            });
            let report = plan.execute(&mut exec_ctx)?;
            print!("{}", report);
        }
    }
    Ok(())
}

// ============================================================================
// Plan-based Extension Sync Implementation
// ============================================================================

/// State of an extension for planning.
#[derive(Debug, Clone)]
pub enum ExtensionState {
    /// Extension is installed but not enabled.
    Disabled,
    /// Extension is not installed.
    NotInstalled,
}

/// An extension that needs action.
#[derive(Debug, Clone)]
pub struct ExtensionToSync {
    /// The extension UUID.
    pub uuid: String,
    /// Current state.
    pub state: ExtensionState,
}

/// Command to sync extensions from manifests.
pub struct ExtensionSyncCommand;

/// Plan for syncing extensions.
pub struct ExtensionSyncPlan {
    /// Extensions to enable.
    pub to_enable: Vec<ExtensionToSync>,
    /// Extensions already enabled.
    pub already_enabled: usize,
}

impl Plannable for ExtensionSyncCommand {
    type Plan = ExtensionSyncPlan;

    fn plan(&self, _ctx: &PlanContext) -> Result<Self::Plan> {
        // Load and merge manifests (read-only, no side effects)
        let system = GnomeExtensionsManifest::load_system()?;
        let user = GnomeExtensionsManifest::load_user()?;
        let merged = GnomeExtensionsManifest::merged(&system, &user);

        let mut to_enable = Vec::new();
        let mut already_enabled = 0;

        for uuid in merged.extensions {
            if is_enabled(&uuid) {
                already_enabled += 1;
            } else if is_installed(&uuid) {
                to_enable.push(ExtensionToSync {
                    uuid,
                    state: ExtensionState::Disabled,
                });
            } else {
                to_enable.push(ExtensionToSync {
                    uuid,
                    state: ExtensionState::NotInstalled,
                });
            }
        }

        Ok(ExtensionSyncPlan {
            to_enable,
            already_enabled,
        })
    }
}

impl Plan for ExtensionSyncPlan {
    fn describe(&self) -> PlanSummary {
        let installable = self
            .to_enable
            .iter()
            .filter(|e| matches!(e.state, ExtensionState::Disabled))
            .count();
        let not_installed = self
            .to_enable
            .iter()
            .filter(|e| matches!(e.state, ExtensionState::NotInstalled))
            .count();

        let mut summary = PlanSummary::new(format!(
            "Extension Sync: {} to enable, {} already enabled, {} not installed",
            installable, self.already_enabled, not_installed
        ));

        for ext in &self.to_enable {
            match ext.state {
                ExtensionState::Disabled => {
                    summary.add_operation(Operation::new(
                        Verb::Enable,
                        format!("extension:{}", ext.uuid),
                    ));
                }
                ExtensionState::NotInstalled => {
                    summary.add_operation(Operation::with_details(
                        Verb::Skip,
                        format!("extension:{}", ext.uuid),
                        "not installed",
                    ));
                }
            }
        }

        summary
    }

    fn execute(self, _ctx: &mut ExecuteContext) -> Result<ExecutionReport> {
        let mut report = ExecutionReport::new();

        for ext in self.to_enable {
            match ext.state {
                ExtensionState::Disabled => match enable_extension(&ext.uuid) {
                    Ok(true) => {
                        report.record_success(Verb::Enable, format!("extension:{}", ext.uuid));
                    }
                    Ok(false) => {
                        report.record_failure(
                            Verb::Enable,
                            format!("extension:{}", ext.uuid),
                            "gnome-extensions enable failed",
                        );
                    }
                    Err(e) => {
                        report.record_failure(
                            Verb::Enable,
                            format!("extension:{}", ext.uuid),
                            e.to_string(),
                        );
                    }
                },
                ExtensionState::NotInstalled => {
                    // Skip, don't record anything for not-installed extensions
                }
            }
        }

        Ok(report)
    }

    fn is_empty(&self) -> bool {
        // Empty if no extensions need enabling (ignore not-installed ones)
        self.to_enable
            .iter()
            .filter(|e| matches!(e.state, ExtensionState::Disabled))
            .count()
            == 0
    }
}

// ============================================================================
// Plan-based Extension Capture Implementation
// ============================================================================

/// Get list of enabled GNOME extension UUIDs from the system.
fn get_enabled_extensions() -> Vec<String> {
    let output = std::process::Command::new("gnome-extensions")
        .args(["list", "--enabled"])
        .output();

    match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .lines()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

/// An extension to capture (add to manifest).
#[derive(Debug, Clone)]
pub struct ExtensionToCapture {
    /// The extension UUID.
    pub uuid: String,
}

/// Command to capture enabled extensions to manifest.
pub struct ExtensionCaptureCommand;

/// Plan for capturing extensions.
pub struct ExtensionCapturePlan {
    /// Extensions to add to manifest.
    pub to_capture: Vec<ExtensionToCapture>,
    /// Extensions already in manifest.
    pub already_in_manifest: usize,
}

impl Plannable for ExtensionCaptureCommand {
    type Plan = ExtensionCapturePlan;

    fn plan(&self, _ctx: &PlanContext) -> Result<Self::Plan> {
        // Get currently enabled extensions on the system
        let enabled = get_enabled_extensions();

        // Load manifests to see what's already tracked
        let system = GnomeExtensionsManifest::load_system()?;
        let user = GnomeExtensionsManifest::load_user()?;
        let merged = GnomeExtensionsManifest::merged(&system, &user);

        let mut to_capture = Vec::new();
        let mut already_in_manifest = 0;

        for uuid in enabled {
            if merged.contains(&uuid) {
                already_in_manifest += 1;
            } else {
                to_capture.push(ExtensionToCapture { uuid });
            }
        }

        // Sort for consistent output
        to_capture.sort_by(|a, b| a.uuid.cmp(&b.uuid));

        Ok(ExtensionCapturePlan {
            to_capture,
            already_in_manifest,
        })
    }
}

impl Plan for ExtensionCapturePlan {
    fn describe(&self) -> PlanSummary {
        let mut summary = PlanSummary::new(format!(
            "Extension Capture: {} to add, {} already in manifest",
            self.to_capture.len(),
            self.already_in_manifest
        ));

        for ext in &self.to_capture {
            summary.add_operation(Operation::new(
                Verb::Capture,
                format!("extension:{}", ext.uuid),
            ));
        }

        summary
    }

    fn execute(self, _ctx: &mut ExecuteContext) -> Result<ExecutionReport> {
        let mut report = ExecutionReport::new();

        // Load user manifest (we add captured extensions there)
        let mut user = GnomeExtensionsManifest::load_user()?;

        for ext in self.to_capture {
            if user.add(ext.uuid.clone()) {
                report.record_success(Verb::Capture, format!("extension:{}", ext.uuid));
            } else {
                // Should not happen since we checked in planning, but handle gracefully
                report.record_failure(
                    Verb::Capture,
                    format!("extension:{}", ext.uuid),
                    "already in manifest",
                );
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
