//! Upstream dependency management command implementation.
//!
//! Manages external resources (themes, icons, fonts, tools) with version
//! pinning and cryptographic verification.

use crate::command_runner::{CommandOptions, CommandRunner};
use crate::manifest::{
    InstallConfig, ManifestRepo, PinnedVersion, ReleaseType, Upstream, UpstreamManifest,
    UpstreamSource,
};
use crate::output::Output;
use anyhow::{Context, Result, bail};
use chrono::Utc;
use clap::{Args, Subcommand};
use owo_colors::OwoColorize;
use sha2::{Digest, Sha256};

#[derive(Debug, Args)]
pub struct UpstreamArgs {
    #[command(subcommand)]
    pub action: UpstreamAction,
}

#[derive(Debug, Subcommand)]
pub enum UpstreamAction {
    /// List all tracked upstream dependencies
    List {
        /// Output format (table, json)
        #[arg(short, long, default_value = "table")]
        format: String,
    },
    /// Check for available updates
    Check {
        /// Upstream name (checks all if omitted)
        name: Option<String>,
        /// Include prereleases
        #[arg(long)]
        include_prereleases: bool,
    },
    /// Update upstream(s) to latest version
    Update {
        /// Upstream name (updates all if omitted)
        name: Option<String>,
        /// Update all upstreams
        #[arg(long)]
        all: bool,
    },
    /// Add a new upstream dependency
    Add {
        /// Source (github:owner/repo or url:https://...)
        source: String,
        /// Name for the upstream (auto-generated if omitted)
        #[arg(long)]
        name: Option<String>,
        /// Asset pattern for GitHub releases
        #[arg(long)]
        asset: Option<String>,
    },
    /// Pin an upstream to a specific version
    Pin {
        /// Upstream name
        name: String,
        /// Version to pin (tag, commit, or "latest")
        version: String,
    },
    /// Remove an upstream dependency
    Remove {
        /// Upstream name
        name: String,
    },
    /// Verify all checksums
    Verify,
    /// Lock: regenerate all checksums
    Lock,
    /// Generate individual files for Containerfile caching
    Generate,
    /// Show detailed info about an upstream
    Info {
        /// Upstream name
        name: String,
    },
}

pub fn run(args: UpstreamArgs, runner: &dyn CommandRunner) -> Result<()> {
    match args.action {
        UpstreamAction::List { format } => handle_list(&format),
        UpstreamAction::Check {
            name,
            include_prereleases,
        } => handle_check(name, include_prereleases, runner),
        UpstreamAction::Update { name, all } => handle_update(name, all, runner),
        UpstreamAction::Add {
            source,
            name,
            asset,
        } => handle_add(source, name, asset),
        UpstreamAction::Pin { name, version } => handle_pin(name, version),
        UpstreamAction::Remove { name } => handle_remove(name),
        UpstreamAction::Verify => handle_verify(runner),
        UpstreamAction::Lock => handle_lock(runner),
        UpstreamAction::Generate => handle_generate(),
        UpstreamAction::Info { name } => handle_info(&name),
    }
}

fn handle_list(format: &str) -> Result<()> {
    let manifest = UpstreamManifest::load()?;

    if manifest.upstreams.is_empty() {
        Output::info("No upstream dependencies tracked.");
        Output::hint("Run `bkt upstream add github:owner/repo` to add one.");
        return Ok(());
    }

    if format == "json" {
        println!("{}", serde_json::to_string_pretty(&manifest.upstreams)?);
        return Ok(());
    }

    Output::header("UPSTREAM DEPENDENCIES");
    println!(
        "{:<25} {:<15} {:<15} SOURCE",
        "NAME".cyan(),
        "VERSION",
        "TYPE"
    );
    Output::separator();

    for upstream in &manifest.upstreams {
        let source_type = match &upstream.source {
            UpstreamSource::GitHub { .. } => "github",
            UpstreamSource::Url { .. } => "url",
        };
        let source_info = match &upstream.source {
            UpstreamSource::GitHub { repo, .. } => repo.clone(),
            UpstreamSource::Url { url } => {
                // Truncate long URLs
                if url.len() > 40 {
                    format!("{}...", &url[..37])
                } else {
                    url.clone()
                }
            }
        };
        println!(
            "{:<25} {:<15} {:<15} {}",
            upstream.name.yellow(),
            upstream.pinned.version,
            source_type.dimmed(),
            source_info.dimmed()
        );
    }

    Output::blank();
    Output::info(format!("Total: {} upstreams", manifest.upstreams.len()));

    Ok(())
}

fn handle_check(
    name: Option<String>,
    include_prereleases: bool,
    runner: &dyn CommandRunner,
) -> Result<()> {
    let manifest = UpstreamManifest::load()?;

    let upstreams: Vec<&Upstream> = match &name {
        Some(n) => {
            let upstream = manifest
                .find(n)
                .with_context(|| format!("Upstream '{}' not found", n))?;
            vec![upstream]
        }
        None => manifest.upstreams.iter().collect(),
    };

    if upstreams.is_empty() {
        Output::info("No upstream dependencies to check.");
        return Ok(());
    }

    Output::header("CHECKING FOR UPDATES");
    let mut updates_available = 0;

    for upstream in upstreams {
        let spinner = Output::spinner(format!("Checking {}...", upstream.name));

        match check_for_update(upstream, include_prereleases, runner) {
            Ok(Some(new_version)) => {
                spinner.finish_clear();
                println!(
                    "  {} {} → {} {}",
                    upstream.name.yellow(),
                    upstream.pinned.version.dimmed(),
                    new_version.green(),
                    "(update available)".green()
                );
                updates_available += 1;
            }
            Ok(None) => {
                spinner.finish_clear();
                println!(
                    "  {} {} {}",
                    upstream.name,
                    upstream.pinned.version.dimmed(),
                    "(up to date)".dimmed()
                );
            }
            Err(e) => {
                spinner.finish_clear();
                Output::warning(format!("{}: failed to check - {}", upstream.name, e));
            }
        }
    }

    Output::blank();
    if updates_available > 0 {
        Output::info(format!(
            "{} update{} available. Run `bkt upstream update` to update.",
            updates_available,
            if updates_available == 1 { "" } else { "s" }
        ));
    } else {
        Output::success("All upstreams are up to date.");
    }

    Ok(())
}

fn check_for_update(
    upstream: &Upstream,
    include_prereleases: bool,
    runner: &dyn CommandRunner,
) -> Result<Option<String>> {
    match &upstream.source {
        UpstreamSource::GitHub { repo, .. } => {
            check_github_update(repo, &upstream.pinned.version, include_prereleases, runner)
        }
        UpstreamSource::Url { .. } => {
            // URL sources can't be automatically checked
            Ok(None)
        }
    }
}

fn check_github_update(
    repo: &str,
    current_version: &str,
    include_prereleases: bool,
    runner: &dyn CommandRunner,
) -> Result<Option<String>> {
    // Use gh CLI to query latest release
    let output = runner
        .run_output(
            "gh",
            &[
                "api",
                &format!("repos/{}/releases/latest", repo),
                "--jq",
                ".tag_name",
            ],
            &CommandOptions::default(),
        )
        .context("Failed to run gh CLI")?;

    if !output.status.success() {
        // Maybe no releases, try tags
        let output = runner
            .run_output(
                "gh",
                &["api", &format!("repos/{}/tags", repo), "--jq", ".[0].name"],
                &CommandOptions::default(),
            )
            .context("Failed to query tags")?;

        if !output.status.success() {
            bail!("Failed to query GitHub API");
        }

        let latest = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if latest.is_empty() {
            return Ok(None);
        }
        if latest != current_version {
            return Ok(Some(latest));
        }
        return Ok(None);
    }

    let latest = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if latest.is_empty() {
        return Ok(None);
    }

    // If current version doesn't match latest, check if it's an update
    if latest != current_version {
        // Skip prereleases unless requested
        if !include_prereleases && is_prerelease(&latest) {
            return Ok(None);
        }
        return Ok(Some(latest));
    }

    Ok(None)
}

fn is_prerelease(version: &str) -> bool {
    version.contains("-alpha")
        || version.contains("-beta")
        || version.contains("-rc")
        || version.contains("-pre")
}

fn handle_update(name: Option<String>, all: bool, runner: &dyn CommandRunner) -> Result<()> {
    if name.is_none() && !all {
        Output::error("Specify an upstream name or use --all to update all.");
        return Ok(());
    }

    let mut manifest = UpstreamManifest::load()?;

    let names: Vec<String> = match &name {
        Some(n) => vec![n.clone()],
        None => manifest.upstreams.iter().map(|u| u.name.clone()).collect(),
    };

    for upstream_name in names {
        let spinner = Output::spinner(format!("Updating {}...", upstream_name));

        match update_upstream(&mut manifest, &upstream_name, runner) {
            Ok(Some(new_version)) => {
                spinner.finish_clear();
                Output::success(format!("{} updated to {}", upstream_name, new_version));
            }
            Ok(None) => {
                spinner.finish_clear();
                Output::info(format!("{} is already up to date", upstream_name));
            }
            Err(e) => {
                spinner.finish_clear();
                Output::error(format!("Failed to update {}: {}", upstream_name, e));
            }
        }
    }

    manifest.save()?;
    Output::success("Manifest saved.");

    Ok(())
}

fn update_upstream(
    manifest: &mut UpstreamManifest,
    name: &str,
    runner: &dyn CommandRunner,
) -> Result<Option<String>> {
    let upstream = manifest
        .find(name)
        .with_context(|| format!("Upstream '{}' not found", name))?
        .clone();

    let new_version = match &upstream.source {
        UpstreamSource::GitHub { repo, .. } => {
            check_github_update(repo, &upstream.pinned.version, false, runner)?
        }
        UpstreamSource::Url { .. } => None,
    };

    if let Some(ref version) = new_version {
        // Update the upstream
        if let Some(u) = manifest.find_mut(name) {
            u.pinned.version = version.clone();
            u.pinned.pinned_at = Utc::now();
            // SHA256 needs to be recomputed
            u.pinned.sha256 =
                "0000000000000000000000000000000000000000000000000000000000000000".to_string();
        }
    }

    Ok(new_version)
}

fn handle_add(source: String, name: Option<String>, asset: Option<String>) -> Result<()> {
    let mut manifest = UpstreamManifest::load()?;

    let (upstream_source, derived_name) = parse_source(&source, asset)?;
    let final_name = name.unwrap_or(derived_name);

    if manifest.contains(&final_name) {
        bail!(
            "Upstream '{}' already exists. Use `bkt upstream pin` to update.",
            final_name
        );
    }

    let upstream = Upstream {
        name: final_name.clone(),
        description: None,
        source: upstream_source,
        pinned: PinnedVersion {
            version: "latest".to_string(),
            commit: None,
            url: None,
            sha256: "0000000000000000000000000000000000000000000000000000000000000000".to_string(),
            gpg_verified: false,
            pinned_at: Utc::now(),
        },
        install: None,
    };

    manifest.upsert(upstream);
    manifest.save()?;

    Output::success(format!("Added upstream: {}", final_name));
    Output::hint("Run `bkt upstream lock` to compute checksums.");

    Ok(())
}

fn parse_source(source: &str, asset: Option<String>) -> Result<(UpstreamSource, String)> {
    if let Some(repo) = source.strip_prefix("github:") {
        let name = repo
            .split('/')
            .next_back()
            .unwrap_or(repo)
            .to_string()
            .to_lowercase();
        Ok((
            UpstreamSource::GitHub {
                repo: repo.to_string(),
                asset_pattern: asset,
                release_type: ReleaseType::Release,
            },
            name,
        ))
    } else if let Some(url) = source.strip_prefix("url:") {
        let name = url
            .split('/')
            .next_back()
            .unwrap_or("unknown")
            .split('.')
            .next()
            .unwrap_or("unknown")
            .to_string()
            .to_lowercase();
        Ok((
            UpstreamSource::Url {
                url: url.to_string(),
            },
            name,
        ))
    } else {
        bail!("Invalid source format. Use 'github:owner/repo' or 'url:https://...'");
    }
}

fn handle_pin(name: String, version: String) -> Result<()> {
    let mut manifest = UpstreamManifest::load()?;

    let upstream = manifest
        .find_mut(&name)
        .with_context(|| format!("Upstream '{}' not found", name))?;

    let old_version = upstream.pinned.version.clone();
    upstream.pinned.version = version.clone();
    upstream.pinned.pinned_at = Utc::now();
    // SHA256 needs to be recomputed
    upstream.pinned.sha256 =
        "0000000000000000000000000000000000000000000000000000000000000000".to_string();

    manifest.save()?;

    Output::success(format!("{}: {} → {}", name, old_version, version));
    Output::hint("Run `bkt upstream lock` to compute the new checksum.");

    Ok(())
}

fn handle_remove(name: String) -> Result<()> {
    let mut manifest = UpstreamManifest::load()?;

    if !manifest.remove(&name) {
        bail!("Upstream '{}' not found", name);
    }

    manifest.save()?;
    Output::success(format!("Removed upstream: {}", name));

    Ok(())
}

fn handle_verify(runner: &dyn CommandRunner) -> Result<()> {
    let manifest = UpstreamManifest::load()?;

    if manifest.upstreams.is_empty() {
        Output::info("No upstream dependencies to verify.");
        return Ok(());
    }

    Output::header("VERIFYING CHECKSUMS");
    let mut verified = 0;
    let mut failed = 0;
    let mut skipped = 0;

    for upstream in &manifest.upstreams {
        // Check if we have a placeholder checksum
        if upstream.pinned.sha256
            == "0000000000000000000000000000000000000000000000000000000000000000"
        {
            println!(
                "  {} {}",
                upstream.name.yellow(),
                "(no checksum - run 'bkt upstream lock')".yellow()
            );
            skipped += 1;
            continue;
        }

        let spinner = Output::spinner(format!("Verifying {}...", upstream.name));

        match verify_upstream(upstream, runner) {
            Ok(true) => {
                spinner.finish_clear();
                println!(
                    "  {} {} {}",
                    "✓".green(),
                    upstream.name,
                    upstream.pinned.version.dimmed()
                );
                verified += 1;
            }
            Ok(false) => {
                spinner.finish_clear();
                println!(
                    "  {} {} {}",
                    "✗".red(),
                    upstream.name.red(),
                    "checksum mismatch!".red()
                );
                failed += 1;
            }
            Err(e) => {
                spinner.finish_clear();
                Output::warning(format!("{}: verification failed - {}", upstream.name, e));
                failed += 1;
            }
        }
    }

    Output::blank();
    Output::info(format!(
        "Verified: {}, Failed: {}, Skipped: {}",
        verified, failed, skipped
    ));

    if failed > 0 {
        bail!("Verification failed for {} upstream(s)", failed);
    }

    // Write verified hash if all passed
    if skipped == 0 {
        manifest.write_verified_hash()?;
        Output::success("Wrote verified manifest hash.");
    }

    Ok(())
}

fn verify_upstream(upstream: &Upstream, runner: &dyn CommandRunner) -> Result<bool> {
    // Download and compute checksum
    let url = get_download_url(upstream, runner)?;
    let computed_hash = download_and_hash(&url, runner)?;
    Ok(computed_hash == upstream.pinned.sha256)
}

fn get_download_url(upstream: &Upstream, runner: &dyn CommandRunner) -> Result<String> {
    match &upstream.source {
        UpstreamSource::GitHub {
            repo,
            asset_pattern,
            release_type,
        } => {
            let version = &upstream.pinned.version;

            match release_type {
                ReleaseType::Release => {
                    if let Some(pattern) = asset_pattern {
                        // Get specific asset from release
                        let output = runner.run_output(
                            "gh",
                            &[
                                "release",
                                "view",
                                version,
                                "--repo",
                                repo,
                                "--json",
                                "assets",
                                "--jq",
                                &format!(
                                    ".assets[] | select(.name | test(\"{}\")) | .url",
                                    pattern.replace('*', ".*")
                                ),
                            ],
                            &CommandOptions::default(),
                        )?;

                        if output.status.success() {
                            let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
                            if !url.is_empty() {
                                return Ok(url);
                            }
                        }
                    }
                    // Fallback to source tarball
                    Ok(format!(
                        "https://github.com/{}/archive/refs/tags/{}.tar.gz",
                        repo, version
                    ))
                }
                ReleaseType::Tag | ReleaseType::Branch => Ok(format!(
                    "https://github.com/{}/archive/refs/tags/{}.tar.gz",
                    repo, version
                )),
            }
        }
        UpstreamSource::Url { url } => {
            // Substitute version placeholder
            Ok(url.replace("{version}", &upstream.pinned.version))
        }
    }
}

fn download_and_hash(url: &str, runner: &dyn CommandRunner) -> Result<String> {
    let output = runner
        .run_output("curl", &["-fsSL", url], &CommandOptions::default())
        .context("Failed to download")?;

    if !output.status.success() {
        bail!(
            "Download failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let hash = Sha256::digest(&output.stdout);
    Ok(hex::encode(hash))
}

fn handle_lock(runner: &dyn CommandRunner) -> Result<()> {
    let mut manifest = UpstreamManifest::load()?;

    if manifest.upstreams.is_empty() {
        Output::info("No upstream dependencies to lock.");
        return Ok(());
    }

    Output::header("LOCKING CHECKSUMS");
    let mut locked = 0;
    let mut failed = 0;

    let names: Vec<String> = manifest.upstreams.iter().map(|u| u.name.clone()).collect();

    for name in names {
        let upstream = manifest.find(&name).unwrap().clone();
        let spinner = Output::spinner(format!("Downloading {}...", upstream.name));

        match lock_upstream(&upstream, runner) {
            Ok((sha256, url)) => {
                spinner.finish_clear();
                if let Some(u) = manifest.find_mut(&name) {
                    u.pinned.sha256 = sha256.clone();
                    u.pinned.url = Some(url);
                }
                println!(
                    "  {} {} → {}",
                    "✓".green(),
                    upstream.name,
                    format!("{}...", &sha256[..16]).dimmed()
                );
                locked += 1;
            }
            Err(e) => {
                spinner.finish_clear();
                Output::error(format!("{}: failed - {}", upstream.name, e));
                failed += 1;
            }
        }
    }

    manifest.save()?;

    Output::blank();
    Output::info(format!("Locked: {}, Failed: {}", locked, failed));

    if failed == 0 {
        manifest.write_verified_hash()?;
        Output::success("Wrote verified manifest hash.");
    }

    Ok(())
}

fn lock_upstream(upstream: &Upstream, runner: &dyn CommandRunner) -> Result<(String, String)> {
    let url = get_download_url(upstream, runner)?;
    let hash = download_and_hash(&url, runner)?;
    Ok((hash, url))
}

fn handle_generate() -> Result<()> {
    let manifest = UpstreamManifest::load()?;
    manifest.generate_files()?;

    Output::success(format!(
        "Generated files for {} upstreams.",
        manifest.upstreams.len()
    ));

    Ok(())
}

fn handle_info(name: &str) -> Result<()> {
    let manifest = UpstreamManifest::load()?;
    let upstream = manifest
        .find(name)
        .with_context(|| format!("Upstream '{}' not found", name))?;

    Output::header(format!("UPSTREAM: {}", upstream.name.to_uppercase()));

    if let Some(desc) = &upstream.description {
        println!("  {} {}", "Description:".dimmed(), desc);
    }

    match &upstream.source {
        UpstreamSource::GitHub {
            repo,
            asset_pattern,
            release_type,
        } => {
            println!("  {} github", "Type:".dimmed());
            println!("  {} {}", "Repository:".dimmed(), repo);
            if let Some(pattern) = asset_pattern {
                println!("  {} {}", "Asset Pattern:".dimmed(), pattern);
            }
            println!("  {} {:?}", "Release Type:".dimmed(), release_type);
        }
        UpstreamSource::Url { url } => {
            println!("  {} url", "Type:".dimmed());
            println!("  {} {}", "URL:".dimmed(), url);
        }
    }

    Output::blank();
    println!("  {}", "Pinned Version:".cyan());
    println!("  {} {}", "Version:".dimmed(), upstream.pinned.version);
    if let Some(commit) = &upstream.pinned.commit {
        println!("  {} {}", "Commit:".dimmed(), commit);
    }
    if let Some(url) = &upstream.pinned.url {
        println!("  {} {}", "URL:".dimmed(), url);
    }
    println!("  {} {}", "SHA256:".dimmed(), upstream.pinned.sha256);
    println!(
        "  {} {}",
        "GPG Verified:".dimmed(),
        if upstream.pinned.gpg_verified {
            "yes"
        } else {
            "no"
        }
    );
    println!("  {} {}", "Pinned At:".dimmed(), upstream.pinned.pinned_at);

    if let Some(install) = &upstream.install {
        Output::blank();
        println!("  {}", "Installation:".cyan());
        match install {
            InstallConfig::Archive {
                extract_to,
                strip_components,
            } => {
                println!("  {} archive", "Type:".dimmed());
                println!("  {} {}", "Extract To:".dimmed(), extract_to);
                println!("  {} {}", "Strip Components:".dimmed(), strip_components);
            }
            InstallConfig::Binary { install_path } => {
                println!("  {} binary", "Type:".dimmed());
                println!("  {} {}", "Install Path:".dimmed(), install_path);
            }
            InstallConfig::Script { command, .. } => {
                println!("  {} script", "Type:".dimmed());
                println!("  {} {}", "Command:".dimmed(), command);
            }
        }
    }

    Ok(())
}
