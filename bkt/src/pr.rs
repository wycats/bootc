//! PR automation workflow.
//!
//! Provides the `--pr` flag functionality: apply locally AND open a PR.

use crate::repo::{RepoConfig, find_repo_path};
use anyhow::{Context, Result, bail};
use std::path::PathBuf;
use std::process::Command;

/// Information about a manifest change for PR creation.
pub struct PrChange {
    pub manifest_type: String, // "shim", "flatpak", "extension", etc.
    pub action: String,        // "add", "remove"
    pub name: String,          // Item name
    pub manifest_file: String, // e.g., "host-shims.json"
}

impl PrChange {
    pub fn branch_name(&self) -> String {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        format!(
            "bkt/{}-{}-{}-{}",
            self.manifest_type, self.action, self.name, timestamp
        )
    }

    pub fn commit_message(&self) -> String {
        format!(
            "feat(manifests): {} {} {}",
            self.action, self.manifest_type, self.name
        )
    }

    pub fn pr_title(&self) -> String {
        format!("{} {} `{}`", self.action, self.manifest_type, self.name)
    }

    pub fn pr_body(&self) -> String {
        format!(
            "This PR was automatically created by `bkt {} {} --pr`.\n\n\
             ## Changes\n\
             - {} `{}` in `manifests/{}`\n\n\
             ---\n\
             *Created by bkt CLI*",
            self.manifest_type,
            self.action,
            if self.action == "add" {
                "Added"
            } else {
                "Removed"
            },
            self.name,
            self.manifest_file
        )
    }
}

/// Find or clone the source repository.
pub fn ensure_repo() -> Result<PathBuf> {
    // First, try to find existing checkout
    if let Ok(path) = find_repo_path() {
        return Ok(path);
    }

    // Load repo config to know where to clone from
    let config = RepoConfig::load()
        .context("Cannot find repo.json - is bkt installed from a proper image?")?;

    // Clone to default location
    let data_dir = directories::BaseDirs::new()
        .map(|d| d.data_dir().to_path_buf())
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var("HOME").unwrap_or_default())
                .join(".local")
                .join("share")
        });
    let repo_path = data_dir.join("bootc").join("source");

    if repo_path.exists() {
        // Pull latest
        println!("Updating existing checkout at {}", repo_path.display());
        let status = Command::new("git")
            .args(["pull", "--rebase"])
            .current_dir(&repo_path)
            .status()
            .context("Failed to run git pull")?;
        if !status.success() {
            bail!("git pull failed");
        }
    } else {
        // Clone
        println!("Cloning {} to {}", config.url, repo_path.display());
        std::fs::create_dir_all(repo_path.parent().unwrap())?;
        let status = Command::new("git")
            .args(["clone", &config.url, repo_path.to_str().unwrap()])
            .status()
            .context("Failed to run git clone")?;
        if !status.success() {
            bail!("git clone failed");
        }
    }

    Ok(repo_path)
}

/// Check if gh CLI is authenticated.
fn check_gh_auth() -> Result<()> {
    let output = Command::new("gh")
        .args(["auth", "status"])
        .output()
        .context("Failed to run gh auth status - is gh installed?")?;

    if !output.status.success() {
        bail!(
            "GitHub CLI not authenticated. Run: gh auth login\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

/// Run the PR workflow for a manifest change.
pub fn run_pr_workflow(change: &PrChange, manifest_content: &str) -> Result<()> {
    check_gh_auth()?;

    let repo_path = ensure_repo()?;
    let manifest_path = repo_path.join("manifests").join(&change.manifest_file);

    // Create branch
    let branch = change.branch_name();
    println!("Creating branch: {}", branch);

    let status = Command::new("git")
        .args(["checkout", "-b", &branch])
        .current_dir(&repo_path)
        .status()
        .context("Failed to create branch")?;
    if !status.success() {
        bail!("Failed to create branch {}", branch);
    }

    // Write updated manifest
    std::fs::write(&manifest_path, manifest_content)
        .with_context(|| format!("Failed to write {}", manifest_path.display()))?;

    // Commit
    let status = Command::new("git")
        .args(["add", manifest_path.to_str().unwrap()])
        .current_dir(&repo_path)
        .status()?;
    if !status.success() {
        bail!("git add failed");
    }

    let status = Command::new("git")
        .args(["commit", "-m", &change.commit_message()])
        .current_dir(&repo_path)
        .status()?;
    if !status.success() {
        bail!("git commit failed");
    }

    // Push
    println!("Pushing branch...");
    let status = Command::new("git")
        .args(["push", "-u", "origin", &branch])
        .current_dir(&repo_path)
        .status()?;
    if !status.success() {
        bail!("git push failed");
    }

    // Create PR
    println!("Creating pull request...");
    let status = Command::new("gh")
        .args([
            "pr",
            "create",
            "--title",
            &change.pr_title(),
            "--body",
            &change.pr_body(),
        ])
        .current_dir(&repo_path)
        .status()?;
    if !status.success() {
        bail!("gh pr create failed");
    }

    // Return to default branch
    let config = RepoConfig::load()?;
    let _ = Command::new("git")
        .args(["checkout", &config.default_branch])
        .current_dir(&repo_path)
        .status();

    println!("✓ PR created successfully!");
    Ok(())
}
