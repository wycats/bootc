//! Build info command implementation.
//!
//! This module provides commands for generating and rendering build descriptions
//! as specified in RFC-0013.

use anyhow::{Context, Result, bail};
use chrono::Utc;
use clap::{Args, Subcommand};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::manifest::base_image;
use crate::manifest::build_info::{
    AppImageDiff, BuildInfo, BuildMetadata, ExtensionDiff, FlatpakAppDiff, FlatpakRemoteDiff,
    GSettingDiff, ManifestDiffs, ShimDiff, SystemConfigDiffs, SystemConfigEntry,
    SystemConfigModified, UpstreamChanges, convert_diff_result,
};
use crate::manifest::diff::{DiffResult, diff_collections, diff_string_sets};
use crate::manifest::parsers::{ConfigFileType, compute_semantic_diff};
use crate::manifest::{
    AppImageApp, AppImageAppsManifest, ExtensionItem, FlatpakApp, FlatpakAppsManifest,
    FlatpakRemote, FlatpakRemotesManifest, GSetting, GSettingsManifest, GnomeExtensionsManifest,
    Shim, ShimsManifest,
};
use crate::output::Output;
use crate::repo::find_repo_path;

#[derive(Debug, Args)]
pub struct BuildInfoArgs {
    #[command(subcommand)]
    pub action: BuildInfoAction,
}

#[derive(Debug, Subcommand)]
pub enum BuildInfoAction {
    /// Generate build info comparing two commits
    Generate {
        /// Starting commit (default: HEAD~1)
        #[arg(long)]
        from: Option<String>,

        /// Ending commit (default: HEAD)
        #[arg(long)]
        to: Option<String>,

        /// Output file (default: stdout)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// Render build info JSON to Markdown
    Render {
        /// Path to build-info.json
        input: PathBuf,

        /// Output file (default: stdout)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// Generate a short summary for OCI annotation (max 512 chars)
    Summary {
        /// Path to build-info.json
        input: PathBuf,

        /// Maximum length (default: 512)
        #[arg(long, default_value = "512")]
        max_length: usize,
    },
}

pub fn run(args: BuildInfoArgs) -> Result<()> {
    match args.action {
        BuildInfoAction::Generate { from, to, output } => generate(from, to, output),
        BuildInfoAction::Render { input, output } => render(input, output),
        BuildInfoAction::Summary { input, max_length } => summary(input, max_length),
    }
}

// ============================================================================
// Generate implementation
// ============================================================================

fn generate(from: Option<String>, to: Option<String>, output: Option<PathBuf>) -> Result<()> {
    let repo_path = find_repo_path()?;

    // Check for shallow clone when using default from ref
    let from_ref = match (&from, is_shallow_clone(&repo_path)?) {
        (Some(f), _) => f.as_str(),
        (None, true) => {
            bail!(
                "Shallow clone detected. Use --from <commit> --to <commit> explicitly, \
                 or fetch full history with: git fetch --unshallow"
            );
        }
        (None, false) => "HEAD~1",
    };
    let to_ref = to.as_deref().unwrap_or("HEAD");

    // Resolve to actual commit hashes
    let from_commit = resolve_commit(&repo_path, from_ref)?;
    let to_commit = resolve_commit(&repo_path, to_ref)?;

    Output::info(format!(
        "Comparing {} → {}",
        &from_commit[..8.min(from_commit.len())],
        &to_commit[..8.min(to_commit.len())]
    ));

    // Generate manifest diffs
    let manifests = diff_all_manifests(&repo_path, &from_commit, &to_commit)?;

    // Generate system config diffs
    let system_config = diff_system_config(&repo_path, &from_commit, &to_commit)?;

    // Generate upstream changes (base image diff)
    let upstream = diff_upstream_changes(&repo_path, &from_commit, &to_commit)?;

    // Build the build info
    let mut build_info = BuildInfo::new(
        BuildMetadata {
            commit: to_commit.clone(),
            timestamp: Utc::now(),
            previous_commit: Some(from_commit),
        },
        manifests,
    );

    // Add system config if there are changes
    if !system_config.is_empty() {
        build_info.system_config = Some(system_config);
    }

    // Add upstream changes if present
    if let Some(upstream_changes) = upstream {
        build_info.upstream = Some(upstream_changes);
    }

    // Serialize and output
    let json = serde_json::to_string_pretty(&build_info)
        .context("Failed to serialize build info to JSON")?;

    write_output(&json, output)?;

    if build_info.is_empty() {
        Output::info("No manifest changes detected.");
    } else {
        Output::success("Build info generated.");
    }

    Ok(())
}

fn is_shallow_clone(repo_path: &Path) -> Result<bool> {
    let shallow_file = repo_path.join(".git/shallow");
    Ok(shallow_file.exists())
}

fn resolve_commit(repo_path: &PathBuf, ref_spec: &str) -> Result<String> {
    let output = Command::new("git")
        .args(["rev-parse", ref_spec])
        .current_dir(repo_path)
        .output()
        .context("Failed to run git rev-parse")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("Failed to resolve commit '{}': {}", ref_spec, stderr.trim());
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn get_file_at_commit(
    repo_path: &PathBuf,
    commit: &str,
    file_path: &str,
) -> Result<Option<String>> {
    let git_path = format!("{}:{}", commit, file_path);
    let output = Command::new("git")
        .args(["show", &git_path])
        .current_dir(repo_path)
        .output()
        .context("Failed to run git show")?;

    if !output.status.success() {
        // File might not exist at this commit
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("does not exist") || stderr.contains("not found") {
            return Ok(None);
        }
        // If it's a different error, still return None but log it
        tracing::debug!("git show failed for {}: {}", git_path, stderr.trim());
        return Ok(None);
    }

    Ok(Some(String::from_utf8_lossy(&output.stdout).to_string()))
}

fn diff_all_manifests(
    repo_path: &PathBuf,
    from_commit: &str,
    to_commit: &str,
) -> Result<ManifestDiffs> {
    let mut diffs = ManifestDiffs::default();

    // Flatpak apps
    let diff = diff_flatpak_apps(repo_path, from_commit, to_commit)?;
    if !diff.is_empty() {
        diffs.flatpak_apps = Some(convert_diff_result(diff));
    }

    // Flatpak remotes
    let diff = diff_flatpak_remotes(repo_path, from_commit, to_commit)?;
    if !diff.is_empty() {
        diffs.flatpak_remotes = Some(convert_diff_result(diff));
    }

    // GNOME extensions
    let diff = diff_extensions(repo_path, from_commit, to_commit)?;
    if !diff.is_empty() {
        diffs.gnome_extensions = Some(convert_diff_result(diff));
    }

    // GSettings
    let diff = diff_gsettings(repo_path, from_commit, to_commit)?;
    if !diff.is_empty() {
        diffs.gsettings = Some(convert_diff_result(diff));
    }

    // Host shims
    let diff = diff_shims(repo_path, from_commit, to_commit)?;
    if !diff.is_empty() {
        diffs.host_shims = Some(convert_diff_result(diff));
    }

    // AppImage apps
    let diff = diff_appimages(repo_path, from_commit, to_commit)?;
    if !diff.is_empty() {
        diffs.appimage_apps = Some(convert_diff_result(diff));
    }

    // System packages (Vec<String>)
    let diff = diff_string_manifest(
        repo_path,
        from_commit,
        to_commit,
        "manifests/system-packages.json",
    )?;
    if !diff.is_empty() {
        diffs.system_packages = Some(diff);
    }

    // Toolbox packages (Vec<String>)
    let diff = diff_string_manifest(
        repo_path,
        from_commit,
        to_commit,
        "manifests/toolbox-packages.json",
    )?;
    if !diff.is_empty() {
        diffs.toolbox_packages = Some(diff);
    }

    Ok(diffs)
}

// ============================================================================
// Concrete diff functions for each manifest type
// ============================================================================

fn diff_flatpak_apps(
    repo_path: &PathBuf,
    from_commit: &str,
    to_commit: &str,
) -> Result<DiffResult<FlatpakApp>> {
    let old_content = get_file_at_commit(repo_path, from_commit, "manifests/flatpak-apps.json")?;
    let new_content = get_file_at_commit(repo_path, to_commit, "manifests/flatpak-apps.json")?;

    let old: FlatpakAppsManifest = parse_or_default(old_content)?;
    let new: FlatpakAppsManifest = parse_or_default(new_content)?;

    Ok(diff_collections(&old.apps, &new.apps))
}

fn diff_flatpak_remotes(
    repo_path: &PathBuf,
    from_commit: &str,
    to_commit: &str,
) -> Result<DiffResult<FlatpakRemote>> {
    let old_content = get_file_at_commit(repo_path, from_commit, "manifests/flatpak-remotes.json")?;
    let new_content = get_file_at_commit(repo_path, to_commit, "manifests/flatpak-remotes.json")?;

    let old: FlatpakRemotesManifest = parse_or_default(old_content)?;
    let new: FlatpakRemotesManifest = parse_or_default(new_content)?;

    Ok(diff_collections(&old.remotes, &new.remotes))
}

fn diff_extensions(
    repo_path: &PathBuf,
    from_commit: &str,
    to_commit: &str,
) -> Result<DiffResult<ExtensionItem>> {
    let old_content =
        get_file_at_commit(repo_path, from_commit, "manifests/gnome-extensions.json")?;
    let new_content = get_file_at_commit(repo_path, to_commit, "manifests/gnome-extensions.json")?;

    let old: GnomeExtensionsManifest = parse_or_default(old_content)?;
    let new: GnomeExtensionsManifest = parse_or_default(new_content)?;

    Ok(diff_collections(&old.extensions, &new.extensions))
}

fn diff_gsettings(
    repo_path: &PathBuf,
    from_commit: &str,
    to_commit: &str,
) -> Result<DiffResult<GSetting>> {
    let old_content = get_file_at_commit(repo_path, from_commit, "manifests/gsettings.json")?;
    let new_content = get_file_at_commit(repo_path, to_commit, "manifests/gsettings.json")?;

    let old: GSettingsManifest = parse_or_default(old_content)?;
    let new: GSettingsManifest = parse_or_default(new_content)?;

    Ok(diff_collections(&old.settings, &new.settings))
}

fn diff_shims(repo_path: &PathBuf, from_commit: &str, to_commit: &str) -> Result<DiffResult<Shim>> {
    let old_content = get_file_at_commit(repo_path, from_commit, "manifests/host-shims.json")?;
    let new_content = get_file_at_commit(repo_path, to_commit, "manifests/host-shims.json")?;

    let old: ShimsManifest = parse_or_default(old_content)?;
    let new: ShimsManifest = parse_or_default(new_content)?;

    Ok(diff_collections(&old.shims, &new.shims))
}

fn diff_appimages(
    repo_path: &PathBuf,
    from_commit: &str,
    to_commit: &str,
) -> Result<DiffResult<AppImageApp>> {
    let old_content = get_file_at_commit(repo_path, from_commit, "manifests/appimage-apps.json")?;
    let new_content = get_file_at_commit(repo_path, to_commit, "manifests/appimage-apps.json")?;

    let old: AppImageAppsManifest = parse_or_default(old_content)?;
    let new: AppImageAppsManifest = parse_or_default(new_content)?;

    Ok(diff_collections(&old.apps, &new.apps))
}

fn parse_or_default<T: serde::de::DeserializeOwned + Default>(
    content: Option<String>,
) -> Result<T> {
    match content {
        Some(c) => Ok(serde_json::from_str(&c)?),
        None => Ok(T::default()),
    }
}

/// Simple JSON format for string list manifests
#[derive(serde::Deserialize, Default)]
struct StringListManifest {
    #[serde(default)]
    packages: Vec<String>,
}

fn diff_string_manifest(
    repo_path: &PathBuf,
    from_commit: &str,
    to_commit: &str,
    manifest_path: &str,
) -> Result<DiffResult<String>> {
    let old_content = get_file_at_commit(repo_path, from_commit, manifest_path)?;
    let new_content = get_file_at_commit(repo_path, to_commit, manifest_path)?;

    let old_packages: Vec<String> = match old_content {
        Some(content) => {
            let manifest: StringListManifest = serde_json::from_str(&content)
                .with_context(|| format!("Failed to parse {} at {}", manifest_path, from_commit))?;
            manifest.packages
        }
        None => vec![],
    };

    let new_packages: Vec<String> = match new_content {
        Some(content) => {
            let manifest: StringListManifest = serde_json::from_str(&content)
                .with_context(|| format!("Failed to parse {} at {}", manifest_path, to_commit))?;
            manifest.packages
        }
        None => vec![],
    };

    Ok(diff_string_sets(&old_packages, &new_packages))
}

// ============================================================================
// System config diffing
// ============================================================================

/// Directories to scan for system config changes.
const SYSTEM_CONFIG_DIRS: &[&str] = &["system/", "systemd/", "skel/"];

fn diff_system_config(
    repo_path: &PathBuf,
    from_commit: &str,
    to_commit: &str,
) -> Result<SystemConfigDiffs> {
    let mut diffs = SystemConfigDiffs::default();

    // Get the list of changed files in system config directories
    let changed_files = get_changed_files(repo_path, from_commit, to_commit, SYSTEM_CONFIG_DIRS)?;

    for (status, path) in changed_files {
        match status.as_str() {
            "A" => {
                // Added file
                diffs.added.push(SystemConfigEntry { path });
            }
            "D" => {
                // Deleted file
                diffs.removed.push(SystemConfigEntry { path });
            }
            "M" => {
                // Modified file - compute semantic diff
                let old_content = get_file_at_commit(repo_path, from_commit, &path)?;
                let new_content = get_file_at_commit(repo_path, to_commit, &path)?;

                let file_type = ConfigFileType::from_path(&path);
                let semantic_diff = match (&old_content, &new_content) {
                    (Some(old), Some(new)) => {
                        Some(compute_semantic_diff(file_type, Some(old), Some(new)))
                    }
                    _ => None,
                };

                diffs.modified.push(SystemConfigModified {
                    path,
                    semantic_diff,
                    diff: None, // Deprecated field
                });
            }
            _ => {
                // Other status (R for rename, C for copy, etc.)
                // For now, treat as modified
                tracing::debug!("Unhandled git status '{}' for {}", status, path);
            }
        }
    }

    Ok(diffs)
}

/// Get list of changed files in specified directories between two commits.
fn get_changed_files(
    repo_path: &PathBuf,
    from_commit: &str,
    to_commit: &str,
    dirs: &[&str],
) -> Result<Vec<(String, String)>> {
    let mut args = vec![
        "diff".to_string(),
        "--name-status".to_string(),
        format!("{}..{}", from_commit, to_commit),
        "--".to_string(),
    ];
    args.extend(dirs.iter().map(|s| s.to_string()));

    let output = Command::new("git")
        .args(&args)
        .current_dir(repo_path)
        .output()
        .context("Failed to run git diff --name-status")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git diff failed: {}", stderr.trim());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut results = Vec::new();

    for line in stdout.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() >= 2 {
            let status = parts[0].to_string();
            let path = parts[1].to_string();
            results.push((status, path));
        }
    }

    Ok(results)
}

// ============================================================================
// Upstream changes (base image diffing)
// ============================================================================

/// Base image name for the upstream image.
const BASE_IMAGE_NAME: &str = "ghcr.io/ublue-os/bazzite";

/// Path to the digest file tracking the base image.
const BASE_IMAGE_DIGEST_FILE: &str = "upstream/bazzite-stable.digest";

/// Diff upstream changes between two commits.
fn diff_upstream_changes(
    repo_path: &PathBuf,
    from_commit: &str,
    to_commit: &str,
) -> Result<Option<UpstreamChanges>> {
    // Check if the digest file changed
    let old_digest = get_base_image_digest_at_commit(repo_path, from_commit)?;
    let new_digest = get_base_image_digest_at_commit(repo_path, to_commit)?;

    // If either digest is missing or they're the same, no change
    match (&old_digest, &new_digest) {
        (Some(old), Some(new)) if old != new => {
            tracing::info!("Base image changed: {} → {}", &old[..16], &new[..16]);

            // Try to compute package diffs
            match base_image::diff_base_image(repo_path, BASE_IMAGE_NAME, old, new) {
                Ok(base_image_change) => Ok(Some(UpstreamChanges {
                    base_image: Some(base_image_change),
                    tools: None,
                })),
                Err(e) => {
                    // Log the error but don't fail the build
                    tracing::warn!("Failed to compute base image package diff: {}", e);
                    Output::warning(format!(
                        "Could not compute package diff (requires podman on host): {}",
                        e
                    ));

                    // Still report the digest change even without package details
                    Ok(Some(UpstreamChanges {
                        base_image: Some(crate::manifest::build_info::BaseImageChange {
                            name: BASE_IMAGE_NAME.to_string(),
                            previous_digest: old.clone(),
                            current_digest: new.clone(),
                            packages: None,
                        }),
                        tools: None,
                    }))
                }
            }
        }
        _ => {
            // No change or missing digests
            Ok(None)
        }
    }
}

/// Parse the digest from a digest file (handles comment lines).
fn parse_digest_file(content: &str) -> Option<String> {
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // Should be just the digest, e.g., sha256:...
        if line.starts_with("sha256:") {
            return Some(line.to_string());
        }
    }
    None
}

/// Get the base image digest at a specific commit.
fn get_base_image_digest_at_commit(repo_path: &PathBuf, commit: &str) -> Result<Option<String>> {
    let content = get_file_at_commit(repo_path, commit, BASE_IMAGE_DIGEST_FILE)?;
    Ok(content.as_ref().and_then(|c| parse_digest_file(c)))
}

fn write_output(content: &str, output: Option<PathBuf>) -> Result<()> {
    match output {
        Some(path) => {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("Failed to create directory {}", parent.display()))?;
            }
            fs::write(&path, content)
                .with_context(|| format!("Failed to write to {}", path.display()))?;
            Output::success(format!("Wrote output to {}", path.display()));
        }
        None => {
            io::stdout()
                .write_all(content.as_bytes())
                .context("Failed to write to stdout")?;
            println!(); // Ensure trailing newline
        }
    }
    Ok(())
}

// ============================================================================
// Render implementation
// ============================================================================

fn render(input: PathBuf, output: Option<PathBuf>) -> Result<()> {
    let content = fs::read_to_string(&input)
        .with_context(|| format!("Failed to read {}", input.display()))?;

    let build_info: BuildInfo = serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse build info from {}", input.display()))?;

    let markdown = render_to_markdown(&build_info);

    write_output(&markdown, output)?;

    Ok(())
}

fn summary(input: PathBuf, max_length: usize) -> Result<()> {
    let content = fs::read_to_string(&input)
        .with_context(|| format!("Failed to read {}", input.display()))?;

    let build_info: BuildInfo = serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse build info from {}", input.display()))?;

    let summary = generate_summary(&build_info, max_length);
    println!("{}", summary);

    Ok(())
}

/// Generate a short summary string for OCI annotation.
///
/// Priority order (per RFC-0013):
/// 1. Kernel updates (always show - security critical)
/// 2. Security-relevant packages
/// 3. Flatpak changes
/// 4. Extension changes
/// 5. Config changes (lowest priority)
fn generate_summary(info: &BuildInfo, max_length: usize) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut total_changes = 0;

    // System packages
    if let Some(diff) = &info.manifests.system_packages {
        let added = diff.added.len();
        let removed = diff.removed.len();
        total_changes += added + removed;
        if added > 0 {
            parts.push(format!("➕{} pkg", added));
        }
        if removed > 0 {
            parts.push(format!("➖{} pkg", removed));
        }
    }

    // Flatpak apps
    if let Some(diff) = &info.manifests.flatpak_apps {
        let added = diff.added.len();
        let removed = diff.removed.len();
        total_changes += added + removed;
        if added > 0 {
            if added == 1 {
                parts.push(format!("➕ Flatpak: {}", diff.added[0].id));
            } else {
                parts.push(format!("➕{} flatpaks", added));
            }
        }
        if removed > 0 {
            if removed == 1 {
                parts.push(format!("➖ Flatpak: {}", diff.removed[0].id));
            } else {
                parts.push(format!("➖{} flatpaks", removed));
            }
        }
    }

    // GNOME extensions
    if let Some(diff) = &info.manifests.gnome_extensions {
        let added = diff.added.len();
        let removed = diff.removed.len();
        total_changes += added + removed;
        if added > 0 {
            parts.push(format!("➕{} extensions", added));
        }
        if removed > 0 {
            parts.push(format!("➖{} extensions", removed));
        }
    }

    // Host shims
    if let Some(diff) = &info.manifests.host_shims {
        let added = diff.added.len();
        let removed = diff.removed.len();
        total_changes += added + removed;
        if added > 0 {
            parts.push(format!("➕{} shims", added));
        }
        if removed > 0 {
            parts.push(format!("➖{} shims", removed));
        }
    }

    // AppImage apps
    if let Some(diff) = &info.manifests.appimage_apps {
        let added = diff.added.len();
        let removed = diff.removed.len();
        total_changes += added + removed;
        if added > 0 {
            parts.push(format!("➕{} AppImages", added));
        }
        if removed > 0 {
            parts.push(format!("➖{} AppImages", removed));
        }
    }

    // GSettings
    if let Some(diff) = &info.manifests.gsettings {
        let changed = diff.added.len() + diff.removed.len() + diff.changed.len();
        total_changes += changed;
        if changed > 0 {
            parts.push(format!("🔧{} settings", changed));
        }
    }

    // System config
    if let Some(config) = &info.system_config {
        let changed = config.added.len() + config.removed.len() + config.modified.len();
        total_changes += changed;
        if changed > 0 {
            parts.push(format!("📁{} config files", changed));
        }
    }

    // Upstream changes (base image)
    if let Some(upstream) = &info.upstream
        && let Some(base) = &upstream.base_image
    {
        if let Some(packages) = &base.packages {
            let updated = packages.updated.len();
            if updated > 0 {
                // Check for kernel updates specifically
                let kernel_update = packages
                    .updated
                    .iter()
                    .find(|u| u.name.starts_with("kernel"));
                if let Some(k) = kernel_update {
                    parts.insert(0, format!("🐧kernel: {} → {}", k.from, k.to));
                } else {
                    parts.insert(0, format!("🔄{} base pkg updates", updated));
                }
                total_changes += updated;
            }
        } else {
            parts.insert(0, "🔄base image updated".to_string());
            total_changes += 1;
        }
    }

    if parts.is_empty() {
        return "No changes detected".to_string();
    }

    // Build the summary, truncating if needed
    let mut summary = parts.join(" | ");

    // Add total if we truncated
    if summary.len() > max_length {
        let truncated = format!("{} changes", total_changes);
        if truncated.len() <= max_length {
            summary = truncated;
        } else {
            summary = summary[..max_length.saturating_sub(3)].to_string();
            summary.push_str("...");
        }
    }

    summary
}

fn render_to_markdown(info: &BuildInfo) -> String {
    let mut md = String::new();

    // Header
    md.push_str("# Build Info\n\n");
    md.push_str(&format!(
        "**Commit**: `{}`\n",
        &info.build.commit[..8.min(info.build.commit.len())]
    ));
    md.push_str(&format!("**Timestamp**: {}\n", info.build.timestamp));
    if let Some(prev) = &info.build.previous_commit {
        md.push_str(&format!("**Previous**: `{}`\n", &prev[..8.min(prev.len())]));
    }
    md.push('\n');

    if info.manifests.is_empty() {
        md.push_str("*No manifest changes detected.*\n");
        return md;
    }

    md.push_str("## Manifest Changes\n\n");

    // Flatpak apps
    if let Some(diff) = &info.manifests.flatpak_apps
        && !diff.is_empty()
    {
        md.push_str("### Flatpak Apps\n\n");
        render_flatpak_apps_diff(&mut md, diff);
    }

    // Flatpak remotes
    if let Some(diff) = &info.manifests.flatpak_remotes
        && !diff.is_empty()
    {
        md.push_str("### Flatpak Remotes\n\n");
        render_flatpak_remotes_diff(&mut md, diff);
    }

    // System packages
    if let Some(diff) = &info.manifests.system_packages
        && !diff.is_empty()
    {
        md.push_str("### System Packages\n\n");
        render_string_diff(&mut md, diff);
    }

    // Toolbox packages
    if let Some(diff) = &info.manifests.toolbox_packages
        && !diff.is_empty()
    {
        md.push_str("### Toolbox Packages\n\n");
        render_string_diff(&mut md, diff);
    }

    // GNOME extensions
    if let Some(diff) = &info.manifests.gnome_extensions
        && !diff.is_empty()
    {
        md.push_str("### GNOME Extensions\n\n");
        render_extensions_diff(&mut md, diff);
    }

    // GSettings
    if let Some(diff) = &info.manifests.gsettings
        && !diff.is_empty()
    {
        md.push_str("### GSettings\n\n");
        render_gsettings_diff(&mut md, diff);
    }

    // Host shims
    if let Some(diff) = &info.manifests.host_shims
        && !diff.is_empty()
    {
        md.push_str("### Host Shims\n\n");
        render_shims_diff(&mut md, diff);
    }

    // AppImage apps
    if let Some(diff) = &info.manifests.appimage_apps
        && !diff.is_empty()
    {
        md.push_str("### AppImage Apps\n\n");
        render_appimage_diff(&mut md, diff);
    }

    // System config
    if let Some(config) = &info.system_config
        && !config.is_empty()
    {
        md.push_str("## System Config Changes\n\n");
        render_system_config_diff(&mut md, config);
    }

    // Upstream changes (base image)
    if let Some(upstream) = &info.upstream {
        md.push_str("## Upstream Changes\n\n");
        render_upstream_diff(&mut md, upstream);
    }

    md
}

fn render_flatpak_apps_diff(md: &mut String, diff: &DiffResult<FlatpakAppDiff>) {
    md.push_str("| Change | App | Details |\n");
    md.push_str("|--------|-----|--------|\n");

    for app in &diff.added {
        md.push_str(&format!(
            "| ➕ Added | `{}` | Remote: {} |\n",
            app.id, app.remote
        ));
    }
    for app in &diff.removed {
        md.push_str(&format!("| ➖ Removed | `{}` | |\n", app.id));
    }
    for change in &diff.changed {
        md.push_str(&format!(
            "| 🔄 Changed | `{}` | {} → {} |\n",
            change.to.id, change.from.remote, change.to.remote
        ));
    }
    md.push('\n');
}

fn render_flatpak_remotes_diff(md: &mut String, diff: &DiffResult<FlatpakRemoteDiff>) {
    md.push_str("| Change | Remote | URL |\n");
    md.push_str("|--------|--------|-----|\n");

    for remote in &diff.added {
        md.push_str(&format!(
            "| ➕ Added | `{}` | {} |\n",
            remote.name, remote.url
        ));
    }
    for remote in &diff.removed {
        md.push_str(&format!("| ➖ Removed | `{}` | |\n", remote.name));
    }
    for change in &diff.changed {
        md.push_str(&format!(
            "| 🔄 Changed | `{}` | {} → {} |\n",
            change.to.name, change.from.url, change.to.url
        ));
    }
    md.push('\n');
}

fn render_string_diff(md: &mut String, diff: &DiffResult<String>) {
    for item in &diff.added {
        md.push_str(&format!("- ➕ `{}`\n", item));
    }
    for item in &diff.removed {
        md.push_str(&format!("- ➖ `{}`\n", item));
    }
    md.push('\n');
}

fn render_extensions_diff(md: &mut String, diff: &DiffResult<ExtensionDiff>) {
    md.push_str("| Change | Extension | State |\n");
    md.push_str("|--------|-----------|-------|\n");

    for ext in &diff.added {
        let state = if ext.enabled { "enabled" } else { "disabled" };
        md.push_str(&format!("| ➕ Added | `{}` | {} |\n", ext.id, state));
    }
    for ext in &diff.removed {
        md.push_str(&format!("| ➖ Removed | `{}` | |\n", ext.id));
    }
    for change in &diff.changed {
        let from_state = if change.from.enabled {
            "enabled"
        } else {
            "disabled"
        };
        let to_state = if change.to.enabled {
            "enabled"
        } else {
            "disabled"
        };
        md.push_str(&format!(
            "| 🔄 Changed | `{}` | {} → {} |\n",
            change.to.id, from_state, to_state
        ));
    }
    md.push('\n');
}

fn render_gsettings_diff(md: &mut String, diff: &DiffResult<GSettingDiff>) {
    md.push_str("| Change | Schema | Key | Value |\n");
    md.push_str("|--------|--------|-----|-------|\n");

    for setting in &diff.added {
        md.push_str(&format!(
            "| ➕ Added | `{}` | `{}` | `{}` |\n",
            setting.schema, setting.key, setting.value
        ));
    }
    for setting in &diff.removed {
        md.push_str(&format!(
            "| ➖ Removed | `{}` | `{}` | |\n",
            setting.schema, setting.key
        ));
    }
    for change in &diff.changed {
        md.push_str(&format!(
            "| 🔄 Changed | `{}` | `{}` | `{}` → `{}` |\n",
            change.to.schema, change.to.key, change.from.value, change.to.value
        ));
    }
    md.push('\n');
}

fn render_shims_diff(md: &mut String, diff: &DiffResult<ShimDiff>) {
    md.push_str("| Change | Shim | Host Command |\n");
    md.push_str("|--------|------|-------------|\n");

    for shim in &diff.added {
        let host = shim.host.as_deref().unwrap_or(&shim.name);
        md.push_str(&format!("| ➕ Added | `{}` | `{}` |\n", shim.name, host));
    }
    for shim in &diff.removed {
        md.push_str(&format!("| ➖ Removed | `{}` | |\n", shim.name));
    }
    for change in &diff.changed {
        let from_host = change.from.host.as_deref().unwrap_or(&change.from.name);
        let to_host = change.to.host.as_deref().unwrap_or(&change.to.name);
        md.push_str(&format!(
            "| 🔄 Changed | `{}` | `{}` → `{}` |\n",
            change.to.name, from_host, to_host
        ));
    }
    md.push('\n');
}

fn render_appimage_diff(md: &mut String, diff: &DiffResult<AppImageDiff>) {
    md.push_str("| Change | App | Repository |\n");
    md.push_str("|--------|-----|------------|\n");

    for app in &diff.added {
        md.push_str(&format!("| ➕ Added | `{}` | {} |\n", app.name, app.repo));
    }
    for app in &diff.removed {
        md.push_str(&format!("| ➖ Removed | `{}` | |\n", app.name));
    }
    for change in &diff.changed {
        md.push_str(&format!(
            "| 🔄 Changed | `{}` | {} → {} |\n",
            change.to.name, change.from.repo, change.to.repo
        ));
    }
    md.push('\n');
}

fn render_system_config_diff(md: &mut String, config: &SystemConfigDiffs) {
    use crate::manifest::parsers::SemanticDiff;

    // Added files
    if !config.added.is_empty() {
        md.push_str("### Added Files\n\n");
        for entry in &config.added {
            md.push_str(&format!("- `{}`\n", entry.path));
        }
        md.push('\n');
    }

    // Removed files
    if !config.removed.is_empty() {
        md.push_str("### Removed Files\n\n");
        for entry in &config.removed {
            md.push_str(&format!("- `{}`\n", entry.path));
        }
        md.push('\n');
    }

    // Modified files
    if !config.modified.is_empty() {
        md.push_str("### Modified Files\n\n");
        for entry in &config.modified {
            md.push_str(&format!("#### `{}`\n\n", entry.path));

            match &entry.semantic_diff {
                Some(SemanticDiff::Keyd(diff)) => {
                    render_keyd_diff(md, diff);
                }
                Some(SemanticDiff::Systemd(diff)) => {
                    render_systemd_diff(md, diff);
                }
                Some(SemanticDiff::KeyValue(diff)) => {
                    render_keyvalue_diff(md, diff);
                }
                Some(SemanticDiff::LineSummary(diff)) => {
                    render_line_summary(md, diff);
                }
                None => {
                    md.push_str("*No semantic diff available*\n\n");
                }
            }
        }
    }
}

fn render_keyd_diff(md: &mut String, diff: &crate::manifest::parsers::KeydDiff) {
    if diff.is_empty() {
        md.push_str("*No changes*\n\n");
        return;
    }

    md.push_str("| Section | Key | Change |\n");
    md.push_str("|---------|-----|--------|\n");

    for (section, bindings) in &diff.sections {
        for binding in bindings {
            let change = match (&binding.from, &binding.to) {
                (None, Some(new)) => format!("➕ Added: `{}`", new),
                (Some(old), None) => format!("➖ Removed: `{}`", old),
                (Some(old), Some(new)) => format!("`{}` → `{}`", old, new),
                (None, None) => "—".to_string(),
            };
            md.push_str(&format!(
                "| `[{}]` | `{}` | {} |\n",
                section, binding.key, change
            ));
        }
    }
    md.push('\n');
}

fn render_systemd_diff(md: &mut String, diff: &crate::manifest::parsers::SystemdDiff) {
    if diff.is_empty() {
        md.push_str("*No changes*\n\n");
        return;
    }

    md.push_str("| Section | Property | Change |\n");
    md.push_str("|---------|----------|--------|\n");

    for (section, properties) in &diff.sections {
        for prop in properties {
            let change = match (&prop.from, &prop.to) {
                (None, Some(new)) => format!("➕ Added: `{}`", new),
                (Some(old), None) => format!("➖ Removed: `{}`", old),
                (Some(old), Some(new)) => format!("`{}` → `{}`", old, new),
                (None, None) => "—".to_string(),
            };
            md.push_str(&format!(
                "| `[{}]` | `{}` | {} |\n",
                section, prop.property, change
            ));
        }
    }
    md.push('\n');
}

fn render_keyvalue_diff(md: &mut String, diff: &crate::manifest::parsers::KeyValueDiff) {
    if diff.is_empty() {
        md.push_str("*No changes*\n\n");
        return;
    }

    md.push_str("| Section | Key | Change |\n");
    md.push_str("|---------|-----|--------|\n");

    for (section, properties) in &diff.sections {
        for prop in properties {
            let change = match (&prop.from, &prop.to) {
                (None, Some(new)) => format!("➕ Added: `{}`", new),
                (Some(old), None) => format!("➖ Removed: `{}`", old),
                (Some(old), Some(new)) => format!("`{}` → `{}`", old, new),
                (None, None) => "—".to_string(),
            };
            let section_display = if section.is_empty() {
                "(root)"
            } else {
                section
            };
            md.push_str(&format!(
                "| `[{}]` | `{}` | {} |\n",
                section_display, prop.property, change
            ));
        }
    }
    md.push('\n');
}

fn render_line_summary(md: &mut String, diff: &crate::manifest::parsers::LineSummary) {
    md.push_str(&format!(
        "*{} lines added, {} lines removed*\n\n",
        diff.added, diff.removed
    ));
}

fn render_upstream_diff(md: &mut String, upstream: &UpstreamChanges) {
    // Base image changes
    if let Some(base) = &upstream.base_image {
        md.push_str("### Base Image\n\n");
        md.push_str(&format!("**Image**: `{}`\n", base.name));
        md.push_str(&format!(
            "**Digest**: `{}...` → `{}...`\n\n",
            &base.previous_digest[..20.min(base.previous_digest.len())],
            &base.current_digest[..20.min(base.current_digest.len())]
        ));

        if let Some(packages) = &base.packages {
            // Summary
            let total_changes =
                packages.added.len() + packages.removed.len() + packages.updated.len();
            if total_changes > 0 {
                md.push_str(&format!(
                    "*{} package change{}*\n\n",
                    total_changes,
                    if total_changes == 1 { "" } else { "s" }
                ));
            }

            // Updated packages (most important - security updates, kernel updates)
            if !packages.updated.is_empty() {
                md.push_str("#### Updated Packages\n\n");
                md.push_str("| Package | From | To |\n");
                md.push_str("|---------|------|----|\n");

                // Prioritize important packages (kernel, glibc, etc.)
                let mut updates: Vec<_> = packages.updated.iter().collect();
                updates.sort_by_key(|u| {
                    // Sort kernel and security-critical packages first
                    let priority = if u.name.starts_with("kernel") {
                        0
                    } else if u.name == "glibc" || u.name == "openssl" || u.name == "systemd" {
                        1
                    } else {
                        2
                    };
                    (priority, &u.name)
                });

                // Show up to 20 updates, with note if there are more
                let show_count = updates.len().min(20);
                for update in &updates[..show_count] {
                    md.push_str(&format!(
                        "| `{}` | {} | {} |\n",
                        update.name, update.from, update.to
                    ));
                }

                if updates.len() > 20 {
                    md.push_str(&format!("\n*...and {} more updates*\n", updates.len() - 20));
                }
                md.push('\n');
            }

            // Added packages
            if !packages.added.is_empty() {
                md.push_str("#### Added Packages\n\n");
                for pkg in &packages.added {
                    md.push_str(&format!("- `{}`\n", pkg));
                }
                md.push('\n');
            }

            // Removed packages
            if !packages.removed.is_empty() {
                md.push_str("#### Removed Packages\n\n");
                for pkg in &packages.removed {
                    md.push_str(&format!("- `{}`\n", pkg));
                }
                md.push('\n');
            }
        } else {
            md.push_str("*Package diff not available*\n\n");
        }
    }
}
