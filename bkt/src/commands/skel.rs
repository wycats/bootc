//! Skel (skeleton files) command implementation.
//!
//! Manages dotfiles that get copied to /etc/skel in the image.
//!
//! # Security
//!
//! This module validates file paths to prevent path traversal attacks.
//! Files can only be copied from within $HOME, and paths containing ".."
//! are rejected.

use crate::output::Output;
use crate::pipeline::ExecutionPlan;
use crate::pr::{PrChange, run_pr_workflow};
use crate::repo::find_repo_path;
use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Args)]
pub struct SkelArgs {
    #[command(subcommand)]
    pub action: SkelAction,
}

#[derive(Debug, Subcommand)]
pub enum SkelAction {
    /// Add a file from $HOME to skel/
    Add {
        /// File path relative to $HOME (e.g., .bashrc or .config/foo/bar)
        file: String,
        /// Create a PR with the change
        #[arg(long)]
        pr: bool,
        /// Skip pre-flight checks for PR workflow
        #[arg(long)]
        skip_preflight: bool,
    },
    /// Show diff between skel files and current $HOME
    Diff {
        /// Specific file to diff (optional, relative to $HOME)
        file: Option<String>,
    },
    /// List all files in skel/
    List,
    /// Sync skel files to $HOME (copies skel → $HOME)
    Sync {
        /// Show what would be done without making changes
        #[arg(long)]
        dry_run: bool,
        /// Force overwrite existing files
        #[arg(long)]
        force: bool,
    },
}

/// Get the skel directory in the repo.
fn skel_dir() -> Result<PathBuf> {
    let repo = find_repo_path()?;
    Ok(repo.join("skel"))
}

/// Get home directory.
/// Uses the directories crate for reliable cross-platform support.
fn home_dir() -> Result<PathBuf> {
    directories::BaseDirs::new()
        .map(|d| d.home_dir().to_path_buf())
        .or_else(|| std::env::var("HOME").ok().map(PathBuf::from))
        .ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))
}

/// Validate that a file path is safe (no path traversal).
fn validate_skel_path(path: &str) -> Result<()> {
    if path.contains("..") {
        bail!("Path traversal not allowed: {}", path);
    }
    if path.starts_with('/') {
        bail!("Absolute paths not allowed: {}", path);
    }
    Ok(())
}

/// List all files in skel directory recursively.
fn list_skel_files(skel: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    if !skel.exists() {
        return Ok(files);
    }

    fn walk(dir: &Path, base: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
        for entry in fs::read_dir(dir).context("Failed to read skel directory")? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                walk(&path, base, files)?;
            } else {
                let relative = path.strip_prefix(base).unwrap_or(&path);
                files.push(relative.to_path_buf());
            }
        }
        Ok(())
    }

    walk(skel, skel, &mut files)?;
    files.sort();
    Ok(files)
}

/// Compare two files and show diff.
fn diff_files(skel_file: &Path, home_file: &Path) -> Result<Option<String>> {
    if !home_file.exists() {
        return Ok(Some(format!(
            "File does not exist in $HOME: {}",
            home_file.display()
        )));
    }

    if !skel_file.exists() {
        return Ok(Some(format!(
            "File does not exist in skel: {}",
            skel_file.display()
        )));
    }

    let output = Command::new("diff")
        .args(["-u", "--color=auto", "--"])
        .arg(skel_file)
        .arg(home_file)
        .output()
        .context("Failed to run diff")?;

    if output.status.success() {
        // Files are identical
        Ok(None)
    } else {
        Ok(Some(String::from_utf8_lossy(&output.stdout).to_string()))
    }
}

pub fn run(args: SkelArgs, _plan: &ExecutionPlan) -> Result<()> {
    // TODO: Migrate to use `ExecutionPlan` instead of per-command flags.
    // The `_plan` parameter is intentionally unused and reserved for future use
    // after this migration.
    match args.action {
        SkelAction::Add {
            file,
            pr,
            skip_preflight,
        } => {
            let home = home_dir()?;
            let skel = skel_dir()?;

            // Normalize the file path (remove leading ./ or ~/)
            let file = file
                .trim_start_matches("./")
                .trim_start_matches("~/")
                .to_string();

            // Validate the path is safe
            validate_skel_path(&file)?;

            let source = home.join(&file);
            let dest = skel.join(&file);

            // Verify source is actually within home directory
            let canonical_source = source
                .canonicalize()
                .with_context(|| format!("Cannot resolve path: {}", source.display()))?;
            let canonical_home = home
                .canonicalize()
                .with_context(|| "Cannot resolve home directory")?;
            if !canonical_source.starts_with(&canonical_home) {
                bail!(
                    "Source file must be within home directory: {}",
                    source.display()
                );
            }

            if !source.exists() {
                bail!("Source file does not exist: {}", source.display());
            }

            // Create parent directories if needed
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("Failed to create directory {}", parent.display()))?;
            }

            // Copy the file
            fs::copy(&source, &dest).with_context(|| {
                format!("Failed to copy {} to {}", source.display(), dest.display())
            })?;

            Output::success(format!("Added to skel: {}", file));
            Output::hint(format!("{} → {}", source.display(), dest.display()));

            if pr {
                // Read the file content for the PR
                let content = fs::read_to_string(&dest)
                    .with_context(|| format!("Failed to read {}", dest.display()))?;

                let change = PrChange {
                    manifest_type: "skel".to_string(),
                    action: "add".to_string(),
                    name: file.clone(),
                    manifest_file: format!("skel/{}", file),
                };

                // For skel, we write the actual file content
                run_pr_workflow(&change, &content, skip_preflight)?;
            }
        }
        SkelAction::Diff { file } => {
            let home = home_dir()?;
            let skel = skel_dir()?;

            if let Some(file) = file {
                // Diff specific file
                let file = file
                    .trim_start_matches("./")
                    .trim_start_matches("~/")
                    .to_string();

                let skel_file = skel.join(&file);
                let home_file = home.join(&file);

                Output::subheader(format!("=== {} ===", file));
                match diff_files(&skel_file, &home_file)? {
                    Some(diff) => println!("{}", diff),
                    None => Output::success("Files are identical"),
                }
            } else {
                // Diff all skel files
                let files = list_skel_files(&skel)?;

                if files.is_empty() {
                    Output::info("No files in skel/");
                    return Ok(());
                }

                let mut identical = 0;
                let mut different = 0;
                let mut missing = 0;

                for file in &files {
                    let skel_file = skel.join(file);
                    let home_file = home.join(file);

                    match diff_files(&skel_file, &home_file)? {
                        Some(diff) => {
                            if home_file.exists() {
                                Output::subheader(format!("=== {} ===", file.display()));
                                println!("{}", diff);
                                different += 1;
                            } else {
                                Output::subheader(format!("=== {} ===", file.display()));
                                Output::warning("missing in $HOME");
                                Output::blank();
                                missing += 1;
                            }
                        }
                        None => {
                            identical += 1;
                        }
                    }
                }

                Output::blank();
                Output::info(format!(
                    "Summary: {} identical, {} different, {} missing in $HOME",
                    identical, different, missing
                ));
            }
        }
        SkelAction::List => {
            let skel = skel_dir()?;
            let files = list_skel_files(&skel)?;

            if files.is_empty() {
                Output::info("No files in skel/");
                return Ok(());
            }

            Output::subheader(format!("FILES IN SKEL/ ({}):", files.len()));
            for file in &files {
                Output::list_item(file.display().to_string());
            }
        }
        SkelAction::Sync { dry_run, force } => {
            let home = home_dir()?;
            let skel = skel_dir()?;
            let files = list_skel_files(&skel)?;

            if files.is_empty() {
                Output::info("No files in skel/");
                return Ok(());
            }

            let mut copied = 0;
            let mut skipped = 0;

            for file in &files {
                let skel_file = skel.join(file);
                let home_file = home.join(file);

                if home_file.exists() && !force {
                    if dry_run {
                        Output::dry_run(format!("Would skip (exists): {}", file.display()));
                    }
                    skipped += 1;
                    continue;
                }

                if dry_run {
                    Output::dry_run(format!(
                        "Would copy: {} → {}",
                        skel_file.display(),
                        home_file.display()
                    ));
                } else {
                    // Create parent directories
                    if let Some(parent) = home_file.parent() {
                        fs::create_dir_all(parent)?;
                    }

                    fs::copy(&skel_file, &home_file).with_context(|| {
                        format!(
                            "Failed to copy {} to {}",
                            skel_file.display(),
                            home_file.display()
                        )
                    })?;
                    Output::success(format!("Copied: {}", file.display()));
                }
                copied += 1;
            }

            Output::blank();
            if dry_run {
                Output::info(format!(
                    "Dry run: {} would be copied, {} would be skipped",
                    copied, skipped
                ));
            } else {
                Output::info(format!("Sync complete: {} copied, {} skipped", copied, skipped));
            }
        }
    }
    Ok(())
}
