//! Shim command implementation.

use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use owo_colors::OwoColorize;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use crate::context::PrMode;
use crate::manifest::ephemeral::{ChangeAction, ChangeDomain, EphemeralChange, EphemeralManifest};
use crate::manifest::{Shim, ShimsManifest};
use crate::output::Output;
use crate::pipeline::ExecutionPlan;
use crate::plan::{
    ExecuteContext, ExecutionReport, Operation, Plan, PlanContext, PlanSummary, Plannable, Verb,
};

#[derive(Debug, Args)]
pub struct ShimArgs {
    #[command(subcommand)]
    pub action: ShimAction,
}

#[derive(Debug, Subcommand)]
pub enum ShimAction {
    /// Add a host shim to the manifest
    Add {
        /// Shim name (command name in toolbox)
        name: String,
        /// Host command name (defaults to shim name)
        #[arg(short = 'H', long)]
        host: Option<String>,
    },
    /// Remove a shim from the manifest
    Remove {
        /// Shim name to remove
        name: String,
    },
    /// List all shims in the manifest
    List {
        /// Output format (table, json)
        #[arg(short, long, default_value = "table")]
        format: String,
    },
    /// Sync shims to the toolbox
    Sync,
}

/// Generate the content of a shim script.
/// Uses shlex for proper POSIX-compliant shell quoting.
fn generate_shim_script(host_cmd: &str) -> Result<String> {
    // Use shlex for proper shell quoting (handles all special characters)
    let quoted = shlex::try_quote(host_cmd)
        .map_err(|e| anyhow::anyhow!("Failed to quote command '{}': {}", host_cmd, e))?;
    Ok(format!(
        r#"#!/bin/bash
# Auto-generated shim - delegates to host command
# Managed by: bkt shim
# Host command: {host_cmd}
exec flatpak-spawn --host {quoted} "$@"
"#,
        host_cmd = host_cmd,
        quoted = quoted
    ))
}

/// Get a list of shim names that are currently installed (exist as files in the shims directory).
///
/// This is useful for drift detection to compare what's on disk vs what's in the manifest.
pub fn get_installed_shims() -> Vec<String> {
    let shims_dir = ShimsManifest::shims_dir();
    let mut installed = Vec::new();

    if let Ok(entries) = fs::read_dir(&shims_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file()
                && let Some(name) = path.file_name() {
                    installed.push(name.to_string_lossy().to_string());
                }
        }
    }

    installed.sort();
    installed
}

/// Sync all shims from merged manifest to disk.
fn sync_shims(dry_run: bool) -> Result<()> {
    let shims_dir = ShimsManifest::shims_dir();

    // Load manifests
    let system = ShimsManifest::load_system()?;
    let user = ShimsManifest::load_user()?;
    let merged = ShimsManifest::merged(&system, &user);

    if !dry_run {
        // Create shims directory
        fs::create_dir_all(&shims_dir).with_context(|| {
            format!("Failed to create shims directory: {}", shims_dir.display())
        })?;

        // Remove all existing shims (clean slate)
        if shims_dir.exists() {
            for entry in fs::read_dir(&shims_dir)? {
                let entry = entry?;
                if entry.file_type()?.is_file() {
                    fs::remove_file(entry.path())?;
                }
            }
        }
    }

    // Generate shims
    let mut count = 0;
    for shim in &merged.shims {
        let shim_path = shims_dir.join(&shim.name);
        let content = generate_shim_script(shim.host_cmd())?;

        if dry_run {
            Output::dry_run(format!(
                "Would create: {} -> {}",
                shim.name,
                shim.host_cmd()
            ));
        } else {
            fs::write(&shim_path, &content)
                .with_context(|| format!("Failed to write shim: {}", shim_path.display()))?;

            // Make executable
            let mut perms = fs::metadata(&shim_path)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&shim_path, perms)?;
        }
        count += 1;
    }

    if dry_run {
        Output::dry_run(format!(
            "Would generate {} shims in {}",
            count,
            shims_dir.display()
        ));
    } else {
        Output::success(format!(
            "Generated {} shims in {}",
            count,
            shims_dir.display()
        ));
    }

    Ok(())
}

/// Determine the source of a shim (system, user, or system+user override).
fn shim_source(name: &str, system: &ShimsManifest, user: &ShimsManifest) -> &'static str {
    let in_system = system.find(name).is_some();
    let in_user = user.find(name).is_some();
    match (in_system, in_user) {
        (true, true) => "system+user",
        (true, false) => "system",
        (false, true) => "user",
        (false, false) => "unknown",
    }
}

pub fn run(args: ShimArgs, plan: &ExecutionPlan) -> Result<()> {
    match args.action {
        ShimAction::Add { name, host } => {
            let host_cmd = host.clone().unwrap_or_else(|| name.clone());
            let shim = Shim {
                name: name.clone(),
                host: if host_cmd == name {
                    None
                } else {
                    Some(host_cmd.clone())
                },
            };

            // Load and update user manifest
            if plan.should_update_local_manifest() {
                let mut user = ShimsManifest::load_user()?;
                let is_update = user.find(&name).is_some();
                user.upsert(shim);
                user.save_user()?;

                if is_update {
                    Output::success(format!("Updated shim: {} -> {}", name, host_cmd));
                } else {
                    Output::success(format!("Added shim: {} -> {}", name, host_cmd));
                }
            } else if plan.dry_run {
                Output::dry_run(format!("Would add shim: {} -> {}", name, host_cmd));
            }

            // Record ephemeral change if using --local (not in dry-run mode)
            if plan.pr_mode == PrMode::LocalOnly && !plan.dry_run {
                let mut ephemeral = EphemeralManifest::load_validated()?;
                ephemeral.record(EphemeralChange::new(
                    ChangeDomain::Shim,
                    ChangeAction::Add,
                    &name,
                ));
                ephemeral.save()?;
            }

            // Sync shims to disk (shims are always synced locally, not host-dependent)
            if plan.should_execute_locally() {
                sync_shims(false)?;
            } else if plan.dry_run {
                Output::dry_run("Would sync shims to disk");
            }

            if plan.should_create_pr() {
                // Load system manifest, add the shim, and create PR
                let mut system = ShimsManifest::load_system()?;
                let shim_for_pr = Shim {
                    name: name.clone(),
                    host: if host_cmd == name {
                        None
                    } else {
                        Some(host_cmd.clone())
                    },
                };
                system.upsert(shim_for_pr);
                let manifest_content = serde_json::to_string_pretty(&system)?;

                plan.maybe_create_pr("shim", "add", &name, "host-shims.json", &manifest_content)?;
            }
        }
        ShimAction::Remove { name } => {
            let system = ShimsManifest::load_system()?;
            let user = ShimsManifest::load_user()?;

            // Check if it's in system manifest but not user manifest
            let in_system = system.find(&name).is_some();
            if in_system && user.find(&name).is_none() && !plan.should_create_pr() {
                Output::info(format!(
                    "'{}' is in the system manifest; use --pr or --pr-only to remove from source",
                    name
                ));
            }

            if plan.should_update_local_manifest() {
                let mut user = ShimsManifest::load_user()?;
                if user.remove(&name) {
                    user.save_user()?;
                    Output::success(format!("Removed shim: {}", name));
                } else {
                    Output::warning(format!("Shim not found in user manifest: {}", name));
                }
            } else if plan.dry_run {
                Output::dry_run(format!("Would remove shim: {}", name));
            }

            // Record ephemeral change if using --local (not in dry-run mode)
            if plan.pr_mode == PrMode::LocalOnly && !plan.dry_run {
                let mut ephemeral = EphemeralManifest::load_validated()?;
                ephemeral.record(EphemeralChange::new(
                    ChangeDomain::Shim,
                    ChangeAction::Remove,
                    &name,
                ));
                ephemeral.save()?;
            }

            // Sync shims to disk
            if plan.should_execute_locally() {
                sync_shims(false)?;
            } else if plan.dry_run {
                Output::dry_run("Would sync shims to disk");
            }

            if plan.should_create_pr() {
                // Load system manifest, remove the shim, and create PR
                let mut system_manifest = ShimsManifest::load_system()?;
                if system_manifest.remove(&name) {
                    let manifest_content = serde_json::to_string_pretty(&system_manifest)?;

                    plan.maybe_create_pr(
                        "shim",
                        "remove",
                        &name,
                        "host-shims.json",
                        &manifest_content,
                    )?;
                } else {
                    Output::info(format!("'{}' not in system manifest, no PR needed", name));
                }
            }
        }
        ShimAction::List { format } => {
            let system = ShimsManifest::load_system()?;
            let user = ShimsManifest::load_user()?;
            let merged = ShimsManifest::merged(&system, &user);

            if format == "json" {
                let json = serde_json::to_string_pretty(&merged)?;
                println!("{}", json);
            } else {
                let shims_dir = ShimsManifest::shims_dir();
                Output::subheader(format!("SHIMS (from {}):", shims_dir.display()));

                if merged.shims.is_empty() {
                    Output::info("(none)");
                } else {
                    for shim in &merged.shims {
                        let source = shim_source(&shim.name, &system, &user);
                        let source_styled = match source {
                            "user" => source.yellow().to_string(),
                            "system" => source.dimmed().to_string(),
                            "system+user" => source.cyan().to_string(),
                            _ => source.to_string(),
                        };
                        if shim.name == shim.host_cmd() {
                            println!("  {:<20}  [{}]", shim.name, source_styled);
                        } else {
                            println!(
                                "  {:<20} {} {:<20}  [{}]",
                                shim.name,
                                "->".dimmed(),
                                shim.host_cmd(),
                                source_styled
                            );
                        }
                    }
                }
            }
        }
        ShimAction::Sync => {
            // Use the new Plan-based implementation
            let cwd = std::env::current_dir()?;
            let plan_ctx = PlanContext::new(cwd, plan.clone());

            let sync_plan = ShimSyncCommand.plan(&plan_ctx)?;

            if sync_plan.is_empty() {
                Output::info("No shims to generate.");
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
    }
    Ok(())
}

// ============================================================================
// Plan-based Shim Sync Implementation
// ============================================================================

/// Command to sync shims from manifests to disk.
pub struct ShimSyncCommand;

/// Plan for syncing shims.
pub struct ShimSyncPlan {
    /// Directory where shims will be written.
    shims_dir: PathBuf,
    /// Shims to create.
    to_create: Vec<Shim>,
}

impl Plannable for ShimSyncCommand {
    type Plan = ShimSyncPlan;

    fn plan(&self, _ctx: &PlanContext) -> Result<Self::Plan> {
        // Load and merge manifests (read-only, no side effects)
        let system = ShimsManifest::load_system()?;
        let user = ShimsManifest::load_user()?;
        let merged = ShimsManifest::merged(&system, &user);

        Ok(ShimSyncPlan {
            shims_dir: ShimsManifest::shims_dir(),
            to_create: merged.shims,
        })
    }
}

impl Plan for ShimSyncPlan {
    fn describe(&self) -> PlanSummary {
        let mut summary = PlanSummary::new(format!(
            "Shim Sync: {} shim(s) in {}",
            self.to_create.len(),
            self.shims_dir.display()
        ));

        for shim in &self.to_create {
            let details = if shim.name == shim.host_cmd() {
                None
            } else {
                Some(format!("-> {}", shim.host_cmd()))
            };

            if let Some(d) = details {
                summary.add_operation(Operation::with_details(
                    Verb::Create,
                    format!("shim:{}", shim.name),
                    d,
                ));
            } else {
                summary.add_operation(Operation::new(Verb::Create, format!("shim:{}", shim.name)));
            }
        }

        summary
    }

    fn execute(self, _ctx: &mut ExecuteContext) -> Result<ExecutionReport> {
        let mut report = ExecutionReport::new();

        // Create shims directory
        fs::create_dir_all(&self.shims_dir).with_context(|| {
            format!(
                "Failed to create shims directory: {}",
                self.shims_dir.display()
            )
        })?;

        // Remove all existing shims in the managed directory.
        // This directory is exclusively managed by bkt; any files here
        // are assumed to be bkt-generated shims that should be regenerated.
        if self.shims_dir.exists() {
            for entry in fs::read_dir(&self.shims_dir)? {
                let entry = entry?;
                if entry.file_type()?.is_file() {
                    fs::remove_file(entry.path())?;
                }
            }
        }

        // Generate shims
        for shim in &self.to_create {
            let shim_path = self.shims_dir.join(&shim.name);

            match generate_shim_script(shim.host_cmd()) {
                Ok(content) => {
                    if let Err(e) = fs::write(&shim_path, &content) {
                        report.record_failure(
                            Verb::Create,
                            format!("shim:{}", shim.name),
                            e.to_string(),
                        );
                        continue;
                    }

                    // Make executable
                    if let Err(e) = (|| -> Result<()> {
                        let mut perms = fs::metadata(&shim_path)?.permissions();
                        perms.set_mode(0o755);
                        fs::set_permissions(&shim_path, perms)?;
                        Ok(())
                    })() {
                        report.record_failure(
                            Verb::Create,
                            format!("shim:{}", shim.name),
                            format!("chmod failed: {}", e),
                        );
                        continue;
                    }

                    report.record_success(Verb::Create, format!("shim:{}", shim.name));
                }
                Err(e) => {
                    report.record_failure(
                        Verb::Create,
                        format!("shim:{}", shim.name),
                        e.to_string(),
                    );
                }
            }
        }

        Ok(report)
    }

    fn is_empty(&self) -> bool {
        self.to_create.is_empty()
    }
}
