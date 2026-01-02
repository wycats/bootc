//! GSettings command implementation.

use crate::manifest::{GSetting, GSettingsManifest};
use crate::output::Output;
use crate::pipeline::ExecutionPlan;
use crate::pr::{PrChange, run_pr_workflow};
use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use owo_colors::OwoColorize;
use std::process::Command;

#[derive(Debug, Args)]
pub struct GSettingArgs {
    #[command(subcommand)]
    pub action: GSettingAction,
}

#[derive(Debug, Subcommand)]
pub enum GSettingAction {
    /// Set a GSettings value in the manifest
    Set {
        /// Schema name (e.g., org.gnome.desktop.interface)
        schema: String,
        /// Key name
        key: String,
        /// Value (as GVariant string)
        value: String,
        /// Optional comment
        #[arg(short, long)]
        comment: Option<String>,
        /// Create a PR with the change
        #[arg(long)]
        pr: bool,
        /// Skip pre-flight checks for PR workflow
        #[arg(long)]
        skip_preflight: bool,
    },
    /// Remove a GSettings entry from the manifest
    Unset {
        /// Schema name
        schema: String,
        /// Key name
        key: String,
        /// Create a PR with the change
        #[arg(long)]
        pr: bool,
        /// Skip pre-flight checks for PR workflow
        #[arg(long)]
        skip_preflight: bool,
    },
    /// List all GSettings in the manifest
    List {
        /// Output format (table, json)
        #[arg(short, long, default_value = "table")]
        format: String,
    },
    /// Apply all GSettings from the manifest
    Apply {
        /// Show what would be done without making changes
        #[arg(long)]
        dry_run: bool,
    },
}

/// Get current value of a gsetting.
fn get_current_value(schema: &str, key: &str) -> Option<String> {
    Command::new("gsettings")
        .args(["get", schema, key])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

/// Set a gsetting value.
fn set_gsetting(schema: &str, key: &str, value: &str) -> Result<bool> {
    let status = Command::new("gsettings")
        .args(["set", schema, key, value])
        .status()
        .context("Failed to run gsettings set")?;
    Ok(status.success())
}

/// Apply all settings from manifest.
fn apply_settings(dry_run: bool) -> Result<()> {
    let system = GSettingsManifest::load_system()?;
    let user = GSettingsManifest::load_user()?;
    let merged = GSettingsManifest::merged(&system, &user);

    let mut applied = 0;
    let mut skipped = 0;
    let mut failed = 0;

    for setting in &merged.settings {
        let current = get_current_value(&setting.schema, &setting.key);

        if current.as_deref() == Some(&setting.value) {
            skipped += 1;
            continue;
        }

        if dry_run {
            Output::dry_run(format!(
                "Would set {}.{} = {} (currently: {})",
                setting.schema,
                setting.key,
                setting.value,
                current.as_deref().unwrap_or("(unset)")
            ));
        } else {
            let spinner = Output::spinner(format!(
                "Setting {}.{} = {}...",
                setting.schema, setting.key, setting.value
            ));
            if set_gsetting(&setting.schema, &setting.key, &setting.value)? {
                spinner.finish_success(format!("Set {}.{}", setting.schema, setting.key));
                applied += 1;
            } else {
                spinner.finish_error(format!("Failed {}.{}", setting.schema, setting.key));
                failed += 1;
            }
        }
    }

    if dry_run {
        Output::blank();
        Output::info(format!(
            "Dry run: {} already set, {} would be applied",
            skipped,
            merged.settings.len() - skipped
        ));
    } else {
        Output::blank();
        Output::info(format!(
            "Apply complete: {} applied, {} already set, {} failed",
            applied, skipped, failed
        ));
    }

    Ok(())
}

pub fn run(args: GSettingArgs, _plan: &ExecutionPlan) -> Result<()> {
    // TODO: Migrate to use `ExecutionPlan` instead of per-command flags.
    // The `_plan` parameter is intentionally unused and reserved for future use
    // after this migration.
    match args.action {
        GSettingAction::Set {
            schema,
            key,
            value,
            comment,
            pr,
            skip_preflight,
        } => {
            let system = GSettingsManifest::load_system()?;
            let mut user = GSettingsManifest::load_user()?;

            // Check if already set to same value
            let existing = system
                .find(&schema, &key)
                .or_else(|| user.find(&schema, &key));

            if let Some(e) = existing {
                if e.value == value {
                    Output::info(format!("Already in manifest: {}.{} = {}", schema, key, value));
                } else {
                    // Update in user manifest
                    let setting = GSetting {
                        schema: schema.clone(),
                        key: key.clone(),
                        value: value.clone(),
                        comment,
                    };
                    user.upsert(setting);
                    user.save_user()?;
                    Output::success(format!("Updated in user manifest: {}.{} = {}", schema, key, value));
                }
            } else {
                let setting = GSetting {
                    schema: schema.clone(),
                    key: key.clone(),
                    value: value.clone(),
                    comment,
                };
                user.upsert(setting);
                user.save_user()?;
                Output::success(format!("Added to user manifest: {}.{} = {}", schema, key, value));
            }

            // Apply immediately
            let spinner = Output::spinner(format!("Applying {}.{} = {}...", schema, key, value));
            if set_gsetting(&schema, &key, &value)? {
                spinner.finish_success(format!("Applied {}.{}", schema, key));
            } else {
                spinner.finish_error(format!("Failed to apply {}.{}", schema, key));
            }

            if pr {
                let mut system_manifest = GSettingsManifest::load_system()?;
                let setting_for_pr = GSetting {
                    schema: schema.clone(),
                    key: key.clone(),
                    value: value.clone(),
                    comment: None,
                };
                system_manifest.upsert(setting_for_pr);
                let manifest_content = serde_json::to_string_pretty(&system_manifest)?;

                let change = PrChange {
                    manifest_type: "gsetting".to_string(),
                    action: "set".to_string(),
                    name: format!("{}.{}", schema, key),
                    manifest_file: "gsettings.json".to_string(),
                };
                run_pr_workflow(&change, &manifest_content, skip_preflight)?;
            }
        }
        GSettingAction::Unset {
            schema,
            key,
            pr,
            skip_preflight,
        } => {
            let mut user = GSettingsManifest::load_user()?;
            let system = GSettingsManifest::load_system()?;

            let in_system = system.find(&schema, &key).is_some();
            if in_system && user.find(&schema, &key).is_none() {
                Output::info(format!(
                    "'{}.{}' is in the system manifest; use --pr to remove from source",
                    schema, key
                ));
            }

            if user.remove(&schema, &key) {
                user.save_user()?;
                Output::success(format!("Removed from user manifest: {}.{}", schema, key));
            } else if !in_system {
                Output::warning(format!("Setting not found in manifest: {}.{}", schema, key));
            }

            // Reset to default
            let spinner = Output::spinner(format!("Resetting {}.{} to default...", schema, key));
            let status = Command::new("gsettings")
                .args(["reset", &schema, &key])
                .status()
                .context("Failed to run gsettings reset")?;
            if status.success() {
                spinner.finish_success(format!("Reset {}.{}", schema, key));
            } else {
                spinner.finish_error(format!("Failed to reset {}.{}", schema, key));
            }

            if pr {
                let mut system_manifest = GSettingsManifest::load_system()?;
                if system_manifest.remove(&schema, &key) {
                    let manifest_content = serde_json::to_string_pretty(&system_manifest)?;

                    let change = PrChange {
                        manifest_type: "gsetting".to_string(),
                        action: "unset".to_string(),
                        name: format!("{}.{}", schema, key),
                        manifest_file: "gsettings.json".to_string(),
                    };
                    run_pr_workflow(&change, &manifest_content, skip_preflight)?;
                } else {
                    Output::info(format!(
                        "'{}.{}' not in system manifest, no PR needed",
                        schema, key
                    ));
                }
            }
        }
        GSettingAction::List { format } => {
            let system = GSettingsManifest::load_system()?;
            let user = GSettingsManifest::load_user()?;
            let merged = GSettingsManifest::merged(&system, &user);

            if format == "json" {
                println!("{}", serde_json::to_string_pretty(&merged)?);
            } else {
                if merged.settings.is_empty() {
                    Output::info("No gsettings in manifest.");
                    return Ok(());
                }

                Output::subheader("GSETTINGS:");
                println!(
                    "{:<45} {:<20} {:<10} {}",
                    "SCHEMA.KEY".cyan(),
                    "VALUE".cyan(),
                    "SOURCE".cyan(),
                    "CURRENT".cyan()
                );
                Output::separator();

                for setting in &merged.settings {
                    let source = if user.find(&setting.schema, &setting.key).is_some() {
                        "user".yellow().to_string()
                    } else {
                        "system".dimmed().to_string()
                    };
                    let current = get_current_value(&setting.schema, &setting.key)
                        .unwrap_or_else(|| "(unset)".to_string());
                    let matches = if current == setting.value {
                        "✓".green().to_string()
                    } else {
                        "≠".yellow().to_string()
                    };

                    println!(
                        "{:<45} {:<20} {:<10} {} {}",
                        format!("{}.{}", setting.schema, setting.key),
                        truncate(&setting.value, 18),
                        source,
                        matches,
                        truncate(&current, 15)
                    );
                }

                Output::blank();
                Output::info(format!(
                    "{} settings ({} system, {} user)",
                    merged.settings.len(),
                    system.settings.len(),
                    user.settings.len()
                ));
            }
        }
        GSettingAction::Apply { dry_run } => {
            apply_settings(dry_run)?;
        }
    }
    Ok(())
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else if max == 0 {
        String::new()
    } else {
        let truncated: String = s.chars().take(max - 1).collect();
        format!("{}…", truncated)
    }
}
