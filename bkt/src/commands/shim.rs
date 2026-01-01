//! Shim command implementation.

use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use std::fs;
use std::os::unix::fs::PermissionsExt;

use crate::manifest::{Shim, ShimsManifest};
use crate::pipeline::ExecutionPlan;
use crate::pr::{PrChange, run_pr_workflow};

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
        /// Create a PR with the change
        #[arg(long)]
        pr: bool,
        /// Skip pre-flight checks for PR workflow
        #[arg(long)]
        skip_preflight: bool,
    },
    /// Remove a shim from the manifest
    Remove {
        /// Shim name to remove
        name: String,
        /// Create a PR with the change
        #[arg(long)]
        pr: bool,
        /// Skip pre-flight checks for PR workflow
        #[arg(long)]
        skip_preflight: bool,
    },
    /// List all shims in the manifest
    List {
        /// Output format (table, json)
        #[arg(short, long, default_value = "table")]
        format: String,
    },
    /// Sync shims to the toolbox
    Sync {
        /// Show what would be done without making changes
        #[arg(long)]
        dry_run: bool,
    },
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
            println!("Would create: {} -> {}", shim.name, shim.host_cmd());
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
        println!("Would generate {} shims in {}", count, shims_dir.display());
    } else {
        println!("✓ Generated {} shims in {}", count, shims_dir.display());
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

pub fn run(args: ShimArgs, _plan: &ExecutionPlan) -> Result<()> {
    // TODO: Migrate to use plan instead of per-command flags
    // For now, per-command --pr and --dry-run flags still work
    match args.action {
        ShimAction::Add {
            name,
            host,
            pr,
            skip_preflight,
        } => {
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
            let mut user = ShimsManifest::load_user()?;
            let is_update = user.find(&name).is_some();
            user.upsert(shim);
            user.save_user()?;

            if is_update {
                println!("Updated shim: {} -> {}", name, host_cmd);
            } else {
                println!("Added shim: {} -> {}", name, host_cmd);
            }

            // Sync shims to disk
            sync_shims(false)?;

            if pr {
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

                let change = PrChange {
                    manifest_type: "shim".to_string(),
                    action: "add".to_string(),
                    name: name.clone(),
                    manifest_file: "host-shims.json".to_string(),
                };
                run_pr_workflow(&change, &manifest_content, skip_preflight)?;
            }
        }
        ShimAction::Remove {
            name,
            pr,
            skip_preflight,
        } => {
            let mut user = ShimsManifest::load_user()?;
            let system = ShimsManifest::load_system()?;

            // Check if it's in system manifest
            if system.find(&name).is_some() && user.find(&name).is_none() {
                println!(
                    "Note: '{}' is in the system manifest; you can add a user override to hide it",
                    name
                );
            }

            if user.remove(&name) {
                user.save_user()?;
                println!("Removed shim: {}", name);
            } else {
                println!("Shim not found in user manifest: {}", name);
            }

            // Sync shims to disk
            sync_shims(false)?;

            if pr {
                // Load system manifest, remove the shim, and create PR
                let mut system_manifest = ShimsManifest::load_system()?;
                if system_manifest.remove(&name) {
                    let manifest_content = serde_json::to_string_pretty(&system_manifest)?;

                    let change = PrChange {
                        manifest_type: "shim".to_string(),
                        action: "remove".to_string(),
                        name: name.clone(),
                        manifest_file: "host-shims.json".to_string(),
                    };
                    run_pr_workflow(&change, &manifest_content, skip_preflight)?;
                } else {
                    println!("Note: '{}' not in system manifest, no PR needed", name);
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
                println!("Shims (from {}):", shims_dir.display());
                println!();

                if merged.shims.is_empty() {
                    println!("  (none)");
                } else {
                    for shim in &merged.shims {
                        let source = shim_source(&shim.name, &system, &user);
                        if shim.name == shim.host_cmd() {
                            println!("  {:<20}  [{}]", shim.name, source);
                        } else {
                            println!(
                                "  {:<20} -> {:<20}  [{}]",
                                shim.name,
                                shim.host_cmd(),
                                source
                            );
                        }
                    }
                }
            }
        }
        ShimAction::Sync { dry_run } => {
            sync_shims(dry_run)?;
        }
    }
    Ok(())
}
