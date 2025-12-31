//! Status command implementation.
//!
//! Shows an overview of all manifest types and their current state.

use crate::manifest::{
    FlatpakAppsManifest, GSettingsManifest, GnomeExtensionsManifest, ShimsManifest,
};
use crate::repo::find_repo_path;
use anyhow::Result;
use clap::Args;
use owo_colors::OwoColorize;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use tracing::debug;

#[derive(Debug, Args)]
pub struct StatusArgs {
    /// Output format (table, json)
    #[arg(short, long, default_value = "table")]
    format: String,
}

#[derive(Debug, serde::Serialize)]
struct StatusReport {
    flatpaks: FlatpakStatus,
    extensions: ExtensionStatus,
    gsettings: GSettingStatus,
    shims: ShimStatus,
    skel: SkelStatus,
}

#[derive(Debug, serde::Serialize)]
struct FlatpakStatus {
    total: usize,
    installed: usize,
    pending: usize,
}

#[derive(Debug, serde::Serialize)]
struct ExtensionStatus {
    total: usize,
    installed: usize,
    enabled: usize,
}

#[derive(Debug, serde::Serialize)]
struct GSettingStatus {
    total: usize,
    applied: usize,
}

#[derive(Debug, serde::Serialize)]
struct ShimStatus {
    total: usize,
    synced: usize,
}

#[derive(Debug, serde::Serialize)]
struct SkelStatus {
    total: usize,
    differs: usize,
}

/// Get skel directory path.
fn skel_dir() -> Option<PathBuf> {
    find_repo_path().ok().map(|p| p.join("skel"))
}

/// List files in skel directory.
fn list_skel_files(skel: &PathBuf) -> Vec<String> {
    let mut files = Vec::new();
    if let Ok(entries) = fs::read_dir(skel) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && entry.file_name() != ".gitkeep" {
                if let Some(name) = path.file_name() {
                    files.push(name.to_string_lossy().to_string());
                }
            }
        }
    }
    files
}

/// Check if a skel file differs from home.
fn skel_differs(skel_path: &PathBuf, home_path: &PathBuf) -> bool {
    if !home_path.exists() {
        return true; // Missing in home = differs
    }

    let skel_content = fs::read_to_string(skel_path).unwrap_or_default();
    let home_content = fs::read_to_string(home_path).unwrap_or_default();
    skel_content != home_content
}

/// Check if a flatpak is installed.
fn is_flatpak_installed(app_id: &str) -> bool {
    Command::new("flatpak")
        .args(["info", app_id])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Check if a GNOME extension is installed.
fn is_extension_installed(uuid: &str) -> bool {
    Command::new("gnome-extensions")
        .args(["info", uuid])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Check if a GNOME extension is enabled.
fn is_extension_enabled(uuid: &str) -> bool {
    let output = Command::new("gnome-extensions")
        .args(["info", uuid])
        .output();

    match output {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            stdout.contains("State: ENABLED") || stdout.contains("State: ACTIVE")
        }
        _ => false,
    }
}

/// Get current value of a gsetting.
fn get_gsetting(schema: &str, key: &str) -> Option<String> {
    Command::new("gsettings")
        .args(["get", schema, key])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

/// Get the shims directory.
fn shims_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/home".to_string());
    PathBuf::from(home).join(".local/bin")
}

pub fn run(args: StatusArgs) -> Result<()> {
    debug!("Gathering status information");

    // Gather flatpak status
    let flatpak_status = {
        let system = FlatpakAppsManifest::load_system().unwrap_or_default();
        let user = FlatpakAppsManifest::load_user().unwrap_or_default();
        let merged = FlatpakAppsManifest::merged(&system, &user);

        let total = merged.apps.len();
        let installed = merged
            .apps
            .iter()
            .filter(|a| is_flatpak_installed(&a.id))
            .count();
        let pending = total - installed;

        FlatpakStatus {
            total,
            installed,
            pending,
        }
    };

    // Gather extension status
    let extension_status = {
        let system = GnomeExtensionsManifest::load_system().unwrap_or_default();
        let user = GnomeExtensionsManifest::load_user().unwrap_or_default();
        let merged = GnomeExtensionsManifest::merged(&system, &user);

        let total = merged.extensions.len();
        let installed = merged
            .extensions
            .iter()
            .filter(|u| is_extension_installed(u))
            .count();
        let enabled = merged
            .extensions
            .iter()
            .filter(|u| is_extension_enabled(u))
            .count();

        ExtensionStatus {
            total,
            installed,
            enabled,
        }
    };

    // Gather gsettings status
    let gsetting_status = {
        let system = GSettingsManifest::load_system().unwrap_or_default();
        let user = GSettingsManifest::load_user().unwrap_or_default();
        let merged = GSettingsManifest::merged(&system, &user);

        let total = merged.settings.len();
        let applied = merged
            .settings
            .iter()
            .filter(|s| {
                get_gsetting(&s.schema, &s.key)
                    .map(|v| v == s.value)
                    .unwrap_or(false)
            })
            .count();

        GSettingStatus { total, applied }
    };

    // Gather shim status
    let shim_status = {
        let system = ShimsManifest::load_system().unwrap_or_default();
        let user = ShimsManifest::load_user().unwrap_or_default();
        let merged = ShimsManifest::merged(&system, &user);

        let shims_dir = shims_dir();
        let total = merged.shims.len();
        let synced = merged
            .shims
            .iter()
            .filter(|s| shims_dir.join(&s.name).exists())
            .count();

        ShimStatus { total, synced }
    };

    // Gather skel status
    let skel_status = {
        if let Some(skel) = skel_dir() {
            let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
            let files = list_skel_files(&skel);
            let total = files.len();
            let differs = files
                .iter()
                .filter(|f| skel_differs(&skel.join(f), &home.join(f)))
                .count();

            SkelStatus { total, differs }
        } else {
            SkelStatus {
                total: 0,
                differs: 0,
            }
        }
    };

    let report = StatusReport {
        flatpaks: flatpak_status,
        extensions: extension_status,
        gsettings: gsetting_status,
        shims: shim_status,
        skel: skel_status,
    };

    if args.format == "json" {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    // Table output
    println!("{}", "bkt status".bold());
    println!();

    // Flatpaks
    let flatpak_info = if report.flatpaks.pending > 0 {
        format!(
            "{} apps ({} pending sync)",
            report.flatpaks.total,
            report.flatpaks.pending.to_string().yellow()
        )
    } else {
        format!("{} apps (all synced)", report.flatpaks.total)
    };
    println!(
        "  {:<12} {}",
        "Flatpaks:".cyan(),
        flatpak_info
    );

    // Extensions
    let ext_info = format!(
        "{} in manifest, {} installed, {} enabled",
        report.extensions.total, report.extensions.installed, report.extensions.enabled
    );
    println!("  {:<12} {}", "Extensions:".cyan(), ext_info);

    // GSettings
    let gs_info = if report.gsettings.applied < report.gsettings.total {
        format!(
            "{} configured ({} applied)",
            report.gsettings.total, report.gsettings.applied
        )
    } else {
        format!("{} configured (all applied)", report.gsettings.total)
    };
    println!("  {:<12} {}", "GSettings:".cyan(), gs_info);

    // Shims
    let shim_info = if report.shims.synced < report.shims.total {
        format!(
            "{} configured ({} synced)",
            report.shims.total, report.shims.synced
        )
    } else {
        format!("{} synced", report.shims.total)
    };
    println!("  {:<12} {}", "Shims:".cyan(), shim_info);

    // Skel
    let skel_info = if report.skel.differs > 0 {
        format!(
            "{} files ({} differ from $HOME)",
            report.skel.total,
            report.skel.differs.to_string().yellow()
        )
    } else if report.skel.total > 0 {
        format!("{} files (all match $HOME)", report.skel.total)
    } else {
        "no files".dimmed().to_string()
    };
    println!("  {:<12} {}", "Skel:".cyan(), skel_info);

    Ok(())
}
