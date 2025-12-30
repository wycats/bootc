//! System profile command implementation.
//!
//! Captures current system state and compares against manifests.

use crate::manifest::{FlatpakAppsManifest, GnomeExtensionsManifest, GSettingsManifest};
use anyhow::{Context, Result};
use clap::{Args, Subcommand};
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
    Ok(stdout.lines().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
}

/// Get list of enabled GNOME extensions.
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
    // Remove brackets and split by comma
    let trimmed = stdout.trim_start_matches('[').trim_end_matches(']');
    if trimmed.is_empty() || trimmed == "@as" {
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
        println!("=== FLATPAK DIFF ===\n");
        
        let manifest = FlatpakAppsManifest::load_system()?;
        let installed = get_installed_flatpaks()?;
        
        let manifest_ids: HashSet<String> = manifest.apps.iter().map(|a| a.id.clone()).collect();
        let installed_ids: HashSet<String> = installed.iter().map(|a| a.id.clone()).collect();

        let in_manifest_not_installed: Vec<_> = manifest_ids.difference(&installed_ids).collect();
        let installed_not_in_manifest: Vec<_> = installed_ids.difference(&manifest_ids).collect();

        if in_manifest_not_installed.is_empty() && installed_not_in_manifest.is_empty() {
            println!("✓ No drift detected\n");
        } else {
            if !in_manifest_not_installed.is_empty() {
                println!("Missing (in manifest, not installed):");
                for id in &in_manifest_not_installed {
                    println!("  - {}", id);
                }
                println!();
            }
            if !installed_not_in_manifest.is_empty() {
                println!("Extra (installed, not in manifest):");
                for id in &installed_not_in_manifest {
                    println!("  + {}", id);
                }
                println!();
            }
        }
    }

    // Extension diff
    if show_all || section == "extension" || section == "ext" {
        println!("=== EXTENSION DIFF ===\n");
        
        let manifest = GnomeExtensionsManifest::load_system()?;
        let installed = get_installed_extensions()?;
        
        let manifest_set: HashSet<String> = manifest.extensions.into_iter().collect();
        let installed_set: HashSet<String> = installed.into_iter().collect();

        let in_manifest_not_installed: Vec<_> = manifest_set.difference(&installed_set).collect();
        let installed_not_in_manifest: Vec<_> = installed_set.difference(&manifest_set).collect();

        if in_manifest_not_installed.is_empty() && installed_not_in_manifest.is_empty() {
            println!("✓ No drift detected\n");
        } else {
            if !in_manifest_not_installed.is_empty() {
                println!("Missing (in manifest, not installed):");
                for uuid in &in_manifest_not_installed {
                    println!("  - {}", uuid);
                }
                println!();
            }
            if !installed_not_in_manifest.is_empty() {
                println!("Extra (installed, not in manifest):");
                for uuid in &installed_not_in_manifest {
                    println!("  + {}", uuid);
                }
                println!();
            }
        }
    }

    // GSettings diff
    if show_all || section == "gsetting" || section == "gs" {
        println!("=== GSETTINGS DIFF ===\n");
        
        let manifest = GSettingsManifest::load_system()?;
        
        if manifest.settings.is_empty() {
            println!("(no gsettings in manifest)\n");
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
                    if current_val != setting.value {
                        println!(
                            "≠ {}.{}\n  manifest: {}\n  current:  {}\n",
                            setting.schema, setting.key, setting.value, current_val
                        );
                        drifted += 1;
                    }
                } else {
                    println!("? {}.{} (could not read current value)\n", setting.schema, setting.key);
                    drifted += 1;
                }
            }

            if drifted == 0 {
                println!("✓ All {} settings match manifest\n", manifest.settings.len());
            }
        }
    }

    Ok(())
}

/// Show files not owned by any RPM package.
fn show_unowned(dir: &PathBuf) -> Result<()> {
    use std::fs;

    if !dir.exists() {
        println!("Directory does not exist: {}", dir.display());
        return Ok(());
    }

    println!("=== UNOWNED FILES IN {} ===\n", dir.display());

    let entries = fs::read_dir(dir).context("Failed to read directory")?;
    let mut unowned = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        // Check if file is owned by any RPM
        let output = Command::new("rpm")
            .args(["-qf", &path.to_string_lossy()])
            .output();

        match output {
            Ok(o) if !o.status.success() => {
                // Not owned by any package
                unowned.push(path);
            }
            _ => {}
        }
    }

    if unowned.is_empty() {
        println!("All files are owned by packages.\n");
    } else {
        println!("Unowned files ({}):", unowned.len());
        for path in &unowned {
            println!("  {}", path.display());
        }
        println!();
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
                println!("Profile saved to {}", path.display());
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
