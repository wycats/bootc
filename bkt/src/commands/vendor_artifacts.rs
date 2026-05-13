//! Vendor artifact status command.
//!
//! Provides a user-facing way to inspect whether vendor-sourced packages
//! (such as VS Code) are newer upstream than the currently installed RPM.

use anyhow::{Context, Result, anyhow, bail};
use bkt_common::manifest::{ArtifactKind, VendorArtifactsManifest, VendorSource};
use clap::{Args, Subcommand};
use owo_colors::OwoColorize;
use serde::Serialize;
use std::collections::HashMap;
use std::process::Command;

use crate::output::Output;

#[derive(Debug, Args)]
pub struct VendorArtifactsArgs {
    #[command(subcommand)]
    pub action: VendorArtifactsAction,
}

#[derive(Debug, Subcommand)]
pub enum VendorArtifactsAction {
    /// Show vendor artifact freshness status
    Status {
        /// Artifact name (shows all if omitted)
        name: Option<String>,
        /// Output JSON instead of a table
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Serialize)]
struct ArtifactStatus {
    name: String,
    kind: String,
    installed: Option<String>,
    latest: String,
    latest_url: String,
    vendor_revision: Option<String>,
    stale: bool,
}

pub fn run(args: VendorArtifactsArgs) -> Result<()> {
    match args.action {
        VendorArtifactsAction::Status { name, json } => status(name, json),
    }
}

fn status(name: Option<String>, json: bool) -> Result<()> {
    let manifest = VendorArtifactsManifest::load_from(
        &crate::repo::find_repo_path()?.join(VendorArtifactsManifest::PROJECT_PATH),
    )?;

    let artifacts: Vec<_> = match name {
        Some(name) => vec![
            manifest
                .find(&name)
                .ok_or_else(|| anyhow!("vendor artifact '{}' not found", name))?,
        ],
        None => manifest.artifacts.iter().collect(),
    };

    let mut statuses = Vec::new();
    for artifact in artifacts {
        let resolved = resolve_artifact(artifact)?;
        let installed = installed_rpm_version(&artifact.name)?;
        let stale = installed
            .as_deref()
            .is_none_or(|version| !version.starts_with(&format!("{}-", resolved.version)));

        statuses.push(ArtifactStatus {
            name: artifact.name.clone(),
            kind: artifact_kind(&artifact.kind).to_string(),
            installed,
            latest: resolved.version,
            latest_url: resolved.url,
            vendor_revision: resolved.vendor_revision,
            stale,
        });
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&statuses)?);
        return Ok(());
    }

    if statuses.is_empty() {
        Output::info("No vendor artifacts configured.");
        return Ok(());
    }

    Output::header("VENDOR ARTIFACTS");
    println!(
        "{:<18} {:<14} {:<14} STATUS",
        "NAME".cyan(),
        "INSTALLED".cyan(),
        "LATEST".cyan()
    );
    Output::separator();

    for status in &statuses {
        let installed = status.installed.as_deref().unwrap_or("not installed");
        let state = if status.stale {
            "update available".yellow().to_string()
        } else {
            "current".green().to_string()
        };
        println!(
            "{:<18} {:<14} {:<14} {}",
            status.name.yellow(),
            installed,
            status.latest,
            state
        );
    }

    let stale_count = statuses.iter().filter(|s| s.stale).count();
    Output::blank();
    if stale_count > 0 {
        Output::warning(format!(
            "{} vendor artifact(s) have updates available. A new image build is needed.",
            stale_count
        ));
        Output::hint(
            "Run `gh workflow run build-and-push --repo wycats/bootc --ref main` to force a build now.",
        );
    } else {
        Output::success("All vendor artifacts are current.");
    }

    Ok(())
}

fn resolve_artifact(
    artifact: &bkt_common::manifest::VendorArtifact,
) -> Result<bkt_common::manifest::ResolvedVendorArtifact> {
    match &artifact.source {
        VendorSource::VendorFeed {
            url,
            params,
            platforms,
            response_map,
        } => {
            let mut template_params = params.clone();
            if let Some(platform) = platforms.get(std::env::consts::ARCH) {
                template_params.insert("platform".to_string(), platform.clone());
            } else if !platforms.is_empty() {
                bail!(
                    "no platform mapping for architecture '{}' in artifact '{}'",
                    std::env::consts::ARCH,
                    artifact.name
                );
            }

            let endpoint = expand_template(url, &template_params)?;
            let body: serde_json::Value = bkt_common::http::download_json(&endpoint, &[])
                .with_context(|| format!("failed to fetch vendor feed for '{}'", artifact.name))?;

            let extract = |field: &str, label: &str| -> Result<String> {
                body[field].as_str().map(String::from).ok_or_else(|| {
                    anyhow!(
                        "vendor response for '{}' missing field '{}' (mapped from '{}')",
                        artifact.name,
                        field,
                        label
                    )
                })
            };

            Ok(bkt_common::manifest::ResolvedVendorArtifact {
                name: artifact.name.clone(),
                kind: artifact_kind(&artifact.kind).to_string(),
                version: extract(&response_map.version, "version")?,
                url: extract(&response_map.url, "url")?,
                sha256: extract(&response_map.sha256, "sha256")?,
                vendor_revision: response_map
                    .vendor_revision
                    .as_ref()
                    .and_then(|field| body[field].as_str().map(String::from)),
            })
        }
    }
}

fn expand_template(template: &str, params: &HashMap<String, String>) -> Result<String> {
    let mut result = template.to_string();
    let mut pos = 0;

    while let Some(start) = result[pos..].find('{') {
        let abs_start = pos + start;
        if let Some(end) = result[abs_start..].find('}') {
            let abs_end = abs_start + end;
            let param_name = &result[abs_start + 1..abs_end];
            let value = params.get(param_name).ok_or_else(|| {
                anyhow!(
                    "missing template parameter '{}' (available: {})",
                    param_name,
                    params.keys().cloned().collect::<Vec<_>>().join(", ")
                )
            })?;
            result.replace_range(abs_start..=abs_end, value);
            pos = abs_start + value.len();
        } else {
            bail!("unmatched '{{' in URL template at position {}", abs_start);
        }
    }

    Ok(result)
}

fn installed_rpm_version(name: &str) -> Result<Option<String>> {
    let output = Command::new("rpm")
        .args(["-q", name, "--qf", "%{VERSION}-%{RELEASE}"])
        .output()
        .with_context(|| format!("failed to query installed RPM '{}'", name))?;

    if output.status.success() {
        Ok(Some(
            String::from_utf8_lossy(&output.stdout).trim().to_string(),
        ))
    } else {
        Ok(None)
    }
}

fn artifact_kind(kind: &ArtifactKind) -> &'static str {
    match kind {
        ArtifactKind::Rpm => "rpm",
    }
}
