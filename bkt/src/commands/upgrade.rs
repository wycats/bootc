//! `bkt upgrade` command — preview and apply system image updates.
//!
//! Shows what's staged for next boot (package diff, image info) and
//! optionally stages new updates from the registry.

use anyhow::{Context, Result};
use clap::Args;
use std::process::Command;

use crate::output::Output;

#[derive(Debug, Args)]
pub struct UpgradeArgs {
    /// Show what's staged for next boot (default behavior)
    #[arg(long)]
    pub preview: bool,

    /// Fetch and stage the latest image from the registry
    #[arg(long)]
    pub fetch: bool,
}

/// Deployment info parsed from rpm-ostree status --json
struct DeploymentInfo {
    version: String,
    checksum: String,
    image: String,
    timestamp: String,
}

pub fn run(args: UpgradeArgs) -> Result<()> {
    if args.fetch {
        return run_fetch();
    }

    // Default: preview
    run_preview()
}

fn run_preview() -> Result<()> {
    let output = Command::new("rpm-ostree")
        .args(["status", "--json"])
        .output()
        .context("Failed to run rpm-ostree status")?;

    if !output.status.success() {
        anyhow::bail!("rpm-ostree status failed");
    }

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).context("Failed to parse rpm-ostree status")?;

    let deployments = json["deployments"]
        .as_array()
        .context("No deployments found")?;

    let booted = deployments
        .iter()
        .find(|d| d["booted"].as_bool().unwrap_or(false))
        .context("No booted deployment found")?;

    let staged = deployments
        .iter()
        .find(|d| d["staged"].as_bool().unwrap_or(false));

    let booted_info = parse_deployment(booted);

    // Show booted info
    Output::info(format!(
        "Booted: {} ({})",
        booted_info.version, &booted_info.timestamp
    ));
    Output::info(format!("  Image: {}", booted_info.image));

    match staged {
        None => {
            Output::blank();
            Output::info("No update staged. Run `bkt upgrade --fetch` to check for updates.");
        }
        Some(staged_dep) => {
            let staged_info = parse_deployment(staged_dep);

            Output::blank();
            Output::info(format!(
                "Staged: {} ({})",
                staged_info.version, &staged_info.timestamp
            ));
            Output::info(format!("  Image: {}", staged_info.image));

            // Get the package diff
            Output::blank();
            show_package_diff(&booted_info.checksum, &staged_info.checksum)?;
        }
    }

    Ok(())
}

fn show_package_diff(booted_checksum: &str, staged_checksum: &str) -> Result<()> {
    let output = Command::new("rpm-ostree")
        .args(["db", "diff", booted_checksum, staged_checksum])
        .output()
        .context("Failed to run rpm-ostree db diff")?;

    if !output.status.success() {
        Output::info("Could not compute package diff");
        return Ok(());
    }

    let diff_text = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = diff_text.lines().collect();

    if lines.is_empty() {
        Output::info("No package changes");
        return Ok(());
    }

    // Packages we care about showing explicitly
    let notable_packages = [
        "code",
        "code-insiders",
        "microsoft-edge-stable",
        "1password",
        "1password-cli",
    ];

    // Parse the structured output from rpm-ostree db diff
    // It outputs sections like "Upgraded:", "Added:", "Removed:" followed by package lines
    let mut section = "";
    let mut up_count = 0;
    let mut add_count = 0;
    let mut rm_count = 0;
    let mut notable_lines: Vec<String> = Vec::new();

    for line in &lines {
        let trimmed = line.trim();
        if trimmed == "Upgraded:" {
            section = "upgraded";
            continue;
        } else if trimmed == "Added:" {
            section = "added";
            continue;
        } else if trimmed == "Removed:" {
            section = "removed";
            continue;
        }

        if trimmed.is_empty() {
            continue;
        }

        match section {
            "upgraded" => {
                up_count += 1;
                if is_notable(trimmed, &notable_packages) {
                    notable_lines.push(format!("  {} (upgraded)", trimmed));
                }
            }
            "added" => {
                add_count += 1;
                if is_notable(trimmed, &notable_packages) {
                    notable_lines.push(format!("  {} (added)", trimmed));
                }
            }
            "removed" => {
                rm_count += 1;
                if is_notable(trimmed, &notable_packages) {
                    notable_lines.push(format!("  {} (removed)", trimmed));
                }
            }
            _ => {}
        }
    }

    Output::info(format!(
        "Package changes: {} upgraded, {} added, {} removed",
        up_count, add_count, rm_count
    ));

    if !notable_lines.is_empty() {
        Output::blank();
        Output::info("Notable:");
        for line in &notable_lines {
            Output::info(line);
        }
    }

    Output::blank();
    Output::info("Run `rpm-ostree db diff` for the full package list.");
    Output::info("Reboot to apply.");

    Ok(())
}

fn run_fetch() -> Result<()> {
    Output::info("Fetching latest image...");

    let status = Command::new("bootc")
        .args(["upgrade"])
        .status()
        .context("Failed to run bootc upgrade")?;

    if !status.success() {
        anyhow::bail!("bootc upgrade failed");
    }

    Output::blank();
    Output::info("Run `bkt upgrade` to preview what changed.");

    Ok(())
}

/// Check if a package line's name matches a notable package exactly.
fn is_notable(line: &str, notable: &[&str]) -> bool {
    let pkg_name = line.split_whitespace().next().unwrap_or("");
    notable.contains(&pkg_name)
}

fn parse_deployment(dep: &serde_json::Value) -> DeploymentInfo {
    DeploymentInfo {
        version: dep["version"].as_str().unwrap_or("unknown").to_string(),
        checksum: dep["checksum"].as_str().unwrap_or("").to_string(),
        image: dep["container-image-reference"]
            .as_str()
            .unwrap_or("unknown")
            .to_string(),
        timestamp: dep["timestamp"]
            .as_u64()
            .map(|ts| {
                chrono::DateTime::from_timestamp(ts as i64, 0)
                    .map(|dt| dt.format("%Y-%m-%d %H:%M UTC").to_string())
                    .unwrap_or_else(|| ts.to_string())
            })
            .or_else(|| dep["version"].as_str().map(String::from))
            .unwrap_or_default(),
    }
}
