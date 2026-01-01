//! GNOME extension command implementation.

use crate::manifest::GnomeExtensionsManifest;
use crate::pipeline::ExecutionPlan;
use crate::pr::{PrChange, run_pr_workflow};
use anyhow::{Context, Result};
use clap::{Args, Subcommand};
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
fn disable_extension(uuid: &str) -> Result<bool> {
    let status = Command::new("gnome-extensions")
        .args(["disable", uuid])
        .status()
        .context("Failed to run gnome-extensions disable")?;
    Ok(status.success())
}

/// Sync extensions from manifest.
fn sync_extensions(dry_run: bool) -> Result<()> {
    let system = GnomeExtensionsManifest::load_system()?;
    let user = GnomeExtensionsManifest::load_user()?;
    let merged = GnomeExtensionsManifest::merged(&system, &user);

    let mut enabled = 0;
    let mut skipped = 0;
    let mut not_installed = 0;

    for uuid in &merged.extensions {
        if !is_installed(uuid) {
            if dry_run {
                println!("Not installed (would skip): {}", uuid);
            } else {
                println!("Not installed (skipping): {}", uuid);
            }
            not_installed += 1;
            continue;
        }

        if is_enabled(uuid) {
            skipped += 1;
            continue;
        }

        if dry_run {
            println!("Would enable: {}", uuid);
        } else {
            print!("Enabling {}... ", uuid);
            if enable_extension(uuid)? {
                println!("✓");
                enabled += 1;
            } else {
                println!("✗");
            }
        }
    }

    if dry_run {
        println!(
            "\nDry run: {} already enabled, {} would be enabled, {} not installed",
            skipped,
            merged.extensions.len() - skipped - not_installed,
            not_installed
        );
    } else {
        println!(
            "\nSync complete: {} enabled, {} already active, {} not installed",
            enabled, skipped, not_installed
        );
    }

    Ok(())
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
        } => {
            let system = GnomeExtensionsManifest::load_system()?;
            let mut user = GnomeExtensionsManifest::load_user()?;

            if system.contains(&uuid) || user.contains(&uuid) {
                println!("Extension already in manifest: {}", uuid);
            } else {
                user.add(uuid.clone());
                user.save_user()?;
                println!("Added to user manifest: {}", uuid);
            }

            // Enable if installed
            if is_installed(&uuid) {
                if !is_enabled(&uuid) {
                    print!("Enabling {}... ", uuid);
                    if enable_extension(&uuid)? {
                        println!("✓");
                    } else {
                        println!("✗");
                    }
                } else {
                    println!("Already enabled: {}", uuid);
                }
            } else {
                println!(
                    "Note: Extension not installed. Install via Extension Manager or extensions.gnome.org"
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
                println!(
                    "Note: '{}' is in the system manifest; use --pr to remove from source",
                    uuid
                );
            }

            if user.remove(&uuid) {
                user.save_user()?;
                println!("Removed from user manifest: {}", uuid);
            } else if !in_system {
                println!("Extension not found in manifest: {}", uuid);
            }

            // Disable if enabled
            if is_enabled(&uuid) {
                print!("Disabling {}... ", uuid);
                if disable_extension(&uuid)? {
                    println!("✓");
                } else {
                    println!("✗");
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
                    println!("Note: '{}' not in system manifest, no PR needed", uuid);
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
                    println!("No extensions in manifest.");
                    return Ok(());
                }

                println!("{:<50} {:<10} STATUS", "UUID", "SOURCE");
                println!("{}", "-".repeat(70));

                for uuid in &merged.extensions {
                    let source = if user.contains(uuid) {
                        "user"
                    } else {
                        "system"
                    };
                    let status = if is_enabled(uuid) {
                        "✓ enabled"
                    } else if is_installed(uuid) {
                        "○ disabled"
                    } else {
                        "✗ not installed"
                    };
                    println!("{:<50} {:<10} {}", uuid, source, status);
                }

                println!(
                    "\n{} extensions ({} system, {} user)",
                    merged.extensions.len(),
                    system.extensions.len(),
                    user.extensions.len()
                );
            }
        }
        ExtensionAction::Sync { dry_run } => {
            sync_extensions(dry_run)?;
        }
    }
    Ok(())
}
