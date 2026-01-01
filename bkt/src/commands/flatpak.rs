//! Flatpak command implementation.

use crate::context::CommandDomain;
use crate::manifest::{FlatpakApp, FlatpakAppsManifest, FlatpakScope};
use crate::pipeline::ExecutionPlan;
use anyhow::{Context, Result};
use clap::{Args, Subcommand};
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

/// Sync flatpak apps from manifest to installed state.
fn sync_flatpaks(plan: &ExecutionPlan) -> Result<()> {
    let system = FlatpakAppsManifest::load_system()?;
    let user = FlatpakAppsManifest::load_user()?;
    let merged = FlatpakAppsManifest::merged(&system, &user);

    let mut installed = 0;
    let mut skipped = 0;
    let mut failed = 0;

    for app in &merged.apps {
        if is_installed(&app.id) {
            skipped += 1;
            continue;
        }

        if plan.dry_run {
            println!(
                "Would install: {} from {} ({})",
                app.id, app.remote, app.scope
            );
        } else if plan.should_execute_locally() {
            print!("Installing {} ({})... ", app.id, app.scope);
            if install_flatpak(app)? {
                println!("✓");
                installed += 1;
            } else {
                println!("✗");
                failed += 1;
            }
        }
    }

    if plan.dry_run {
        println!(
            "\nDry run: {} already installed, {} would be installed",
            skipped,
            merged.apps.len() - skipped
        );
    } else {
        println!(
            "\nSync complete: {} installed, {} already present, {} failed",
            installed, skipped, failed
        );
    }

    Ok(())
}

pub fn run(args: FlatpakArgs, plan: &ExecutionPlan) -> Result<()> {
    match args.action {
        FlatpakAction::Add {
            app_id,
            remote,
            scope,
        } => {
            // Validate that flatpak operations are allowed in this context
            plan.validate_domain(CommandDomain::Flatpak)?;

            let scope: FlatpakScope = scope.parse()?;

            // Check if already in manifest
            let system = FlatpakAppsManifest::load_system()?;
            let mut user = FlatpakAppsManifest::load_user()?;

            let already_exists = system.find(&app_id).is_some() || user.find(&app_id).is_some();

            if already_exists {
                println!("Flatpak already in manifest: {}", app_id);
            } else if plan.should_update_local_manifest() {
                let app = FlatpakApp {
                    id: app_id.clone(),
                    remote: remote.clone(),
                    scope,
                };
                user.upsert(app.clone());
                user.save_user()?;
                println!("Added to user manifest: {} ({}, {})", app_id, remote, scope);
            } else if plan.dry_run {
                println!(
                    "[dry-run] Would add to manifest: {} ({}, {})",
                    app_id, remote, scope
                );
            }

            // Install the flatpak
            if plan.should_execute_locally() && !is_installed(&app_id) {
                print!("Installing {}... ", app_id);
                let app = FlatpakApp {
                    id: app_id.clone(),
                    remote: remote.clone(),
                    scope,
                };
                if install_flatpak(&app)? {
                    println!("✓");
                } else {
                    println!("✗ (failed)");
                }
            } else if plan.dry_run && !is_installed(&app_id) {
                println!("[dry-run] Would install: {}", app_id);
            } else if is_installed(&app_id) {
                println!("Already installed: {}", app_id);
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
                println!(
                    "Note: '{}' is only in the system manifest; run this command without the --local flag to also create a PR to remove it from the system manifest",
                    app_id
                );
            }

            if plan.should_update_local_manifest() {
                if user.remove(&app_id) {
                    user.save_user()?;
                    println!("Removed from user manifest: {}", app_id);
                } else if !in_system {
                    println!("Flatpak not found in manifest: {}", app_id);
                }
            } else if plan.dry_run {
                if user.find(&app_id).is_some() {
                    println!("[dry-run] Would remove from user manifest: {}", app_id);
                } else if !in_system {
                    println!("[dry-run] Flatpak not found in manifest: {}", app_id);
                }
            }

            // Optionally uninstall
            if plan.should_execute_locally() && is_installed(&app_id) {
                print!("Uninstalling {}... ", app_id);
                // Try system first, then user
                if uninstall_flatpak(&app_id, FlatpakScope::System)?
                    || uninstall_flatpak(&app_id, FlatpakScope::User)?
                {
                    println!("✓");
                } else {
                    println!("✗ (may need manual removal)");
                }
            } else if plan.dry_run && is_installed(&app_id) {
                println!("[dry-run] Would uninstall: {}", app_id);
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
                    println!("Note: '{}' not in system manifest, no PR needed", app_id);
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
                    println!("No flatpak apps in manifest.");
                    return Ok(());
                }

                println!("{:<50} {:<12} {:<8} SOURCE", "ID", "REMOTE", "SCOPE");
                println!("{}", "-".repeat(80));

                for app in &merged.apps {
                    let source = if user.find(&app.id).is_some() {
                        "user"
                    } else {
                        "system"
                    };
                    let installed = if is_installed(&app.id) { "✓" } else { "✗" };
                    println!(
                        "{:<50} {:<12} {:<8} {} {}",
                        app.id, app.remote, app.scope, source, installed
                    );
                }

                println!(
                    "\n{} apps ({} system, {} user)",
                    merged.apps.len(),
                    system.apps.len(),
                    user.apps.len()
                );
            }
        }
        FlatpakAction::Sync => {
            // Validate that flatpak operations are allowed in this context
            plan.validate_domain(CommandDomain::Flatpak)?;

            sync_flatpaks(plan)?;
        }
    }
    Ok(())
}
