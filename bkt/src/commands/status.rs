//! Status command implementation.
//!
//! The **Daily Loop Hub**: shows system status, manifest status, drift detection,
//! and next actions in a single unified view.
//!
//! This is the entry point for the workflow:
//! 1. `bkt status` - See where you are
//! 2. Decide what to do (upgrade, apply, capture)
//! 3. Act
//! 4. Back to `bkt status`

use crate::component::SystemComponent;
use crate::output::Output;
use crate::repo::find_repo_path;
use anyhow::Result;
use clap::{Args, ValueEnum};
use owo_colors::OwoColorize;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use tracing::debug;

use super::dnf::get_layered_packages;
use super::extension::ExtensionComponent;
use super::flatpak::FlatpakComponent;
use super::gsetting::GSettingComponent;
use super::shim::ShimComponent;

#[derive(Debug, Clone, Copy, ValueEnum, Default)]
pub enum OutputFormat {
    /// Human-readable table output
    #[default]
    Table,
    /// JSON output for scripting
    Json,
}

#[derive(Debug, Args)]
pub struct StatusArgs {
    /// Output format
    #[arg(short, long, value_enum, default_value = "table")]
    format: OutputFormat,

    /// Show verbose output with more details
    #[arg(short, long)]
    verbose: bool,

    /// Skip OS status (faster, useful in toolbox)
    #[arg(long)]
    skip_os: bool,
}

#[derive(Debug, serde::Serialize)]
pub struct StatusReport {
    /// OS/bootc status information
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os: Option<OsStatus>,
    /// Manifest subsystem status
    pub manifests: ManifestStatus,
    /// Drift detection results
    pub drift: DriftStatus,
    /// Suggested next actions
    pub next_actions: Vec<NextAction>,
}

/// OS-level status from bootc/rpm-ostree
#[derive(Debug, serde::Serialize)]
pub struct OsStatus {
    /// Currently booted image
    pub image: Option<String>,
    /// Currently booted version
    pub version: Option<String>,
    /// Image digest/checksum
    pub checksum: Option<String>,
    /// Whether an update is staged for next boot
    pub staged_update: Option<StagedUpdate>,
    /// Layered packages (rpm-ostree)
    pub layered_packages: Vec<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct StagedUpdate {
    pub version: Option<String>,
    pub checksum: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct ManifestStatus {
    pub flatpaks: FlatpakStatus,
    pub extensions: ExtensionStatus,
    pub gsettings: GSettingStatus,
    pub shims: ShimStatus,
    pub skel: SkelStatus,
}

#[derive(Debug, serde::Serialize)]
pub struct FlatpakStatus {
    total: usize,
    installed: usize,
    pending: usize,
    /// Flatpaks installed on system but not in manifest
    untracked: usize,
}

#[derive(Debug, serde::Serialize)]
pub struct ExtensionStatus {
    total: usize,
    installed: usize,
    enabled: usize,
    /// Extensions enabled but not in manifest
    untracked: usize,
}

#[derive(Debug, serde::Serialize)]
pub struct GSettingStatus {
    total: usize,
    applied: usize,
    /// Settings whose current system value differs from the manifest value
    /// (need to be synced back to match manifest, not captured)
    drifted: usize,
}

#[derive(Debug, serde::Serialize)]
pub struct ShimStatus {
    total: usize,
    synced: usize,
}

#[derive(Debug, serde::Serialize)]
pub struct SkelStatus {
    total: usize,
    differs: usize,
}

/// Aggregated drift status
#[derive(Debug, serde::Serialize)]
pub struct DriftStatus {
    /// Whether any drift was detected
    pub has_drift: bool,
    /// Count of items that need sync (manifest → system)
    pub pending_sync: usize,
    /// Count of items that need capture (system → manifest)
    pub pending_capture: usize,
}

/// A suggested next action
#[derive(Debug, serde::Serialize)]
pub struct NextAction {
    /// Short description
    pub description: String,
    /// The command to run
    pub command: String,
    /// Priority (lower = more important)
    pub priority: u8,
}

/// Get OS status from rpm-ostree
fn get_os_status() -> Option<OsStatus> {
    let output = Command::new("rpm-ostree")
        .args(["status", "--json"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    let deployments = json.get("deployments")?.as_array()?;

    // Find booted deployment
    let booted = deployments
        .iter()
        .find(|d| d.get("booted").and_then(|b| b.as_bool()).unwrap_or(false))?;

    // Find staged deployment (if any)
    let staged = deployments
        .iter()
        .find(|d| d.get("staged").and_then(|b| b.as_bool()).unwrap_or(false));

    let staged_update = staged.map(|s| StagedUpdate {
        version: s.get("version").and_then(|v| v.as_str()).map(String::from),
        checksum: s.get("checksum").and_then(|v| v.as_str()).map(String::from),
    });

    let layered = get_layered_packages();

    Some(OsStatus {
        image: booted
            .get("container-image-reference")
            .or_else(|| booted.get("origin"))
            .and_then(|v| v.as_str())
            .map(String::from),
        version: booted
            .get("version")
            .and_then(|v| v.as_str())
            .map(String::from),
        checksum: booted
            .get("checksum")
            .and_then(|v| v.as_str())
            .map(|s| s.chars().take(12).collect()),
        staged_update,
        layered_packages: layered,
    })
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
            if path.is_file()
                && entry.file_name() != ".gitkeep"
                && let Some(name) = path.file_name()
            {
                files.push(name.to_string_lossy().to_string());
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

pub fn run(args: StatusArgs) -> Result<()> {
    debug!("Gathering status information");

    // Gather OS status (unless skipped)
    let os_status = if args.skip_os { None } else { get_os_status() };

    // Gather flatpak status using SystemComponent trait
    let flatpak_status = {
        let component = FlatpakComponent::new();
        let system = component.scan_system()?;
        let manifest = component.load_manifest()?;
        let drift = component.diff(&system, &manifest);

        FlatpakStatus {
            total: drift.synced_count + drift.to_install.len(),
            installed: drift.synced_count,
            pending: drift.to_install.len(),
            untracked: drift.untracked.len(),
        }
    };

    // Gather extension status using SystemComponent trait
    let extension_status = {
        let component = ExtensionComponent::new();
        let system = component.scan_system()?;
        let manifest = component.load_manifest()?;
        let drift = component.diff(&system, &manifest);

        // For extensions, "installed" means present on system, "enabled" means in sync with manifest
        let total = drift.synced_count + drift.to_install.len() + drift.to_update.len();
        let installed = system.len();
        let enabled = drift.synced_count; // Extensions that match manifest state

        ExtensionStatus {
            total,
            installed,
            enabled,
            untracked: drift.untracked.len(),
        }
    };

    // Gather gsettings status using SystemComponent trait
    let gsetting_status = {
        let component = GSettingComponent::new();
        let system = component.scan_system()?;
        let manifest = component.load_manifest()?;
        let drift = component.diff(&system, &manifest);

        GSettingStatus {
            total: drift.synced_count + drift.to_install.len() + drift.to_update.len(),
            applied: drift.synced_count,
            drifted: drift.to_install.len() + drift.to_update.len(),
        }
    };

    // Gather shim status using SystemComponent trait
    let shim_status = {
        let component = ShimComponent::new();
        let system = component.scan_system()?;
        let manifest = component.load_manifest()?;
        let drift = component.diff(&system, &manifest);

        ShimStatus {
            total: drift.synced_count + drift.to_install.len(),
            synced: drift.synced_count,
        }
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

    let manifest_status = ManifestStatus {
        flatpaks: flatpak_status,
        extensions: extension_status,
        gsettings: gsetting_status,
        shims: shim_status,
        skel: skel_status,
    };

    // Calculate drift
    // pending_sync: items that need to be applied from manifest → system
    // pending_capture: items on system that aren't in manifest (untracked)
    let pending_sync = manifest_status.flatpaks.pending
        + (manifest_status.extensions.total - manifest_status.extensions.enabled)
        + manifest_status.gsettings.drifted // drifted = needs sync, not capture
        + (manifest_status.shims.total - manifest_status.shims.synced)
        + manifest_status.skel.differs;

    let pending_capture = manifest_status.flatpaks.untracked + manifest_status.extensions.untracked;

    let drift_status = DriftStatus {
        has_drift: pending_sync > 0 || pending_capture > 0,
        pending_sync,
        pending_capture,
    };

    // Generate next actions
    let mut next_actions = Vec::new();

    if let Some(ref os) = os_status
        && os.staged_update.is_some()
    {
        next_actions.push(NextAction {
            description: "Reboot to apply staged OS update".to_string(),
            command: "systemctl reboot".to_string(),
            priority: 1,
        });
    }

    if pending_sync > 0 {
        next_actions.push(NextAction {
            description: format!("Sync {} items from manifests to system", pending_sync),
            command: "bkt apply".to_string(),
            priority: 2,
        });
    }

    if pending_capture > 0 {
        next_actions.push(NextAction {
            description: format!("Capture {} untracked items to manifests", pending_capture),
            command: "bkt capture".to_string(),
            priority: 3,
        });
    }

    // Sort by priority
    next_actions.sort_by_key(|a| a.priority);

    let report = StatusReport {
        os: os_status,
        manifests: manifest_status,
        drift: drift_status,
        next_actions,
    };

    match args.format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        OutputFormat::Table => {
            print_table_output(&report, args.verbose);
        }
    }

    Ok(())
}

fn print_table_output(report: &StatusReport, verbose: bool) {
    Output::header("bkt status");
    Output::blank();

    // OS Section
    if let Some(ref os) = report.os {
        println!("{}", "  OS".bold());

        if let Some(ref version) = os.version {
            println!("    {} {}", "Version:".dimmed(), version);
        }
        if let Some(ref image) = os.image {
            // Truncate long image refs for display (Unicode-safe)
            let display_image = {
                let char_count = image.chars().count();
                if char_count > 60 {
                    // Keep the last 57 characters
                    let tail: String = image.chars().skip(char_count - 57).collect();
                    format!("...{}", tail)
                } else {
                    image.clone()
                }
            };
            println!("    {} {}", "Image:".dimmed(), display_image);
        }
        if let Some(ref checksum) = os.checksum {
            println!("    {} {}", "Checksum:".dimmed(), checksum);
        }

        if let Some(ref staged) = os.staged_update {
            let staged_info = staged
                .version
                .as_ref()
                .map(|v| v.to_string())
                .unwrap_or_else(|| "update".to_string());
            println!("    {} {}", "Staged:".dimmed(), staged_info.green().bold());
        }

        if !os.layered_packages.is_empty() {
            println!(
                "    {} {} packages",
                "Layered:".dimmed(),
                os.layered_packages.len()
            );
            if verbose {
                for pkg in &os.layered_packages {
                    println!("      - {}", pkg.dimmed());
                }
            }
        }

        Output::blank();
    }

    // Manifests Section
    println!("{}", "  Manifests".bold());

    // Flatpaks
    let flatpak_info = if report.manifests.flatpaks.pending > 0 {
        format!(
            "{} apps ({} to install)",
            report.manifests.flatpaks.total,
            report.manifests.flatpaks.pending.to_string().yellow()
        )
    } else {
        format!("{} apps {}", report.manifests.flatpaks.total, "✓".green())
    };
    let untracked_flatpak = if report.manifests.flatpaks.untracked > 0 {
        format!(
            " | {} untracked",
            report.manifests.flatpaks.untracked.to_string().cyan()
        )
    } else {
        String::new()
    };
    println!(
        "    {:<12} {}{}",
        "Flatpaks:".dimmed(),
        flatpak_info,
        untracked_flatpak
    );

    // Extensions
    let ext_pending = report.manifests.extensions.total - report.manifests.extensions.enabled;
    let ext_info = if ext_pending > 0 {
        format!(
            "{} extensions ({} to enable)",
            report.manifests.extensions.total,
            ext_pending.to_string().yellow()
        )
    } else {
        format!(
            "{} extensions {}",
            report.manifests.extensions.total,
            "✓".green()
        )
    };
    let untracked_ext = if report.manifests.extensions.untracked > 0 {
        format!(
            " | {} untracked",
            report.manifests.extensions.untracked.to_string().cyan()
        )
    } else {
        String::new()
    };
    println!(
        "    {:<12} {}{}",
        "Extensions:".dimmed(),
        ext_info,
        untracked_ext
    );

    // GSettings
    let gs_pending = report.manifests.gsettings.total - report.manifests.gsettings.applied;
    let gs_info = if gs_pending > 0 {
        format!(
            "{} settings ({} to apply)",
            report.manifests.gsettings.total,
            gs_pending.to_string().yellow()
        )
    } else {
        format!(
            "{} settings {}",
            report.manifests.gsettings.total,
            "✓".green()
        )
    };
    let drifted_gs = if report.manifests.gsettings.drifted > 0 {
        format!(
            " | {} drifted",
            report.manifests.gsettings.drifted.to_string().cyan()
        )
    } else {
        String::new()
    };
    println!(
        "    {:<12} {}{}",
        "GSettings:".dimmed(),
        gs_info,
        drifted_gs
    );

    // Shims
    let shim_pending = report.manifests.shims.total - report.manifests.shims.synced;
    let shim_info = if shim_pending > 0 {
        format!(
            "{} shims ({} to sync)",
            report.manifests.shims.total,
            shim_pending.to_string().yellow()
        )
    } else if report.manifests.shims.total > 0 {
        format!("{} shims {}", report.manifests.shims.total, "✓".green())
    } else {
        "no shims".dimmed().to_string()
    };
    println!("    {:<12} {}", "Shims:".dimmed(), shim_info);

    // Skel
    let skel_info = if report.manifests.skel.differs > 0 {
        format!(
            "{} files ({} differ)",
            report.manifests.skel.total,
            report.manifests.skel.differs.to_string().yellow()
        )
    } else if report.manifests.skel.total > 0 {
        format!("{} files {}", report.manifests.skel.total, "✓".green())
    } else {
        "no files".dimmed().to_string()
    };
    println!("    {:<12} {}", "Skel:".dimmed(), skel_info);

    // Drift Detection Section - show untracked items that need capture
    if report.drift.pending_capture > 0 {
        Output::blank();
        println!("{} {}", "⚠️".yellow(), "Drift Detected".yellow().bold());

        // Show specific drift items
        if report.manifests.flatpaks.untracked > 0 {
            let plural = if report.manifests.flatpaks.untracked == 1 {
                "flatpak"
            } else {
                "flatpaks"
            };
            println!(
                "    {} {} installed but not in manifest",
                report
                    .manifests
                    .flatpaks
                    .untracked
                    .to_string()
                    .cyan()
                    .bold(),
                plural
            );
        }

        if report.manifests.extensions.untracked > 0 {
            let plural = if report.manifests.extensions.untracked == 1 {
                "extension"
            } else {
                "extensions"
            };
            println!(
                "    {} {} enabled but not in manifest",
                report
                    .manifests
                    .extensions
                    .untracked
                    .to_string()
                    .cyan()
                    .bold(),
                plural
            );
        }

        Output::blank();
        println!(
            "    Run {} to import these changes.",
            "bkt capture".cyan().bold()
        );
    }

    // Next Actions Section
    if !report.next_actions.is_empty() {
        Output::blank();
        println!("{}", "  Next Actions".bold());

        for action in &report.next_actions {
            println!("    {} {}", "→".cyan(), action.description);
            println!("      {}", action.command.dimmed());
        }
    } else {
        Output::blank();
        println!("  {} {}", "✓".green().bold(), "All synced!".green());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drift_status_has_drift_when_pending_sync() {
        let drift = DriftStatus {
            has_drift: true,
            pending_sync: 5,
            pending_capture: 0,
        };
        assert!(drift.has_drift);
        assert_eq!(drift.pending_sync, 5);
    }

    #[test]
    fn test_drift_status_has_drift_when_pending_capture() {
        let drift = DriftStatus {
            has_drift: true,
            pending_sync: 0,
            pending_capture: 3,
        };
        assert!(drift.has_drift);
        assert_eq!(drift.pending_capture, 3);
    }

    #[test]
    fn test_drift_status_no_drift_when_synced() {
        let drift = DriftStatus {
            has_drift: false,
            pending_sync: 0,
            pending_capture: 0,
        };
        assert!(!drift.has_drift);
    }

    #[test]
    fn test_next_action_priority_ordering() {
        let mut actions = vec![
            NextAction {
                description: "Low priority".to_string(),
                command: "cmd3".to_string(),
                priority: 3,
            },
            NextAction {
                description: "High priority".to_string(),
                command: "cmd1".to_string(),
                priority: 1,
            },
            NextAction {
                description: "Medium priority".to_string(),
                command: "cmd2".to_string(),
                priority: 2,
            },
        ];

        actions.sort_by_key(|a| a.priority);

        assert_eq!(actions[0].command, "cmd1");
        assert_eq!(actions[1].command, "cmd2");
        assert_eq!(actions[2].command, "cmd3");
    }

    #[test]
    fn test_os_status_serialization() {
        let os = OsStatus {
            image: Some("ghcr.io/myimage:latest".to_string()),
            version: Some("1.0.0".to_string()),
            checksum: Some("abc123".to_string()),
            staged_update: None,
            layered_packages: vec!["pkg1".to_string(), "pkg2".to_string()],
        };

        let json = serde_json::to_string(&os).unwrap();
        assert!(json.contains("ghcr.io/myimage:latest"));
        assert!(json.contains("1.0.0"));
        assert!(json.contains("pkg1"));
    }

    #[test]
    fn test_os_status_with_staged_update() {
        let os = OsStatus {
            image: Some("ghcr.io/myimage:latest".to_string()),
            version: Some("1.0.0".to_string()),
            checksum: Some("abc123".to_string()),
            staged_update: Some(StagedUpdate {
                version: Some("1.1.0".to_string()),
                checksum: Some("def456".to_string()),
            }),
            layered_packages: vec![],
        };

        let json = serde_json::to_string(&os).unwrap();
        assert!(json.contains("staged_update"));
        assert!(json.contains("1.1.0"));
    }

    #[test]
    fn test_manifest_status_counts() {
        let manifest = ManifestStatus {
            flatpaks: FlatpakStatus {
                total: 10,
                installed: 8,
                pending: 2,
                untracked: 3,
            },
            extensions: ExtensionStatus {
                total: 5,
                installed: 4,
                enabled: 3,
                untracked: 1,
            },
            gsettings: GSettingStatus {
                total: 20,
                applied: 18,
                drifted: 2,
            },
            shims: ShimStatus {
                total: 3,
                synced: 3,
            },
            skel: SkelStatus {
                total: 2,
                differs: 0,
            },
        };

        // Pending sync = pending flatpaks + (extensions not enabled) + drifted gsettings + (shims not synced) + skel differs
        let pending_sync = manifest.flatpaks.pending
            + (manifest.extensions.total - manifest.extensions.enabled)
            + manifest.gsettings.drifted // drifted = needs sync, not capture
            + (manifest.shims.total - manifest.shims.synced)
            + manifest.skel.differs;

        assert_eq!(pending_sync, 2 + 2 + 2 + 0 + 0); // 6

        // Pending capture = untracked flatpaks + untracked extensions (NOT drifted gsettings)
        let pending_capture = manifest.flatpaks.untracked + manifest.extensions.untracked;

        assert_eq!(pending_capture, 3 + 1); // 4
    }

    #[test]
    fn test_status_report_json_serialization() {
        let report = StatusReport {
            os: None,
            manifests: ManifestStatus {
                flatpaks: FlatpakStatus {
                    total: 0,
                    installed: 0,
                    pending: 0,
                    untracked: 0,
                },
                extensions: ExtensionStatus {
                    total: 0,
                    installed: 0,
                    enabled: 0,
                    untracked: 0,
                },
                gsettings: GSettingStatus {
                    total: 0,
                    applied: 0,
                    drifted: 0,
                },
                shims: ShimStatus {
                    total: 0,
                    synced: 0,
                },
                skel: SkelStatus {
                    total: 0,
                    differs: 0,
                },
            },
            drift: DriftStatus {
                has_drift: false,
                pending_sync: 0,
                pending_capture: 0,
            },
            next_actions: vec![],
        };

        let json = serde_json::to_string_pretty(&report).unwrap();
        assert!(json.contains("manifests"));
        assert!(json.contains("drift"));
        assert!(json.contains("next_actions"));
        // os should be omitted when None
        assert!(!json.contains("\"os\""));
    }
}
