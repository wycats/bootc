//! Vendor artifact status command.
//!
//! Provides a user-facing way to inspect whether vendor-sourced packages
//! (such as VS Code) are newer upstream than the currently installed RPM.

use anyhow::{Context, Result, anyhow, bail};
use bkt_common::checksum::sha256_hex;
use bkt_common::manifest::{ArtifactKind, VendorArtifactsManifest, VendorSource};
use clap::{Args, Subcommand};
use owo_colors::OwoColorize;
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;

use crate::context::{CommandDomain, PrMode};
use crate::output::Output;
use crate::pipeline::ExecutionPlan;

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
    /// Try a vendor artifact update in the transient host overlay
    Try {
        /// Artifact name
        name: String,
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

pub fn run(args: VendorArtifactsArgs, plan: &ExecutionPlan) -> Result<()> {
    match args.action {
        VendorArtifactsAction::Status { name, json } => status(name, json),
        VendorArtifactsAction::Try { name } => try_artifact(&name, plan),
    }
}

fn status(name: Option<String>, json: bool) -> Result<()> {
    let manifest = load_manifest()?;

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

fn try_artifact(name: &str, plan: &ExecutionPlan) -> Result<()> {
    plan.validate_domain(CommandDomain::System)?;

    if plan.pr_mode == PrMode::PrOnly {
        bail!("vendor artifact try installs locally and does not support --pr-only");
    }

    if !plan.dry_run && !plan.should_execute_locally() {
        bail!("vendor artifact try installs locally and requires local execution");
    }

    let manifest = load_manifest()?;
    let artifact = manifest
        .find(name)
        .ok_or_else(|| anyhow!("vendor artifact '{}' not found", name))?;

    if artifact.kind != ArtifactKind::Rpm {
        bail!(
            "unsupported artifact kind '{}' for '{}'",
            artifact_kind(&artifact.kind),
            artifact.name
        );
    }

    validate_artifact_filename(&artifact.name)?;

    let resolved = resolve_artifact(artifact)?;
    let installed = installed_rpm_version(&artifact.name)?;
    let expected_prefix = format!("{}-", resolved.version);

    Output::header(format!("TRY VENDOR ARTIFACT: {}", artifact.name));
    Output::kv("Latest", &resolved.version);
    Output::kv("Installed", installed.as_deref().unwrap_or("not installed"));

    if installed
        .as_deref()
        .is_some_and(|version| version.starts_with(&expected_prefix))
    {
        Output::success(format!("{} is already current", artifact.name));
        return Ok(());
    }

    let temp_dir = vendor_temp_dir(&artifact.name);
    let rpm_path = temp_dir.join(format!("{}.rpm", artifact.name));
    let rpm_path_display = rpm_path.display().to_string();

    if plan.dry_run {
        Output::dry_run(format!("Would download {}", resolved.url));
        Output::dry_run(format!("Would verify SHA256 {}", resolved.sha256));
        Output::dry_run("Would unlock /usr overlay via rpm-ostree usroverlay");
        Output::dry_run("Would create /var/lib/rpm-state");
        Output::dry_run(format!(
            "Would install {} via dnf5 install -y",
            rpm_path_display
        ));
        return Ok(());
    }

    Output::info(format!(
        "Downloading {} v{}...",
        artifact.name, resolved.version
    ));
    let data = bkt_common::http::download(&resolved.url)
        .with_context(|| format!("failed to download vendor artifact '{}'", artifact.name))?;

    Output::info("Verifying SHA256...");
    let actual = sha256_hex(&data);
    if actual != resolved.sha256 {
        bail!(
            "SHA256 mismatch for {}: expected {}, got {}",
            artifact.name,
            resolved.sha256,
            actual
        );
    }

    std::fs::create_dir(&temp_dir)
        .with_context(|| format!("failed to create temp dir {}", temp_dir.display()))?;
    let cleanup = TempDirCleanup::new(temp_dir.clone());

    write_new_file(&rpm_path, &data)
        .with_context(|| format!("failed to write RPM to {}", rpm_path.display()))?;

    let runner = plan.runner();
    crate::commands::try_cmd::ensure_usroverlay(runner, false)?;
    crate::commands::try_cmd::ensure_rpm_state_dir(runner, false)?;

    Output::info(format!("Installing {} via dnf5...", artifact.name.cyan()));
    crate::commands::try_cmd::run_pkexec_status(
        runner,
        "/usr/bin/dnf5",
        &["install", "-y", &rpm_path_display],
    )?;

    drop(cleanup);

    Output::success(format!(
        "Installed {} {} in the transient overlay",
        artifact.name, resolved.version
    ));

    Ok(())
}

fn load_manifest() -> Result<VendorArtifactsManifest> {
    VendorArtifactsManifest::load_from(
        &crate::repo::find_repo_path()?.join(VendorArtifactsManifest::PROJECT_PATH),
    )
    .context("failed to load vendor artifacts manifest")
}

fn validate_artifact_filename(name: &str) -> Result<()> {
    if name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Ok(());
    }

    bail!(
        "invalid artifact name '{}': only [A-Za-z0-9_-] are allowed for local RPM filenames",
        name
    )
}

fn vendor_temp_dir(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("bkt-vendor-{}-{}", name, std::process::id()))
}

fn write_new_file(path: &std::path::Path, data: &[u8]) -> Result<()> {
    use std::io::Write;

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(data)?;
    file.sync_all()?;
    Ok(())
}

struct TempDirCleanup {
    path: PathBuf,
}

impl TempDirCleanup {
    fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl Drop for TempDirCleanup {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.path) {
            Output::warning(format!(
                "Failed to remove temporary directory {}: {}",
                self.path.display(),
                error
            ));
        }
    }
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
