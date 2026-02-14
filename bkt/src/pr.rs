//! PR automation workflow.
//!
//! Provides the `--pr` flag functionality: apply locally AND open a PR.
//!
//! # Security
//!
//! This module handles user-controlled input that flows into git commands.
//! All inputs are validated to prevent command injection:
//! - Branch names are sanitized to alphanumeric, hyphens, underscores, and dots
//! - Manifest file paths are validated against path traversal
//! - Item names are sanitized before use in branch names
//!
//! # Errors
//!
//! The PR workflow can fail at multiple points:
//! - Pre-flight checks: `gh` or `git` not configured
//! - Network: Failed to clone/push to remote
//! - Auth: GitHub authentication issues
//! - Git state: Conflicts, dirty working directory

use crate::command_runner::{CommandOptions, CommandRunner, RealCommandRunner};
use crate::repo::{RepoConfig, find_repo_path};
use anyhow::{Context, Result, bail};
use std::path::PathBuf;
use std::sync::Arc;

/// Trait for PR creation operations - enables testing without git/gh.
pub trait PrBackend: Send + Sync {
    /// Create a PR with the given manifest changes.
    fn create_pr(
        &self,
        change: &PrChange,
        manifest_content: &str,
        skip_preflight: bool,
    ) -> Result<()>;
}

/// Production backend that uses real git/gh commands.
pub struct GitHubBackend {
    command_runner: Arc<dyn CommandRunner>,
}

impl GitHubBackend {
    pub fn new(runner: Arc<dyn CommandRunner>) -> Self {
        Self {
            command_runner: runner,
        }
    }
}

impl Default for GitHubBackend {
    fn default() -> Self {
        Self::new(Arc::new(RealCommandRunner))
    }
}

impl PrBackend for GitHubBackend {
    fn create_pr(
        &self,
        change: &PrChange,
        manifest_content: &str,
        skip_preflight: bool,
    ) -> Result<()> {
        run_pr_workflow(
            &*self.command_runner,
            change,
            manifest_content,
            skip_preflight,
        )
    }
}

/// Characters allowed in git ref names (conservative subset).
/// Alphanumeric plus hyphen, underscore, and dot.
fn is_safe_ref_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.'
}

/// Sanitize a string for use in git ref names.
/// Replaces unsafe characters with hyphens and truncates to avoid length issues.
fn sanitize_for_ref(s: &str) -> String {
    let sanitized: String = s
        .chars()
        .map(|c| if is_safe_ref_char(c) { c } else { '-' })
        .collect();
    // Git has a 255-byte limit on ref names; keep it well under
    if sanitized.len() > 50 {
        sanitized[..50].to_string()
    } else {
        sanitized
    }
}

/// Validate that a manifest file path is safe (no path traversal).
fn validate_manifest_path(path: &str) -> Result<()> {
    if path.contains("..") || path.starts_with('/') || path.contains('\\') {
        bail!("Invalid manifest path: {}", path);
    }
    // Only allow paths within manifests/ or skel/
    if !path.ends_with(".json") && !path.starts_with("skel/") {
        bail!("Manifest path must be a .json file or in skel/: {}", path);
    }
    Ok(())
}

/// Validate that a branch name pattern is safe.
fn validate_branch_pattern(branch: &str) -> Result<()> {
    if branch.chars().all(|c| is_safe_ref_char(c) || c == '/') {
        Ok(())
    } else {
        bail!("Invalid branch name: {}", branch);
    }
}

/// Result of a pre-flight check.
#[derive(Debug)]
pub struct PreflightResult {
    pub name: String,
    pub passed: bool,
    pub message: String,
    pub fix_hint: Option<String>,
}

impl PreflightResult {
    fn pass(name: &str, message: &str) -> Self {
        Self {
            name: name.to_string(),
            passed: true,
            message: message.to_string(),
            fix_hint: None,
        }
    }

    fn fail(name: &str, message: &str, fix_hint: &str) -> Self {
        Self {
            name: name.to_string(),
            passed: false,
            message: message.to_string(),
            fix_hint: if fix_hint.trim().is_empty() {
                None
            } else {
                Some(fix_hint.to_string())
            },
        }
    }
}

/// Run all pre-flight checks for the PR workflow.
/// Returns Ok if all checks pass, or Err with details about what failed.
pub fn run_preflight_checks(runner: &dyn CommandRunner) -> Result<Vec<PreflightResult>> {
    Ok(vec![
        // Check 1: gh CLI available
        check_gh_available(runner),
        // Check 2: gh authenticated
        check_gh_auth_status(runner),
        // Check 3: git available
        check_git_available(runner),
        // Check 4: git user.name configured
        check_git_user_name(runner),
        // Check 5: git user.email configured
        check_git_user_email(runner),
        // Check 6: repo.json exists
        check_repo_config(),
    ])
}

fn check_gh_available(runner: &dyn CommandRunner) -> PreflightResult {
    match runner.run_output("gh", &["--version"], &CommandOptions::default()) {
        Ok(output) if output.status.success() => {
            let version = String::from_utf8_lossy(&output.stdout);
            let version_line = version.lines().next().unwrap_or("gh installed");
            PreflightResult::pass("gh CLI", version_line)
        }
        _ => PreflightResult::fail(
            "gh CLI",
            "GitHub CLI (gh) not found",
            "Install: dnf install gh",
        ),
    }
}

fn check_gh_auth_status(runner: &dyn CommandRunner) -> PreflightResult {
    let output = match runner.run_output("gh", &["auth", "status"], &CommandOptions::default()) {
        Ok(o) => o,
        Err(_) => {
            return PreflightResult::fail(
                "gh auth",
                "Cannot run gh auth status",
                "Install gh first: dnf install gh",
            );
        }
    };

    if output.status.success() {
        // Parse the output to get username
        let stderr = String::from_utf8_lossy(&output.stderr);
        let username = stderr
            .lines()
            .find(|l| l.contains("Logged in to"))
            .and_then(|l| l.split("as ").nth(1))
            .and_then(|s| s.split_whitespace().next())
            .unwrap_or("authenticated");
        PreflightResult::pass("gh auth", &format!("Authenticated as {}", username))
    } else {
        PreflightResult::fail(
            "gh auth",
            "GitHub CLI not authenticated",
            "Run: gh auth login",
        )
    }
}

fn check_git_available(runner: &dyn CommandRunner) -> PreflightResult {
    match runner.run_output("git", &["--version"], &CommandOptions::default()) {
        Ok(output) if output.status.success() => {
            let version = String::from_utf8_lossy(&output.stdout);
            PreflightResult::pass("git", version.trim())
        }
        _ => PreflightResult::fail("git", "git not found", "Install: dnf install git"),
    }
}

fn check_git_user_name(runner: &dyn CommandRunner) -> PreflightResult {
    let output = match runner.run_output(
        "git",
        &["config", "--get", "user.name"],
        &CommandOptions::default(),
    ) {
        Ok(o) => o,
        Err(_) => {
            return PreflightResult::fail(
                "git user.name",
                "Cannot run git config",
                "Ensure git is installed",
            );
        }
    };

    if output.status.success() {
        let name = String::from_utf8_lossy(&output.stdout);
        PreflightResult::pass("git user.name", name.trim())
    } else {
        PreflightResult::fail(
            "git user.name",
            "Git user name not configured",
            "Run: git config --global user.name \"Your Name\"",
        )
    }
}

fn check_git_user_email(runner: &dyn CommandRunner) -> PreflightResult {
    let output = match runner.run_output(
        "git",
        &["config", "--get", "user.email"],
        &CommandOptions::default(),
    ) {
        Ok(o) => o,
        Err(_) => {
            return PreflightResult::fail(
                "git user.email",
                "Cannot run git config",
                "Ensure git is installed",
            );
        }
    };

    if output.status.success() {
        let email = String::from_utf8_lossy(&output.stdout);
        PreflightResult::pass("git user.email", email.trim())
    } else {
        PreflightResult::fail(
            "git user.email",
            "Git user email not configured",
            "Run: git config --global user.email \"you@example.com\"",
        )
    }
}

fn check_repo_config() -> PreflightResult {
    match RepoConfig::load() {
        Ok(config) => {
            PreflightResult::pass("repo.json", &format!("{}/{}", config.owner, config.name))
        }
        Err(e) => PreflightResult::fail("repo.json", &e.to_string(), ""),
    }
}

/// Check all preflight conditions and return error if any fail.
/// Set `skip` to true to bypass checks (for --skip-preflight flag).
pub fn ensure_preflight(runner: &dyn CommandRunner, skip: bool) -> Result<()> {
    if skip {
        return Ok(());
    }

    let results = run_preflight_checks(runner)?;
    let failed: Vec<_> = results.iter().filter(|r| !r.passed).collect();

    if failed.is_empty() {
        return Ok(());
    }

    eprintln!("\n✗ Pre-flight checks failed:\n");
    for result in &failed {
        eprintln!("  ✗ {}: {}", result.name, result.message);
        if let Some(hint) = &result.fix_hint {
            eprintln!("    → {}", hint);
        }
    }
    eprintln!();

    bail!("Pre-flight checks failed. Fix the issues above or use --skip-preflight to bypass.");
}

/// Information about a manifest change for PR creation.
#[derive(Debug, Clone)]
pub struct PrChange {
    pub manifest_type: String, // "shim", "flatpak", "extension", etc.
    pub action: String,        // "add", "remove"
    pub name: String,          // Item name
    pub manifest_file: String, // e.g., "host-shims.json"
}

impl PrChange {
    /// Generate a safe branch name for this change.
    /// All components are sanitized to prevent command injection.
    pub fn branch_name(&self) -> String {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or_else(|_| {
                // Fallback: use process ID for uniqueness
                std::process::id() as u64
            });
        // Sanitize all user-controlled components
        let safe_type = sanitize_for_ref(&self.manifest_type);
        let safe_action = sanitize_for_ref(&self.action);
        let safe_name = sanitize_for_ref(&self.name);
        format!(
            "bkt/{}-{}-{}-{}",
            safe_type, safe_action, safe_name, timestamp
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
pub fn ensure_repo(runner: &dyn CommandRunner) -> Result<PathBuf> {
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

        // Check for uncommitted changes and commit them first
        let status_output = runner
            .run_output(
                "git",
                &["status", "--porcelain"],
                &CommandOptions::with_cwd(&repo_path),
            )
            .context("Failed to check git status")?;

        if !status_output.status.success() {
            bail!("git status failed");
        }

        if !status_output.stdout.is_empty() {
            println!("Committing local changes before pull...");
            // Stage all changes
            let add_status = runner
                .run_status("git", &["add", "-A"], &CommandOptions::with_cwd(&repo_path))
                .context("Failed to stage changes")?;
            if !add_status.success() {
                bail!("git add failed");
            }

            // Commit with auto-message
            let commit_status = runner
                .run_status(
                    "git",
                    &["commit", "-m", "Auto-commit local changes before sync"],
                    &CommandOptions::with_cwd(&repo_path),
                )
                .context("Failed to commit changes")?;
            if !commit_status.success() {
                bail!("git commit failed");
            }
        }

        let status = runner
            .run_status(
                "git",
                &["pull", "--rebase"],
                &CommandOptions::with_cwd(&repo_path),
            )
            .context("Failed to run git pull")?;
        if !status.success() {
            bail!("git pull failed");
        }
    } else {
        // Clone
        println!("Cloning {} to {}", config.url, repo_path.display());
        std::fs::create_dir_all(
            repo_path
                .parent()
                .ok_or_else(|| anyhow::anyhow!("Invalid repo path: no parent directory"))?,
        )?;
        let repo_path_str = repo_path
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("Invalid repo path"))?;
        let status = runner
            .run_status(
                "git",
                &["clone", &config.url, repo_path_str],
                &CommandOptions::default(),
            )
            .context("Failed to run git clone")?;
        if !status.success() {
            bail!("git clone failed");
        }
    }

    Ok(repo_path)
}

/// Run the PR workflow for a manifest change.
/// Set `skip_preflight` to true to bypass pre-flight checks.
///
/// # Errors
///
/// Returns an error if:
/// - Pre-flight checks fail (gh/git not configured)
/// - Repository cannot be cloned or updated
/// - Git operations fail (branch creation, commit, push)
/// - GitHub PR creation fails
/// - Manifest path validation fails (path traversal attempt)
pub fn run_pr_workflow(
    runner: &dyn CommandRunner,
    change: &PrChange,
    manifest_content: &str,
    skip_preflight: bool,
) -> Result<()> {
    // Validate manifest path before proceeding
    validate_manifest_path(&change.manifest_file)?;

    ensure_preflight(runner, skip_preflight)?;

    let repo_path = ensure_repo(runner)?;

    // Determine the full path - skel files go directly, others go in manifests/
    let manifest_path = if change.manifest_file.starts_with("skel/") {
        repo_path.join(&change.manifest_file)
    } else {
        repo_path.join("manifests").join(&change.manifest_file)
    };

    // Create branch
    let branch = change.branch_name();
    // Validate branch name is safe (should always pass due to sanitization)
    validate_branch_pattern(&branch)?;
    println!("Creating branch: {}", branch);

    let status = runner
        .run_status(
            "git",
            &["checkout", "-b", &branch],
            &CommandOptions::with_cwd(&repo_path),
        )
        .context("Failed to create branch")?;
    if !status.success() {
        bail!("Failed to create branch {}", branch);
    }

    // Write updated manifest
    std::fs::write(&manifest_path, manifest_content)
        .with_context(|| format!("Failed to write {}", manifest_path.display()))?;

    // Commit
    let manifest_path_str = manifest_path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("Invalid manifest path"))?;
    let status = runner.run_status(
        "git",
        &["add", "--", manifest_path_str],
        &CommandOptions::with_cwd(&repo_path),
    )?;
    if !status.success() {
        bail!("git add failed");
    }

    let status = runner.run_status(
        "git",
        &["commit", "-m", &change.commit_message()],
        &CommandOptions::with_cwd(&repo_path),
    )?;
    if !status.success() {
        bail!("git commit failed");
    }

    // Push
    println!("Pushing branch...");
    let status = runner.run_status(
        "git",
        &["push", "-u", "origin", &branch],
        &CommandOptions::with_cwd(&repo_path),
    )?;
    if !status.success() {
        bail!("git push failed");
    }

    // Create PR
    println!("Creating pull request...");
    let status = runner.run_status(
        "gh",
        &[
            "pr",
            "create",
            "--title",
            &change.pr_title(),
            "--body",
            &change.pr_body(),
        ],
        &CommandOptions::with_cwd(&repo_path),
    )?;
    if !status.success() {
        bail!("gh pr create failed");
    }

    // Return to default branch
    let config = RepoConfig::load()?;
    validate_branch_pattern(&config.default_branch)?;
    match runner.run_status(
        "git",
        &["checkout", &config.default_branch],
        &CommandOptions::with_cwd(&repo_path),
    ) {
        Ok(status) if !status.success() => {
            eprintln!(
                "Warning: failed to switch back to '{}' branch",
                config.default_branch
            );
        }
        Err(e) => {
            eprintln!("Warning: failed to run git checkout: {}", e);
        }
        _ => {}
    }

    println!("✓ PR created successfully!");
    Ok(())
}

/// Test utilities for PR backend mocking.
#[cfg(test)]
#[allow(dead_code)]
pub mod testing {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// Recorded PR creation call for test assertions.
    #[derive(Debug, Clone)]
    pub struct PrCall {
        pub change: PrChange,
        pub manifest_content: String,
        pub skip_preflight: bool,
    }

    /// Mock backend that records PR creation calls for testing.
    #[derive(Default)]
    pub struct MockPrBackend {
        calls: Arc<Mutex<Vec<PrCall>>>,
    }

    impl MockPrBackend {
        pub fn new() -> Self {
            Self::default()
        }

        /// Get all recorded PR creation calls.
        pub fn calls(&self) -> Vec<PrCall> {
            self.calls.lock().unwrap().clone()
        }

        /// Assert that no PRs were created.
        pub fn assert_no_pr(&self) {
            let calls = self.calls();
            assert!(calls.is_empty(), "Expected no PRs, got {}", calls.len());
        }
    }

    impl PrBackend for MockPrBackend {
        fn create_pr(
            &self,
            change: &PrChange,
            manifest_content: &str,
            skip_preflight: bool,
        ) -> Result<()> {
            self.calls.lock().unwrap().push(PrCall {
                change: change.clone(),
                manifest_content: manifest_content.to_string(),
                skip_preflight,
            });
            Ok(())
        }
    }
}
