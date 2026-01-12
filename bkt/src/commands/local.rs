//! Local command implementation.
//!
//! The `bkt local` command manages the ephemeral manifest - changes made with
//! `--local` that haven't been promoted to a PR yet.
//!
//! # Commands
//!
//! - `bkt local list` - Show all tracked local changes
//! - `bkt local commit` - Promote local changes to a PR
//! - `bkt local clear` - Discard all tracked changes (keeps installed items)
//!
//! # Example Workflow
//!
//! ```bash
//! # Make some local-only changes
//! bkt flatpak add --local org.gnome.Calculator
//! bkt dnf install --local htop
//!
//! # View what's tracked
//! bkt local list
//! # Added (local-only):
//! #   flatpak: org.gnome.Calculator
//! #   dnf: htop
//!
//! # Promote all to a PR
//! bkt local commit
//!
//! # Or discard tracking (items remain installed)
//! bkt local clear
//! ```

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand, ValueEnum};
use owo_colors::OwoColorize;

use crate::manifest::ephemeral::{ChangeAction, ChangeDomain, EphemeralChange, EphemeralManifest};
use crate::manifest::{
    FlatpakApp, FlatpakAppsManifest, FlatpakScope, GSetting, GSettingsManifest,
    GnomeExtensionsManifest, Shim, ShimsManifest, SystemPackagesManifest,
};
use crate::output::Output;
use crate::pipeline::ExecutionPlan;
use crate::pr::{ensure_preflight, ensure_repo};
use crate::repo::RepoConfig;
use std::collections::HashMap;
use std::process::Command;

#[derive(Debug, Args)]
pub struct LocalArgs {
    #[command(subcommand)]
    pub action: LocalAction,
}

#[derive(Debug, Clone, Copy, ValueEnum, Default)]
pub enum ListFormat {
    /// Human-readable table output
    #[default]
    Table,
    /// JSON output for scripting
    Json,
}

#[derive(Debug, Subcommand)]
pub enum LocalAction {
    /// List all tracked local-only changes
    ///
    /// Shows changes made with --local that haven't been promoted to a PR.
    /// These changes are lost on reboot or image switch.
    List {
        /// Output format
        #[arg(short, long, value_enum, default_value = "table")]
        format: ListFormat,

        /// Filter by domain (flatpak, dnf, extension, gsetting, shim)
        #[arg(short, long)]
        domain: Option<String>,
    },

    /// Promote local changes to a PR
    ///
    /// Creates a PR containing all tracked local-only changes.
    /// After successful PR creation, the ephemeral manifest is cleared.
    Commit {
        /// PR title (auto-generated if not specified)
        #[arg(short, long)]
        message: Option<String>,

        /// Only commit changes for a specific domain
        #[arg(short, long)]
        domain: Option<String>,

        /// Interactive selection of which changes to include
        #[arg(short, long)]
        select: bool,
    },

    /// Clear all tracked local changes
    ///
    /// Removes all entries from the ephemeral manifest.
    /// The installed items remain - only the tracking is cleared.
    Clear {
        /// Skip confirmation prompt
        #[arg(short, long)]
        force: bool,
    },

    /// Show the path to the ephemeral manifest file
    Path,
}

/// Parse a domain filter string into a ChangeDomain.
fn parse_domain_filter(domain: &str) -> Result<ChangeDomain> {
    match domain.to_lowercase().as_str() {
        "flatpak" | "fp" => Ok(ChangeDomain::Flatpak),
        "extension" | "ext" => Ok(ChangeDomain::Extension),
        "gsetting" | "gs" => Ok(ChangeDomain::Gsetting),
        "shim" => Ok(ChangeDomain::Shim),
        "dnf" | "rpm" => Ok(ChangeDomain::Dnf),
        _ => bail!(
            "Unknown domain: '{}'. Valid domains: flatpak, extension, gsetting, shim, dnf",
            domain
        ),
    }
}

/// List all tracked local changes.
fn list_changes(format: ListFormat, domain_filter: Option<String>) -> Result<()> {
    let manifest = EphemeralManifest::load_validated()?;

    // Apply domain filter if specified
    let domain_filter = domain_filter.map(|d| parse_domain_filter(&d)).transpose()?;

    let filtered_changes: Vec<&EphemeralChange> = manifest
        .changes
        .iter()
        .filter(|c| domain_filter.is_none_or(|d| c.domain == d))
        .collect();

    match format {
        ListFormat::Json => {
            let output = serde_json::to_string_pretty(&filtered_changes)?;
            println!("{}", output);
        }
        ListFormat::Table => {
            if filtered_changes.is_empty() {
                if domain_filter.is_some() {
                    Output::info("No local changes tracked for this domain.");
                } else {
                    Output::info("No local changes tracked.");
                    println!();
                    println!("  Use {} to make local-only changes.", "--local".cyan());
                    println!(
                        "  Example: {}",
                        "bkt flatpak add --local org.gnome.Calculator".dimmed()
                    );
                }
                return Ok(());
            }

            println!(
                "{} {} tracked:",
                "Local changes".bold(),
                format!("({})", filtered_changes.len()).dimmed()
            );
            println!();

            // Group by domain for cleaner output
            let by_domain = manifest.changes_by_domain();
            let mut domains: Vec<_> = by_domain.keys().collect();
            domains.sort_by_key(|d| d.to_string());

            for domain in domains {
                if domain_filter.is_some_and(|d| *domain != d) {
                    continue;
                }

                let changes = by_domain.get(domain).unwrap();
                println!("  {}:", domain.to_string().cyan().bold());
                for change in changes {
                    let action_str = match change.action {
                        ChangeAction::Add => "+".green().to_string(),
                        ChangeAction::Remove => "-".red().to_string(),
                        ChangeAction::Update => "~".yellow().to_string(),
                    };
                    println!("    {} {}", action_str, change.identifier);
                }
                println!();
            }

            println!(
                "{}",
                "These changes will be lost on reboot or image switch.".dimmed()
            );
            println!(
                "{}",
                format!("Run '{}' to promote to a PR.", "bkt local commit").dimmed()
            );
        }
    }

    Ok(())
}

/// Promote local changes to a PR.
fn commit_changes(
    plan: &ExecutionPlan,
    message: Option<String>,
    domain_filter: Option<String>,
    _select: bool,
) -> Result<()> {
    let manifest = EphemeralManifest::load_validated()?;

    if manifest.is_empty() {
        Output::info("No local changes to commit.");
        return Ok(());
    }

    // Apply domain filter if specified
    let domain_filter = domain_filter.map(|d| parse_domain_filter(&d)).transpose()?;

    let changes_to_commit: Vec<&EphemeralChange> = manifest
        .changes
        .iter()
        .filter(|c| domain_filter.is_none_or(|d| c.domain == d))
        .collect();

    if changes_to_commit.is_empty() {
        Output::info("No local changes match the filter.");
        return Ok(());
    }

    // Generate a default commit message if not provided
    let message = message.unwrap_or_else(|| {
        if changes_to_commit.len() == 1 {
            let change = changes_to_commit[0];
            format!("{}: {} {}", change.domain, change.action, change.identifier)
        } else {
            let domains: std::collections::HashSet<_> =
                changes_to_commit.iter().map(|c| c.domain).collect();
            if domains.len() == 1 {
                let domain = domains.iter().next().unwrap();
                format!("{}: {} local changes", domain, changes_to_commit.len())
            } else {
                format!("Promote {} local changes", changes_to_commit.len())
            }
        }
    });

    if plan.dry_run {
        Output::dry_run(format!(
            "Would commit {} local changes with message: {}",
            changes_to_commit.len(),
            message
        ));
        for change in &changes_to_commit {
            Output::dry_run(format!("  {} {}", change.action, change.identifier));
        }
        return Ok(());
    }

    // Run the actual commit workflow
    run_commit_workflow(
        &changes_to_commit,
        &message,
        plan.skip_preflight,
        domain_filter,
    )?;

    // Clear the committed changes
    if domain_filter.is_some() {
        // Only clear the filtered domain's changes
        let mut updated_manifest = EphemeralManifest::load_validated()?;
        for change in &changes_to_commit {
            updated_manifest.remove(change.domain, &change.identifier);
        }
        if updated_manifest.is_empty() {
            EphemeralManifest::delete_file()?;
        } else {
            updated_manifest.save()?;
        }
    } else {
        EphemeralManifest::delete_file()?;
    }

    Output::success("PR created successfully! Local changes cleared.");
    Ok(())
}

/// Information about a manifest change.
struct ManifestChange {
    content: String,
}

/// Apply ephemeral changes to system manifests and create a PR.
fn run_commit_workflow(
    changes: &[&EphemeralChange],
    message: &str,
    skip_preflight: bool,
    _domain_filter: Option<ChangeDomain>,
) -> Result<()> {
    // Run preflight checks
    ensure_preflight(skip_preflight)?;

    // Get the repo path
    let repo_path = ensure_repo()?;
    let manifests_dir = repo_path.join("manifests");

    // Group changes by domain and apply them to manifests
    let mut manifest_changes: HashMap<String, ManifestChange> = HashMap::new();

    // Process each domain
    let by_domain = group_changes_by_domain(changes);

    for (domain, domain_changes) in &by_domain {
        match domain {
            ChangeDomain::Flatpak => {
                let change = apply_flatpak_changes(domain_changes, &manifests_dir)?;
                manifest_changes.insert("flatpak-apps.json".to_string(), change);
            }
            ChangeDomain::Extension => {
                let change = apply_extension_changes(domain_changes, &manifests_dir)?;
                manifest_changes.insert("gnome-extensions.json".to_string(), change);
            }
            ChangeDomain::Gsetting => {
                let change = apply_gsetting_changes(domain_changes, &manifests_dir)?;
                manifest_changes.insert("gsettings.json".to_string(), change);
            }
            ChangeDomain::Shim => {
                let change = apply_shim_changes(domain_changes, &manifests_dir)?;
                manifest_changes.insert("host-shims.json".to_string(), change);
            }
            ChangeDomain::Dnf => {
                let change = apply_dnf_changes(domain_changes, &manifests_dir)?;
                manifest_changes.insert("system-packages.json".to_string(), change);
            }
        }
    }

    if manifest_changes.is_empty() {
        Output::info("No manifest changes to commit.");
        return Ok(());
    }

    // Create branch and commit
    create_batch_pr(&repo_path, &manifest_changes, message)?;

    Ok(())
}

/// Group changes by domain.
fn group_changes_by_domain<'a>(
    changes: &'a [&'a EphemeralChange],
) -> HashMap<ChangeDomain, Vec<&'a EphemeralChange>> {
    let mut grouped: HashMap<ChangeDomain, Vec<&'a EphemeralChange>> = HashMap::new();
    for change in changes {
        grouped.entry(change.domain).or_default().push(*change);
    }
    grouped
}

/// Apply flatpak changes to the manifest.
fn apply_flatpak_changes(
    changes: &[&EphemeralChange],
    manifests_dir: &std::path::Path,
) -> Result<ManifestChange> {
    let manifest_path = manifests_dir.join("flatpak-apps.json");
    let mut manifest = FlatpakAppsManifest::load(&manifest_path)?;

    for change in changes {
        match change.action {
            ChangeAction::Add => {
                // Get remote and scope from metadata, or use defaults
                let remote = change
                    .metadata
                    .get("remote")
                    .cloned()
                    .unwrap_or_else(|| "flathub".to_string());
                let scope: FlatpakScope = change
                    .metadata
                    .get("scope")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or_default();

                let app = FlatpakApp {
                    id: change.identifier.clone(),
                    remote,
                    scope,                    branch: None,
                    commit: None,
                    overrides: None,                };
                manifest.upsert(app);
            }
            ChangeAction::Remove => {
                manifest.remove(&change.identifier);
            }
            ChangeAction::Update => {
                // Update is treated as upsert
                let remote = change
                    .metadata
                    .get("remote")
                    .cloned()
                    .unwrap_or_else(|| "flathub".to_string());
                let scope: FlatpakScope = change
                    .metadata
                    .get("scope")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or_default();

                let app = FlatpakApp {
                    id: change.identifier.clone(),
                    remote,
                    scope,                    branch: None,
                    commit: None,
                    overrides: None,                };
                manifest.upsert(app);
            }
        }
    }

    let content = serde_json::to_string_pretty(&manifest)?;
    Ok(ManifestChange { content })
}

/// Apply extension changes to the manifest.
fn apply_extension_changes(
    changes: &[&EphemeralChange],
    manifests_dir: &std::path::Path,
) -> Result<ManifestChange> {
    let manifest_path = manifests_dir.join("gnome-extensions.json");
    let mut manifest = GnomeExtensionsManifest::load(&manifest_path)?;

    for change in changes {
        match change.action {
            ChangeAction::Add | ChangeAction::Update => {
                manifest.add(change.identifier.clone());
            }
            ChangeAction::Remove => {
                manifest.remove(&change.identifier);
            }
        }
    }

    let content = serde_json::to_string_pretty(&manifest)?;
    Ok(ManifestChange { content })
}

/// Apply gsetting changes to the manifest.
fn apply_gsetting_changes(
    changes: &[&EphemeralChange],
    manifests_dir: &std::path::Path,
) -> Result<ManifestChange> {
    let manifest_path = manifests_dir.join("gsettings.json");
    let mut manifest = GSettingsManifest::load(&manifest_path)?;

    for change in changes {
        // Parse schema.key from identifier
        let parts: Vec<&str> = change.identifier.rsplitn(2, '.').collect();
        if parts.len() != 2 {
            bail!(
                "Invalid gsetting identifier: {} (expected schema.key)",
                change.identifier
            );
        }
        let key = parts[0];
        let schema = parts[1];

        match change.action {
            ChangeAction::Add | ChangeAction::Update => {
                let value = change
                    .metadata
                    .get("value")
                    .cloned()
                    .unwrap_or_else(|| "''".to_string());

                let setting = GSetting {
                    schema: schema.to_string(),
                    key: key.to_string(),
                    value,
                    comment: None,
                };
                manifest.upsert(setting);
            }
            ChangeAction::Remove => {
                manifest.remove(schema, key);
            }
        }
    }

    let content = serde_json::to_string_pretty(&manifest)?;
    Ok(ManifestChange { content })
}

/// Apply shim changes to the manifest.
fn apply_shim_changes(
    changes: &[&EphemeralChange],
    manifests_dir: &std::path::Path,
) -> Result<ManifestChange> {
    let manifest_path = manifests_dir.join("host-shims.json");
    let mut manifest = ShimsManifest::load(&manifest_path)?;

    for change in changes {
        match change.action {
            ChangeAction::Add | ChangeAction::Update => {
                let host = change.metadata.get("host").cloned();
                let shim = Shim {
                    name: change.identifier.clone(),
                    host,
                };
                manifest.upsert(shim);
            }
            ChangeAction::Remove => {
                manifest.remove(&change.identifier);
            }
        }
    }

    let content = serde_json::to_string_pretty(&manifest)?;
    Ok(ManifestChange { content })
}

/// Apply dnf/package changes to the manifest.
fn apply_dnf_changes(
    changes: &[&EphemeralChange],
    manifests_dir: &std::path::Path,
) -> Result<ManifestChange> {
    let manifest_path = manifests_dir.join("system-packages.json");
    let mut manifest = SystemPackagesManifest::load(&manifest_path)?;

    for change in changes {
        match change.action {
            ChangeAction::Add | ChangeAction::Update => {
                manifest.add_package(change.identifier.clone());
            }
            ChangeAction::Remove => {
                manifest.remove_package(&change.identifier);
            }
        }
    }

    let content = serde_json::to_string_pretty(&manifest)?;
    Ok(ManifestChange { content })
}

/// Create a batch PR with multiple manifest changes.
fn create_batch_pr(
    repo_path: &std::path::Path,
    changes: &HashMap<String, ManifestChange>,
    message: &str,
) -> Result<()> {
    // Generate branch name
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_else(|_| std::process::id() as u64);
    let branch_name = format!("bkt/local-commit-{}", timestamp);

    // Create branch
    Output::info(format!("Creating branch: {}", branch_name));
    let status = Command::new("git")
        .args(["checkout", "-b", &branch_name])
        .current_dir(repo_path)
        .status()
        .context("Failed to create branch")?;
    if !status.success() {
        bail!("Failed to create branch {}", branch_name);
    }

    // Write each manifest file
    let manifests_dir = repo_path.join("manifests");
    for (filename, manifest_change) in changes {
        let file_path = manifests_dir.join(filename);
        std::fs::write(&file_path, &manifest_change.content)
            .with_context(|| format!("Failed to write {}", file_path.display()))?;

        // Stage the file
        let status = Command::new("git")
            .args(["add", &format!("manifests/{}", filename)])
            .current_dir(repo_path)
            .status()
            .context("Failed to stage manifest")?;
        if !status.success() {
            bail!("Failed to stage {}", filename);
        }
    }

    // Create commit
    let commit_message = format!("feat(manifests): {}", message);
    let status = Command::new("git")
        .args(["commit", "-m", &commit_message])
        .current_dir(repo_path)
        .status()
        .context("Failed to commit")?;
    if !status.success() {
        bail!("Failed to commit changes");
    }

    // Push branch
    Output::info("Pushing branch...");
    let status = Command::new("git")
        .args(["push", "-u", "origin", &branch_name])
        .current_dir(repo_path)
        .status()
        .context("Failed to push branch")?;
    if !status.success() {
        bail!("Failed to push branch");
    }

    // Create PR
    Output::info("Creating pull request...");
    let pr_body = generate_pr_body(changes);
    let status = Command::new("gh")
        .args(["pr", "create", "--title", message, "--body", &pr_body])
        .current_dir(repo_path)
        .status()
        .context("Failed to create PR")?;
    if !status.success() {
        bail!("Failed to create PR");
    }

    // Return to default branch
    let config = RepoConfig::load()?;
    if let Err(e) = Command::new("git")
        .args(["checkout", &config.default_branch])
        .current_dir(repo_path)
        .status()
    {
        eprintln!(
            "Warning: failed to switch back to '{}' branch: {}",
            config.default_branch, e
        );
    }

    Ok(())
}

/// Generate PR body describing all the changes.
fn generate_pr_body(changes: &HashMap<String, ManifestChange>) -> String {
    let mut body =
        String::from("This PR was automatically created by `bkt local commit`.\n\n## Changes\n\n");

    for filename in changes.keys() {
        body.push_str(&format!("- Updated `manifests/{}`\n", filename));
    }

    body.push_str("\n---\n*Created by bkt CLI*");
    body
}

/// Clear all tracked local changes.
fn clear_changes(force: bool, dry_run: bool) -> Result<()> {
    let manifest = EphemeralManifest::load_validated()?;

    if manifest.is_empty() {
        Output::info("No local changes to clear.");
        return Ok(());
    }

    if dry_run {
        Output::dry_run(format!(
            "Would clear {} tracked local changes",
            manifest.len()
        ));
        return Ok(());
    }

    if !force {
        // Show what would be cleared
        println!(
            "{} {} changes will be cleared:",
            "Warning:".yellow().bold(),
            manifest.len()
        );
        for change in &manifest.changes {
            println!("  {} {}", change.domain, change.identifier);
        }
        println!();
        println!(
            "{}",
            "The installed items will remain - only tracking is cleared.".dimmed()
        );
        println!();

        // Prompt for confirmation using cliclack
        let confirmed = cliclack::confirm("Clear all tracked changes?")
            .initial_value(false)
            .interact()
            .context("Failed to read confirmation")?;

        if !confirmed {
            Output::info("Cancelled.");
            return Ok(());
        }
    }

    // Clear and save
    EphemeralManifest::delete_file()?;
    Output::success(format!("Cleared {} tracked local changes.", manifest.len()));

    Ok(())
}

/// Show the path to the ephemeral manifest.
fn show_path() -> Result<()> {
    let path = EphemeralManifest::path();
    println!("{}", path.display());
    Ok(())
}

/// Run the local command.
pub fn run(args: LocalArgs, plan: &ExecutionPlan) -> Result<()> {
    match args.action {
        LocalAction::List { format, domain } => list_changes(format, domain),
        LocalAction::Commit {
            message,
            domain,
            select,
        } => commit_changes(plan, message, domain, select),
        LocalAction::Clear { force } => clear_changes(force, plan.dry_run),
        LocalAction::Path => show_path(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_domain_filter() {
        assert_eq!(
            parse_domain_filter("flatpak").unwrap(),
            ChangeDomain::Flatpak
        );
        assert_eq!(parse_domain_filter("fp").unwrap(), ChangeDomain::Flatpak);
        assert_eq!(
            parse_domain_filter("extension").unwrap(),
            ChangeDomain::Extension
        );
        assert_eq!(parse_domain_filter("ext").unwrap(), ChangeDomain::Extension);
        assert_eq!(parse_domain_filter("dnf").unwrap(), ChangeDomain::Dnf);
        assert_eq!(parse_domain_filter("rpm").unwrap(), ChangeDomain::Dnf);
        assert!(parse_domain_filter("invalid").is_err());
    }

    #[test]
    fn test_group_changes_by_domain() {
        let changes = vec![
            EphemeralChange::new(ChangeDomain::Flatpak, ChangeAction::Add, "app1"),
            EphemeralChange::new(ChangeDomain::Dnf, ChangeAction::Add, "pkg1"),
            EphemeralChange::new(ChangeDomain::Flatpak, ChangeAction::Remove, "app2"),
            EphemeralChange::new(ChangeDomain::Gsetting, ChangeAction::Update, "schema.key"),
        ];
        let refs: Vec<&EphemeralChange> = changes.iter().collect();
        let grouped = group_changes_by_domain(&refs);

        assert_eq!(grouped.get(&ChangeDomain::Flatpak).unwrap().len(), 2);
        assert_eq!(grouped.get(&ChangeDomain::Dnf).unwrap().len(), 1);
        assert_eq!(grouped.get(&ChangeDomain::Gsetting).unwrap().len(), 1);
        assert!(grouped.get(&ChangeDomain::Shim).is_none());
    }

    #[test]
    fn test_apply_flatpak_changes_add() {
        let temp = assert_fs::TempDir::new().unwrap();
        let manifests_dir = temp.path();

        // Create empty manifest
        let manifest_path = manifests_dir.join("flatpak-apps.json");
        std::fs::write(&manifest_path, r#"{"apps": []}"#).unwrap();

        let changes = vec![EphemeralChange::new(
            ChangeDomain::Flatpak,
            ChangeAction::Add,
            "org.test.App",
        )];
        let refs: Vec<&EphemeralChange> = changes.iter().collect();

        let result = apply_flatpak_changes(&refs, manifests_dir).unwrap();

        // Verify the content includes the new app
        assert!(result.content.contains("org.test.App"));
        assert!(result.content.contains("flathub")); // default remote
    }

    #[test]
    fn test_apply_extension_changes_add() {
        let temp = assert_fs::TempDir::new().unwrap();
        let manifests_dir = temp.path();

        // Create empty manifest
        let manifest_path = manifests_dir.join("gnome-extensions.json");
        std::fs::write(&manifest_path, r#"{"extensions": []}"#).unwrap();

        let changes = vec![EphemeralChange::new(
            ChangeDomain::Extension,
            ChangeAction::Add,
            "test@example.com",
        )];
        let refs: Vec<&EphemeralChange> = changes.iter().collect();

        let result = apply_extension_changes(&refs, manifests_dir).unwrap();

        assert!(result.content.contains("test@example.com"));
    }

    #[test]
    fn test_apply_dnf_changes_add() {
        let temp = assert_fs::TempDir::new().unwrap();
        let manifests_dir = temp.path();

        // Create empty manifest
        let manifest_path = manifests_dir.join("system-packages.json");
        std::fs::write(
            &manifest_path,
            r#"{"packages": [], "groups": [], "excluded": [], "copr_repos": []}"#,
        )
        .unwrap();

        let changes = vec![EphemeralChange::new(
            ChangeDomain::Dnf,
            ChangeAction::Add,
            "htop",
        )];
        let refs: Vec<&EphemeralChange> = changes.iter().collect();

        let result = apply_dnf_changes(&refs, manifests_dir).unwrap();

        assert!(result.content.contains("htop"));
    }

    #[test]
    fn test_apply_shim_changes_add() {
        let temp = assert_fs::TempDir::new().unwrap();
        let manifests_dir = temp.path();

        // Create empty manifest
        let manifest_path = manifests_dir.join("host-shims.json");
        std::fs::write(&manifest_path, r#"{"shims": []}"#).unwrap();

        let changes = vec![EphemeralChange::new(
            ChangeDomain::Shim,
            ChangeAction::Add,
            "docker",
        )];
        let refs: Vec<&EphemeralChange> = changes.iter().collect();

        let result = apply_shim_changes(&refs, manifests_dir).unwrap();

        assert!(result.content.contains("docker"));
    }

    #[test]
    fn test_apply_gsetting_changes() {
        let temp = assert_fs::TempDir::new().unwrap();
        let manifests_dir = temp.path();

        // Create empty manifest
        let manifest_path = manifests_dir.join("gsettings.json");
        std::fs::write(&manifest_path, r#"{"settings": []}"#).unwrap();

        let mut change = EphemeralChange::new(
            ChangeDomain::Gsetting,
            ChangeAction::Update,
            "org.gnome.desktop.interface.color-scheme",
        );
        change
            .metadata
            .insert("value".to_string(), "'dark'".to_string());

        let changes = vec![change];
        let refs: Vec<&EphemeralChange> = changes.iter().collect();

        let result = apply_gsetting_changes(&refs, manifests_dir).unwrap();

        assert!(result.content.contains("org.gnome.desktop.interface"));
        assert!(result.content.contains("color-scheme"));
        assert!(result.content.contains("'dark'"));
    }

    #[test]
    fn test_generate_pr_body() {
        let mut changes = HashMap::new();
        changes.insert(
            "flatpak-apps.json".to_string(),
            ManifestChange {
                content: "{}".to_string(),
            },
        );
        changes.insert(
            "system-packages.json".to_string(),
            ManifestChange {
                content: "{}".to_string(),
            },
        );

        let body = generate_pr_body(&changes);

        assert!(body.contains("bkt local commit"));
        assert!(body.contains("flatpak-apps.json"));
        assert!(body.contains("system-packages.json"));
    }
}
