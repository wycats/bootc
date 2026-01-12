//! System profile command implementation.
//!
//! Captures current system state and compares against manifests.

use crate::manifest::{FlatpakAppsManifest, GSettingsManifest, GnomeExtensionsManifest};
use crate::output::Output;
use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use owo_colors::OwoColorize;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Args)]
pub struct ProfileArgs {
    #[command(subcommand)]
    pub action: ProfileAction,
}

#[derive(Debug, Subcommand)]
pub enum ProfileAction {
    /// Capture current system profile
    Capture {
        /// Output file (defaults to stdout)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Show diff between system state and manifests
    Diff {
        /// Show only specific sections (flatpak, extension, gsetting)
        #[arg(short, long)]
        section: Option<String>,
    },
    /// Show unowned files (not in RPM database)
    Unowned {
        /// Directory to scan
        #[arg(short, long, default_value = "/usr/local/bin")]
        dir: PathBuf,
    },
}

/// Profile of installed flatpaks.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FlatpakProfile {
    pub apps: Vec<InstalledFlatpak>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledFlatpak {
    pub id: String,
    pub origin: String,
    pub installation: String,
}

/// Profile of installed GNOME extensions.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExtensionProfile {
    pub installed: Vec<String>,
    pub enabled: Vec<String>,
}

/// Complete system profile.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemProfile {
    pub generated_at: String,
    pub hostname: String,
    pub flatpak: FlatpakProfile,
    pub extensions: ExtensionProfile,
}

/// Get list of installed flatpaks.
fn get_installed_flatpaks() -> Result<Vec<InstalledFlatpak>> {
    let output = Command::new("flatpak")
        .args(["list", "--app", "--columns=installation,application,origin"])
        .output()
        .context("Failed to run flatpak list")?;

    if !output.status.success() {
        return Ok(vec![]);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut apps = Vec::new();

    for line in stdout.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() >= 2 {
            apps.push(InstalledFlatpak {
                installation: parts.first().unwrap_or(&"").to_string(),
                id: parts.get(1).unwrap_or(&"").to_string(),
                origin: parts.get(2).unwrap_or(&"").to_string(),
            });
        }
    }

    Ok(apps)
}

/// Get list of installed GNOME extensions.
fn get_installed_extensions() -> Result<Vec<String>> {
    let output = Command::new("gnome-extensions")
        .args(["list"])
        .output()
        .context("Failed to run gnome-extensions list")?;

    if !output.status.success() {
        return Ok(vec![]);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect())
}

/// Get list of enabled GNOME extensions.
///
/// Parses GVariant array output from `gsettings get`. This is a simplified parser
/// that handles the common case of `['ext1@author', 'ext2@author']` format.
/// Empty arrays may appear as `@as []` in some GVariant representations.
///
/// Note: This parser handles the common cases but may not handle all GVariant
/// representations (e.g., with type annotations or nested structures).
fn get_enabled_extensions() -> Result<Vec<String>> {
    let output = Command::new("gsettings")
        .args(["get", "org.gnome.shell", "enabled-extensions"])
        .output()
        .context("Failed to run gsettings get")?;

    if !output.status.success() {
        return Ok(vec![]);
    }

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();

    // Parse GVariant array format: ['ext1@author', 'ext2@author']
    // Handle empty array representations: [], @as [], @as
    let trimmed = stdout
        .trim_start_matches("@as")
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']');
    if trimmed.is_empty() {
        return Ok(vec![]);
    }

    let extensions: Vec<String> = trimmed
        .split(',')
        .map(|s| s.trim().trim_matches('\'').trim_matches('"').to_string())
        .filter(|s| !s.is_empty())
        .collect();

    Ok(extensions)
}

/// Get hostname.
fn get_hostname() -> String {
    std::fs::read_to_string("/etc/hostname")
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string())
}

/// Capture the full system profile.
fn capture_profile() -> Result<SystemProfile> {
    let now = chrono::Utc::now().to_rfc3339();

    let flatpaks = get_installed_flatpaks().unwrap_or_default();
    let installed_exts = get_installed_extensions().unwrap_or_default();
    let enabled_exts = get_enabled_extensions().unwrap_or_default();

    Ok(SystemProfile {
        generated_at: now,
        hostname: get_hostname(),
        flatpak: FlatpakProfile { apps: flatpaks },
        extensions: ExtensionProfile {
            installed: installed_exts,
            enabled: enabled_exts,
        },
    })
}

/// Show diff between system state and manifests.
fn show_diff(section: Option<&str>) -> Result<()> {
    let show_all = section.is_none();
    let section = section.unwrap_or("");

    // Flatpak diff
    if show_all || section == "flatpak" || section == "fp" {
        Output::subheader("=== FLATPAK DIFF ===");

        let system = FlatpakAppsManifest::load_system()?;
        let user = FlatpakAppsManifest::load_user().unwrap_or_default();
        let manifest = FlatpakAppsManifest::merged(&system, &user);
        let installed = get_installed_flatpaks()?;

        let manifest_ids: HashSet<String> = manifest.apps.iter().map(|a| a.id.clone()).collect();
        let installed_ids: HashSet<String> = installed.iter().map(|a| a.id.clone()).collect();

        let in_manifest_not_installed: Vec<_> = manifest_ids.difference(&installed_ids).collect();
        let installed_not_in_manifest: Vec<_> = installed_ids.difference(&manifest_ids).collect();

        if in_manifest_not_installed.is_empty() && installed_not_in_manifest.is_empty() {
            Output::success("No drift detected");
            Output::blank();
        } else {
            if !in_manifest_not_installed.is_empty() {
                println!("{}", "Missing (in manifest, not installed):".yellow());
                for id in &in_manifest_not_installed {
                    println!("  {} {}", "-".red(), id);
                }
                Output::blank();
            }
            if !installed_not_in_manifest.is_empty() {
                println!("{}", "Extra (installed, not in manifest):".yellow());
                for id in &installed_not_in_manifest {
                    println!("  {} {}", "+".green(), id);
                }
                Output::blank();
            }
        }
    }

    // Extension diff
    if show_all || section == "extension" || section == "ext" {
        Output::subheader("=== EXTENSION DIFF ===");

        let system = GnomeExtensionsManifest::load_system()?;
        let user = GnomeExtensionsManifest::load_user().unwrap_or_default();
        let manifest = GnomeExtensionsManifest::merged(&system, &user);
        let installed = get_installed_extensions()?;

        let manifest_set: HashSet<String> = manifest
            .extensions
            .into_iter()
            .map(|e| e.id().to_string())
            .collect();
        let installed_set: HashSet<String> = installed.into_iter().collect();

        let in_manifest_not_installed: Vec<_> = manifest_set.difference(&installed_set).collect();
        let installed_not_in_manifest: Vec<_> = installed_set.difference(&manifest_set).collect();

        if in_manifest_not_installed.is_empty() && installed_not_in_manifest.is_empty() {
            Output::success("No drift detected");
            Output::blank();
        } else {
            if !in_manifest_not_installed.is_empty() {
                println!("{}", "Missing (in manifest, not installed):".yellow());
                for uuid in &in_manifest_not_installed {
                    println!("  {} {}", "-".red(), uuid);
                }
                Output::blank();
            }
            if !installed_not_in_manifest.is_empty() {
                println!("{}", "Extra (installed, not in manifest):".yellow());
                for uuid in &installed_not_in_manifest {
                    println!("  {} {}", "+".green(), uuid);
                }
                Output::blank();
            }
        }
    }

    // GSettings diff
    if show_all || section == "gsetting" || section == "gs" {
        Output::subheader("=== GSETTINGS DIFF ===");

        let system = GSettingsManifest::load_system()?;
        let user = GSettingsManifest::load_user().unwrap_or_default();
        let manifest = GSettingsManifest::merged(&system, &user);

        if manifest.settings.is_empty() {
            Output::info("(no gsettings in manifest)");
            Output::blank();
        } else {
            let mut drifted = 0;
            for setting in &manifest.settings {
                let current = Command::new("gsettings")
                    .args(["get", &setting.schema, &setting.key])
                    .output()
                    .ok()
                    .filter(|o| o.status.success())
                    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());

                if let Some(current_val) = current {
                    // Normalize for comparison: gsettings outputs strings with quotes
                    let normalized_current = current_val
                        .strip_prefix('\'')
                        .and_then(|s| s.strip_suffix('\''))
                        .unwrap_or(&current_val);
                    if normalized_current != setting.value {
                        println!(
                            "{} {}.{}\n  manifest: {}\n  current:  {}",
                            "≠".yellow(),
                            setting.schema,
                            setting.key,
                            setting.value.green(),
                            current_val.red()
                        );
                        Output::blank();
                        drifted += 1;
                    }
                } else {
                    println!(
                        "{} {}.{} (could not read current value)",
                        "?".yellow(),
                        setting.schema,
                        setting.key
                    );
                    Output::blank();
                    drifted += 1;
                }
            }

            if drifted == 0 {
                Output::success(format!(
                    "All {} settings match manifest",
                    manifest.settings.len()
                ));
                Output::blank();
            }
        }
    }

    Ok(())
}

/// Show files not owned by any RPM package.
fn show_unowned(dir: &PathBuf) -> Result<()> {
    use std::fs;

    if !dir.exists() {
        Output::warning(format!("Directory does not exist: {}", dir.display()));
        return Ok(());
    }

    Output::subheader(format!("=== UNOWNED FILES IN {} ===", dir.display()));

    let entries = fs::read_dir(dir).context("Failed to read directory")?;
    let mut unowned = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        // Check if file is owned by any RPM
        let output = Command::new("rpm").args(["-qf", "--"]).arg(&path).output();

        match output {
            Ok(o) if !o.status.success() => {
                // Not owned by any package
                unowned.push(path);
            }
            _ => {}
        }
    }

    if unowned.is_empty() {
        Output::success("All files are owned by packages.");
        Output::blank();
    } else {
        println!("{} ({}):", "Unowned files".yellow(), unowned.len());
        for path in &unowned {
            Output::list_item(path.display().to_string());
        }
        Output::blank();
    }

    Ok(())
}

pub fn run(args: ProfileArgs) -> Result<()> {
    match args.action {
        ProfileAction::Capture { output } => {
            let profile = capture_profile()?;
            let json = serde_json::to_string_pretty(&profile)?;

            if let Some(path) = output {
                std::fs::write(&path, &json)
                    .with_context(|| format!("Failed to write to {}", path.display()))?;
                Output::success(format!("Profile saved to {}", path.display()));
            } else {
                println!("{}", json);
            }
        }
        ProfileAction::Diff { section } => {
            show_diff(section.as_deref())?;
        }
        ProfileAction::Unowned { dir } => {
            show_unowned(&dir)?;
        }
    }
    Ok(())
}
