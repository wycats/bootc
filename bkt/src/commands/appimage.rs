//! AppImage command implementation via GearLever integration.
//!
//! This module provides commands for managing AppImages through GearLever.
//! The manifest format is simplified and backend-agnostic.

use crate::context::PrMode;
use crate::manifest::ephemeral::{ChangeAction, ChangeDomain, EphemeralChange, EphemeralManifest};
use crate::manifest::{AppImageApp, AppImageAppsManifest, GearLeverNativeManifest};
use crate::output::Output;
use crate::pipeline::ExecutionPlan;
use crate::plan::{
    ExecuteContext, ExecutionReport, Operation, Plan, PlanContext, PlanSummary, Plannable, Verb,
};
use crate::pr::ensure_repo;
use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use owo_colors::OwoColorize;
use std::collections::HashSet;

#[derive(Debug, Args)]
pub struct AppImageArgs {
    #[command(subcommand)]
    pub action: AppImageAction,
}

#[derive(Debug, Subcommand)]
pub enum AppImageAction {
    /// Add an AppImage from GitHub releases
    Add {
        /// GitHub repository in "github:owner/repo" format
        repo: String,
        /// Asset filename pattern (glob supported)
        #[arg(short, long)]
        asset: String,
        /// Human-readable name (defaults to repo name)
        #[arg(short, long)]
        name: Option<String>,
        /// Include prereleases/nightlies
        #[arg(long)]
        prereleases: bool,
    },
    /// Remove an AppImage from the manifest
    Remove {
        /// App name to remove
        name: String,
    },
    /// Disable an AppImage (keeps in manifest but won't sync)
    Disable {
        /// App name to disable
        name: String,
    },
    /// Enable a previously disabled AppImage
    Enable {
        /// App name to enable
        name: String,
    },
    /// List all AppImages in the manifest
    List {
        /// Output format (table, json)
        #[arg(short, long, default_value = "table")]
        format: String,
    },
    /// Sync: update GearLever config from manifest
    Sync {
        /// Keep user-added apps in GearLever (don't prune)
        #[arg(long)]
        keep: bool,
    },
    /// Capture installed AppImages from GearLever to manifest
    Capture {
        /// Apply the plan immediately (default is preview only)
        #[arg(long)]
        apply: bool,
    },
}

/// Parse a "github:owner/repo" string into "owner/repo".
fn parse_github_repo(input: &str) -> Result<String> {
    let repo = input
        .strip_prefix("github:")
        .or_else(|| input.strip_prefix("gh:"))
        .unwrap_or(input);

    // Validate it looks like owner/repo
    if !repo.contains('/') || repo.starts_with('/') || repo.ends_with('/') {
        bail!(
            "Invalid repository format: '{}'. Expected 'github:owner/repo' or 'owner/repo'.",
            input
        );
    }

    Ok(repo.to_string())
}

/// Infer app name from repo (e.g., "OrcaSlicer/OrcaSlicer" -> "OrcaSlicer").
fn infer_name_from_repo(repo: &str) -> String {
    repo.split('/').next_back().unwrap_or(repo).to_string()
}

/// Get the manifest path from the repo.
fn get_manifest_path() -> Result<std::path::PathBuf> {
    let repo_path = ensure_repo()?;
    Ok(repo_path.join("manifests"))
}

pub fn run(args: AppImageArgs, plan: &ExecutionPlan) -> Result<()> {
    match args.action {
        AppImageAction::Add {
            repo,
            asset,
            name,
            prereleases,
        } => {
            let repo = parse_github_repo(&repo)?;
            let name = name.unwrap_or_else(|| infer_name_from_repo(&repo));
            let manifests_dir = get_manifest_path()?;

            let mut manifest = AppImageAppsManifest::load_from_dir(&manifests_dir)?;

            let already_exists = manifest.find(&name).is_some();
            if already_exists {
                Output::warning(format!("AppImage '{}' already in manifest, updating", name));
            }

            let app = AppImageApp {
                name: name.clone(),
                repo: repo.clone(),
                asset,
                prereleases,
                disabled: false,
            };

            if plan.should_update_local_manifest() {
                manifest.upsert(app.clone());
                manifest.save_to_dir(&manifests_dir)?;
                Output::success(format!("Added AppImage '{}' (github:{})", name, repo));
            } else if plan.dry_run {
                Output::dry_run(format!("Would add AppImage '{}'", name));
            }

            // Record ephemeral change if using --local (not in dry-run mode)
            if plan.pr_mode == PrMode::LocalOnly && !plan.dry_run && !already_exists {
                let mut ephemeral = EphemeralManifest::load_validated()?;
                ephemeral.record(EphemeralChange::new(
                    ChangeDomain::AppImage,
                    ChangeAction::Add,
                    &name,
                ));
                ephemeral.save()?;
            }

            // Sync to GearLever if executing locally
            if plan.should_execute_locally() && !plan.dry_run {
                let mut gearlever = GearLeverNativeManifest::load()?;
                gearlever.upsert(&app);
                gearlever.save()?;
                Output::success(format!("Synced '{}' to GearLever", name));
            }
        }
        AppImageAction::Remove { name } => {
            let manifests_dir = get_manifest_path()?;
            let mut manifest = AppImageAppsManifest::load_from_dir(&manifests_dir)?;

            if manifest.find(&name).is_none() {
                Output::warning(format!("AppImage '{}' not found in manifest", name));
                return Ok(());
            }

            if plan.should_update_local_manifest() {
                manifest.remove(&name);
                manifest.save_to_dir(&manifests_dir)?;
                Output::success(format!("Removed AppImage '{}' from manifest", name));
            } else if plan.dry_run {
                Output::dry_run(format!("Would remove AppImage '{}'", name));
            }

            // Record ephemeral change if using --local
            if plan.pr_mode == PrMode::LocalOnly && !plan.dry_run {
                let mut ephemeral = EphemeralManifest::load_validated()?;
                ephemeral.record(EphemeralChange::new(
                    ChangeDomain::AppImage,
                    ChangeAction::Remove,
                    &name,
                ));
                ephemeral.save()?;
            }

            // Remove from GearLever if executing locally
            if plan.should_execute_locally() && !plan.dry_run {
                let mut gearlever = GearLeverNativeManifest::load()?;
                if gearlever.remove_by_name(&name) {
                    gearlever.save()?;
                    Output::success(format!("Removed '{}' from GearLever", name));
                }
            }
        }
        AppImageAction::Disable { name } => {
            let manifests_dir = get_manifest_path()?;
            let mut manifest = AppImageAppsManifest::load_from_dir(&manifests_dir)?;

            if let Some(app) = manifest.find_mut(&name) {
                if app.disabled {
                    Output::info(format!("AppImage '{}' is already disabled", name));
                } else {
                    app.disabled = true;
                    if plan.dry_run {
                        Output::dry_run(format!("Would disable AppImage '{}'", name));
                    } else {
                        manifest.save_to_dir(&manifests_dir)?;
                        Output::success(format!("Disabled AppImage '{}'", name));
                    }
                }
            } else {
                Output::warning(format!("AppImage '{}' not found in manifest", name));
            }
        }
        AppImageAction::Enable { name } => {
            let manifests_dir = get_manifest_path()?;
            let mut manifest = AppImageAppsManifest::load_from_dir(&manifests_dir)?;

            if let Some(app) = manifest.find_mut(&name) {
                if !app.disabled {
                    Output::info(format!("AppImage '{}' is already enabled", name));
                } else {
                    app.disabled = false;
                    if plan.dry_run {
                        Output::dry_run(format!("Would enable AppImage '{}'", name));
                    } else {
                        manifest.save_to_dir(&manifests_dir)?;
                        Output::success(format!("Enabled AppImage '{}'", name));
                    }
                }
            } else {
                Output::warning(format!("AppImage '{}' not found in manifest", name));
            }
        }
        AppImageAction::List { format } => {
            let manifests_dir = get_manifest_path()?;
            let manifest = AppImageAppsManifest::load_from_dir(&manifests_dir)?;

            if manifest.apps.is_empty() {
                Output::info("No AppImages in manifest");
                return Ok(());
            }

            if format == "json" {
                println!("{}", serde_json::to_string_pretty(&manifest.apps)?);
            } else {
                println!(
                    "{:30} {:30} {:10} {}",
                    "Name".bold(),
                    "Repository".bold(),
                    "Prerelease".bold(),
                    "Status".bold()
                );
                println!("{}", "-".repeat(80));
                for app in &manifest.apps {
                    let status = if app.disabled {
                        "disabled".dimmed().to_string()
                    } else {
                        "enabled".green().to_string()
                    };
                    println!(
                        "{:30} {:30} {:10} {}",
                        app.name,
                        app.repo,
                        if app.prereleases { "yes" } else { "no" },
                        status
                    );
                }
            }
        }
        AppImageAction::Sync { keep } => {
            let cmd = AppImageSyncCommand {
                keep_unmanaged: keep,
            };
            let plan_ctx = PlanContext::new(std::env::current_dir()?, plan.clone());
            let sync_plan = cmd.plan(&plan_ctx)?;

            if sync_plan.is_empty() {
                Output::success("GearLever config is in sync with manifest");
                return Ok(());
            }

            print!("{}", sync_plan.describe());

            if plan.dry_run {
                Output::info("Run without --dry-run to apply these changes.");
                return Ok(());
            }

            let mut exec_ctx = ExecuteContext::new(plan.clone());
            let report = sync_plan.execute(&mut exec_ctx)?;
            print!("{}", report);
        }
        AppImageAction::Capture { apply } => {
            let cmd = AppImageCaptureCommand;
            let plan_ctx = PlanContext::new(std::env::current_dir()?, plan.clone());
            let capture_plan = cmd.plan(&plan_ctx)?;

            if capture_plan.is_empty() {
                Output::success("No new AppImages to capture");
                return Ok(());
            }

            print!("{}", capture_plan.describe());

            if !apply {
                Output::info("Run with --apply to add these to the manifest.");
                return Ok(());
            }

            if plan.dry_run {
                Output::info("Run without --dry-run to apply these changes.");
                return Ok(());
            }

            let mut exec_ctx = ExecuteContext::new(plan.clone());
            let report = capture_plan.execute(&mut exec_ctx)?;
            print!("{}", report);
        }
    }
    Ok(())
}

// ============================================================================
// Plan-based AppImage Sync Implementation
// ============================================================================

/// Command to sync AppImages to GearLever.
pub struct AppImageSyncCommand {
    /// Whether to keep user-added apps in GearLever.
    pub keep_unmanaged: bool,
}

/// An AppImage that needs to be synced.
#[derive(Debug, Clone)]
pub struct AppImageToSync {
    /// The app to sync.
    pub app: AppImageApp,
    /// Whether this is an add or update.
    pub is_update: bool,
}

/// Plan for syncing AppImages to GearLever.
pub struct AppImageSyncPlan {
    /// Apps to add/update in GearLever.
    pub to_sync: Vec<AppImageToSync>,
    /// Apps to remove from GearLever (pruning).
    pub to_remove: Vec<String>,
    /// Apps already in sync.
    pub already_synced: usize,
}

impl Plannable for AppImageSyncCommand {
    type Plan = AppImageSyncPlan;

    fn plan(&self, _ctx: &PlanContext) -> Result<Self::Plan> {
        // Load our manifest from the repo
        let manifests_dir = get_manifest_path()?;
        let manifest = AppImageAppsManifest::load_from_dir(&manifests_dir)?;

        // Load GearLever's current state
        let gearlever = GearLeverNativeManifest::load()?;

        let mut to_sync = Vec::new();
        let mut already_synced = 0;

        // Build set of manifest app names for pruning check
        let manifest_names: HashSet<_> = manifest.enabled_apps().map(|a| a.name.clone()).collect();

        // Check each enabled app in our manifest
        for app in manifest.enabled_apps() {
            if let Some(existing) = gearlever.find_by_name(&app.name) {
                // Check if config matches
                let expected = app.to_gearlever_entry();
                if existing.update_url != expected.update_url
                    || existing.update_manager_config.allow_prereleases
                        != expected.update_manager_config.allow_prereleases
                {
                    to_sync.push(AppImageToSync {
                        app: app.clone(),
                        is_update: true,
                    });
                } else {
                    already_synced += 1;
                }
            } else {
                // Not in GearLever, need to add
                to_sync.push(AppImageToSync {
                    app: app.clone(),
                    is_update: false,
                });
            }
        }

        // Find apps to prune (in GearLever but not in manifest)
        let to_remove = if self.keep_unmanaged {
            Vec::new()
        } else {
            gearlever
                .entries
                .values()
                .filter(|e| !manifest_names.contains(&e.name))
                .map(|e| e.name.clone())
                .collect()
        };

        Ok(AppImageSyncPlan {
            to_sync,
            to_remove,
            already_synced,
        })
    }
}

impl Plan for AppImageSyncPlan {
    fn describe(&self) -> PlanSummary {
        let mut summary = PlanSummary::new(format!(
            "AppImage Sync: {} to sync, {} to remove, {} already synced",
            self.to_sync.len(),
            self.to_remove.len(),
            self.already_synced
        ));

        for item in &self.to_sync {
            let verb = if item.is_update {
                Verb::Update
            } else {
                Verb::Install
            };
            summary.add_operation(Operation::with_details(
                verb,
                format!("appimage:{}", item.app.name),
                format!("github:{}", item.app.repo),
            ));
        }

        for name in &self.to_remove {
            summary.add_operation(Operation::new(Verb::Remove, format!("appimage:{}", name)));
        }

        summary
    }

    fn execute(self, ctx: &mut ExecuteContext) -> Result<ExecutionReport> {
        let mut report = ExecutionReport::new();

        // Load current GearLever state
        let mut gearlever = GearLeverNativeManifest::load()?;

        // Add/update apps
        for item in &self.to_sync {
            gearlever.upsert(&item.app);
            let verb = if item.is_update {
                Verb::Update
            } else {
                Verb::Install
            };
            report.record_success_and_notify(ctx, verb, format!("appimage:{}", item.app.name));
        }

        // Remove apps (pruning)
        for name in &self.to_remove {
            gearlever.remove_by_name(name);
            report.record_success_and_notify(ctx, Verb::Remove, format!("appimage:{}", name));
        }

        // Save the updated config
        if !self.to_sync.is_empty() || !self.to_remove.is_empty() {
            gearlever
                .save()
                .context("Failed to save GearLever config")?;
        }

        Ok(report)
    }

    fn is_empty(&self) -> bool {
        self.to_sync.is_empty() && self.to_remove.is_empty()
    }
}

// ============================================================================
// Plan-based AppImage Capture Implementation
// ============================================================================

/// Command to capture AppImages from GearLever.
pub struct AppImageCaptureCommand;

/// An AppImage to capture.
#[derive(Debug, Clone)]
pub struct AppImageToCapture {
    /// The app to add to manifest.
    pub app: AppImageApp,
}

/// Plan for capturing AppImages from GearLever.
pub struct AppImageCapturePlan {
    /// Apps to add to manifest.
    pub to_capture: Vec<AppImageToCapture>,
    /// Apps already in manifest.
    pub already_in_manifest: usize,
}

impl Plannable for AppImageCaptureCommand {
    type Plan = AppImageCapturePlan;

    fn plan(&self, _ctx: &PlanContext) -> Result<Self::Plan> {
        // Load GearLever's current state
        let gearlever = GearLeverNativeManifest::load()?;

        // Load our manifest from the repo
        let manifests_dir = get_manifest_path()?;
        let manifest = AppImageAppsManifest::load_from_dir(&manifests_dir)?;

        let mut to_capture = Vec::new();
        let mut already_in_manifest = 0;

        for app in gearlever.to_appimage_apps() {
            if manifest.find(&app.name).is_some() {
                already_in_manifest += 1;
            } else {
                to_capture.push(AppImageToCapture { app });
            }
        }

        // Sort for consistent output
        to_capture.sort_by(|a, b| a.app.name.cmp(&b.app.name));

        Ok(AppImageCapturePlan {
            to_capture,
            already_in_manifest,
        })
    }
}

impl Plan for AppImageCapturePlan {
    fn describe(&self) -> PlanSummary {
        let mut summary = PlanSummary::new(format!(
            "AppImage Capture: {} to add, {} already in manifest",
            self.to_capture.len(),
            self.already_in_manifest
        ));

        for item in &self.to_capture {
            summary.add_operation(Operation::with_details(
                Verb::Capture,
                format!("appimage:{}", item.app.name),
                format!("github:{}", item.app.repo),
            ));
        }

        summary
    }

    fn execute(self, ctx: &mut ExecuteContext) -> Result<ExecutionReport> {
        let mut report = ExecutionReport::new();

        // Load manifest from the repo
        let manifests_dir = get_manifest_path()?;
        let mut manifest = AppImageAppsManifest::load_from_dir(&manifests_dir)?;

        for item in self.to_capture {
            manifest.upsert(item.app.clone());
            report.record_success_and_notify(
                ctx,
                Verb::Capture,
                format!("appimage:{}", item.app.name),
            );
        }

        // Save updated manifest
        manifest.save_to_dir(&manifests_dir)?;

        Ok(report)
    }

    fn is_empty(&self) -> bool {
        self.to_capture.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_github_repo() {
        assert_eq!(
            parse_github_repo("github:owner/repo").unwrap(),
            "owner/repo"
        );
        assert_eq!(parse_github_repo("gh:owner/repo").unwrap(), "owner/repo");
        assert_eq!(parse_github_repo("owner/repo").unwrap(), "owner/repo");

        assert!(parse_github_repo("invalid").is_err());
        assert!(parse_github_repo("/repo").is_err());
        assert!(parse_github_repo("owner/").is_err());
    }

    #[test]
    fn test_infer_name_from_repo() {
        assert_eq!(infer_name_from_repo("OrcaSlicer/OrcaSlicer"), "OrcaSlicer");
        assert_eq!(infer_name_from_repo("ferdium/ferdium-app"), "ferdium-app");
    }
}
