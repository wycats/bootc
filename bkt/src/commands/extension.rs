//! GNOME extension command implementation.

use crate::context::PrMode;
use crate::manifest::GnomeExtensionsManifest;
use crate::manifest::ephemeral::{ChangeAction, ChangeDomain, EphemeralChange, EphemeralManifest};
use crate::output::Output;
use crate::pipeline::ExecutionPlan;
use crate::plan::{
    ExecuteContext, ExecutionReport, Operation, Plan, PlanContext, PlanSummary, Plannable, Verb,
};
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
        /// Skip validation that extension exists
        #[arg(long)]
        force: bool,
    },
    /// Remove a GNOME extension from the manifest
    Remove {
        /// Extension UUID to remove
        uuid: String,
    },
    /// List all GNOME extensions in the manifest
    List {
        /// Output format (table, json)
        #[arg(short, long, default_value = "table")]
        format: String,
    },
    /// Sync: enable extensions from manifest
    Sync,
    /// Capture enabled extensions to manifest
    Capture {
        /// Apply the plan immediately (default is preview only)
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

pub fn run(args: ExtensionArgs, plan: &ExecutionPlan) -> Result<()> {
    match args.action {
        ExtensionAction::Add { uuid, force } => {
            // Validate that the extension exists on extensions.gnome.org
            if !force {
                validate_gnome_extension(&uuid)?;
            }

            let system = GnomeExtensionsManifest::load_system()?;
            let mut user = GnomeExtensionsManifest::load_user()?;

            // Pre-compute state before any manifest modifications
            let already_in_manifest = system.contains(&uuid) || user.contains(&uuid);

            if plan.should_update_local_manifest() {
                if already_in_manifest {
                    Output::warning(format!("Extension already in manifest: {}", uuid));
                } else {
                    user.add(uuid.clone());
                    user.save_user()?;
                    Output::success(format!("Added to user manifest: {}", uuid));
                }
            } else if plan.dry_run {
                Output::dry_run(format!("Would add to user manifest: {}", uuid));
            }

            // Record ephemeral change if using --local (not in dry-run mode)
            if plan.pr_mode == PrMode::LocalOnly && !plan.dry_run && !already_in_manifest {
                let mut ephemeral = EphemeralManifest::load_validated()?;
                ephemeral.record(EphemeralChange::new(
                    ChangeDomain::Extension,
                    ChangeAction::Add,
                    &uuid,
                ));
                ephemeral.save()?;
            }

            // Enable if installed
            if plan.should_execute_locally() {
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
            } else if plan.dry_run {
                Output::dry_run(format!("Would enable extension: {}", uuid));
            }

            if plan.should_create_pr() {
                let mut system_manifest = GnomeExtensionsManifest::load_system()?;
                system_manifest.add(uuid.clone());
                let manifest_content = serde_json::to_string_pretty(&system_manifest)?;

                plan.maybe_create_pr(
                    "extension",
                    "add",
                    &uuid,
                    "gnome-extensions.json",
                    &manifest_content,
                )?;
            }
        }
        ExtensionAction::Remove { uuid } => {
            let mut user = GnomeExtensionsManifest::load_user()?;
            let system = GnomeExtensionsManifest::load_system()?;

            let in_system = system.contains(&uuid);
            if in_system && !user.contains(&uuid) && !plan.should_create_pr() {
                Output::info(format!(
                    "'{}' is in the system manifest; use --pr or --pr-only to remove from source",
                    uuid
                ));
            }

            if plan.should_update_local_manifest() {
                if user.remove(&uuid) {
                    user.save_user()?;
                    Output::success(format!("Removed from user manifest: {}", uuid));
                } else if !in_system {
                    Output::warning(format!("Extension not found in manifest: {}", uuid));
                }
            } else if plan.dry_run {
                Output::dry_run(format!("Would remove from user manifest: {}", uuid));
            }

            // Record ephemeral change if using --local (not in dry-run mode)
            if plan.pr_mode == PrMode::LocalOnly && !plan.dry_run {
                let mut ephemeral = EphemeralManifest::load_validated()?;
                ephemeral.record(EphemeralChange::new(
                    ChangeDomain::Extension,
                    ChangeAction::Remove,
                    &uuid,
                ));
                ephemeral.save()?;
            }

            // Disable if enabled
            if plan.should_execute_locally() {
                if is_enabled(&uuid) {
                    let spinner = Output::spinner(format!("Disabling {}...", uuid));
                    if disable_extension(&uuid)? {
                        spinner.finish_success(format!("Disabled {}", uuid));
                    } else {
                        spinner.finish_error(format!("Failed to disable {}", uuid));
                    }
                }
            } else if plan.dry_run && is_enabled(&uuid) {
                Output::dry_run(format!("Would disable extension: {}", uuid));
            }

            if plan.should_create_pr() {
                let mut system_manifest = GnomeExtensionsManifest::load_system()?;
                if system_manifest.remove(&uuid) {
                    let manifest_content = serde_json::to_string_pretty(&system_manifest)?;

                    plan.maybe_create_pr(
                        "extension",
                        "remove",
                        &uuid,
                        "gnome-extensions.json",
                        &manifest_content,
                    )?;
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

                for item in &merged.extensions {
                    let uuid = item.id();
                    let source = if user.contains(uuid) {
                        "user".yellow().to_string()
                    } else {
                        "system".dimmed().to_string()
                    };
                    let status = if is_enabled(uuid) {
                        if item.enabled() {
                            format!("{} enabled", "✓".green())
                        } else {
                            format!("{} enabled (should be disabled)", "⚠".red())
                        }
                    } else if is_installed(uuid) {
                        if item.enabled() {
                            format!("{} disabled", "○".yellow())
                        } else {
                            format!("{} disabled (config)", "○".dimmed())
                        }
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
        ExtensionAction::Sync => {
            // Use the new Plan-based implementation
            let cwd = std::env::current_dir()?;
            let plan_ctx = PlanContext::new(cwd, plan.clone());

            let sync_plan = ExtensionSyncCommand.plan(&plan_ctx)?;

            if sync_plan.is_empty() {
                Output::success("All extensions are already enabled.");
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
        ExtensionAction::Capture { apply } => {
            // Use the Plan-based capture implementation
            let cwd = std::env::current_dir()?;
            let plan_ctx = PlanContext::new(cwd, plan.clone());

            let capture_plan = ExtensionCaptureCommand.plan(&plan_ctx)?;

            if capture_plan.is_empty() {
                Output::success("All enabled extensions are already in the manifest.");
                return Ok(());
            }

            // Always show the plan
            print!("{}", capture_plan.describe());

            if plan.dry_run || !apply {
                if !apply && !plan.dry_run {
                    Output::hint("Use --apply to execute this plan.");
                }
                return Ok(());
            }

            // Execute the plan
            let mut exec_ctx = ExecuteContext::new(plan.clone());
            let report = capture_plan.execute(&mut exec_ctx)?;
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
    /// Extensions to disable (UUIDs).
    pub to_disable: Vec<String>,
    /// Extensions checked.
    pub checked: usize,
}

impl Plannable for ExtensionSyncCommand {
    type Plan = ExtensionSyncPlan;

    fn plan(&self, _ctx: &PlanContext) -> Result<Self::Plan> {
        // Load and merge manifests (read-only, no side effects)
        let system = GnomeExtensionsManifest::load_system()?;
        let user = GnomeExtensionsManifest::load_user()?;
        let merged = GnomeExtensionsManifest::merged(&system, &user);

        let mut to_enable = Vec::new();
        let mut to_disable = Vec::new();
        let mut checked = 0;

        for item in merged.extensions {
            let uuid = item.id().to_string();
            let should_be_enabled = item.enabled();
            checked += 1;

            if is_enabled(&uuid) {
                if !should_be_enabled {
                    to_disable.push(uuid);
                }
            } else if should_be_enabled {
                if is_installed(&uuid) {
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
        }

        Ok(ExtensionSyncPlan {
            to_enable,
            to_disable,
            checked,
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
        let _not_installed = self
            .to_enable
            .iter()
            .filter(|e| matches!(e.state, ExtensionState::NotInstalled))
            .count();

        let mut summary = PlanSummary::new(format!(
            "Extension Sync: {} to enable, {} to disable, {} checked",
            installable,
            self.to_disable.len(),
            self.checked
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

        for uuid in &self.to_disable {
            summary.add_operation(Operation::new(Verb::Disable, format!("extension:{}", uuid)));
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

        for uuid in self.to_disable {
            match disable_extension(&uuid) {
                Ok(true) => {
                    report.record_success(Verb::Disable, format!("extension:{}", uuid));
                }
                Ok(false) => {
                    report.record_failure(
                        Verb::Disable,
                        format!("extension:{}", uuid),
                        "gnome-extensions disable failed",
                    );
                }
                Err(e) => {
                    report.record_failure(
                        Verb::Disable,
                        format!("extension:{}", uuid),
                        e.to_string(),
                    );
                }
            }
        }

        Ok(report)
    }

    fn is_empty(&self) -> bool {
        // Empty if no extensions need enabling (ignore not-installed ones) AND nothing to disable
        let to_enable_count = self
            .to_enable
            .iter()
            .filter(|e| matches!(e.state, ExtensionState::Disabled))
            .count();

        to_enable_count == 0 && self.to_disable.is_empty()
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

/// Get list of all installed GNOME extension UUIDs from the system.
fn get_installed_extensions_list() -> Vec<String> {
    let output = std::process::Command::new("gnome-extensions")
        .arg("list")
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
    /// Whether the extension is enabled.
    pub enabled: bool,
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
        // Get all installed extensions and enabled ones
        let installed = get_installed_extensions_list();
        let enabled: std::collections::HashSet<_> = get_enabled_extensions().into_iter().collect();

        // Load manifests to see what's already tracked
        let system = GnomeExtensionsManifest::load_system()?;
        let user = GnomeExtensionsManifest::load_user()?;
        let merged = GnomeExtensionsManifest::merged(&system, &user);

        let mut to_capture = Vec::new();
        let mut already_in_manifest = 0;

        for uuid in installed {
            let is_enabled_physically = enabled.contains(&uuid);

            if let Some(existing) = merged.get(&uuid) {
                // Check if the manifest state matches the physical system state.
                // If both match, the extension is already correctly tracked - skip it.
                if existing.enabled() == is_enabled_physically {
                    already_in_manifest += 1;
                    continue;
                }
                // If state differs (e.g., system manifest says enabled but physically disabled),
                // we fall through to capture it. This will add/update the user manifest to
                // explicitly record the desired state, overriding the system default.
            }

            // Capture this extension:
            // - Not in manifest yet, OR
            // - In manifest but state differs from physical state
            to_capture.push(ExtensionToCapture {
                enabled: is_enabled_physically,
                uuid,
            });
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
            let desc = if ext.enabled {
                format!("extension:{}", ext.uuid)
            } else {
                format!("extension:{} (disabled)", ext.uuid)
            };
            summary.add_operation(Operation::new(Verb::Capture, desc));
        }

        summary
    }

    fn execute(self, _ctx: &mut ExecuteContext) -> Result<ExecutionReport> {
        let mut report = ExecutionReport::new();

        // Load user manifest (we add captured extensions there)
        let mut user = GnomeExtensionsManifest::load_user()?;

        for ext in self.to_capture {
            let item = if ext.enabled {
                crate::manifest::extension::ExtensionItem::Uuid(ext.uuid.clone())
            } else {
                crate::manifest::extension::ExtensionItem::Object(
                    crate::manifest::extension::ExtensionConfig {
                        id: ext.uuid.clone(),
                        enabled: false,
                    },
                )
            };

            if user.add(item) {
                let desc = if ext.enabled {
                    format!("extension:{}", ext.uuid)
                } else {
                    format!("extension:{} (disabled)", ext.uuid)
                };
                report.record_success(Verb::Capture, desc);
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
