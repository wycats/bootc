//! Skel (skeleton files) command implementation.
//!
//! Manages dotfiles that get copied to /etc/skel in the image.
//!
//! # Security
//!
//! This module validates file paths to prevent path traversal attacks.
//! Files can only be copied from within $HOME, and paths containing ".."
//! are rejected.

use crate::command_runner::{CommandOptions, CommandRunner};
use crate::output::Output;
use crate::pipeline::ExecutionPlan;
use crate::repo::find_repo_path;
use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use owo_colors::OwoColorize;
use std::fs;
use std::path::{Path, PathBuf};

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

/// Result of comparing two files
enum DiffResult {
    Identical,
    Different(String),
    MissingInHome,
    MissingInSkel,
}

/// Compare two files and return diff result.
fn diff_files(
    skel_file: &Path,
    home_file: &Path,
    runner: &dyn CommandRunner,
) -> Result<DiffResult> {
    if !home_file.exists() {
        return Ok(DiffResult::MissingInHome);
    }

    if !skel_file.exists() {
        return Ok(DiffResult::MissingInSkel);
    }

    let skel_arg = skel_file.to_str().unwrap_or_default();
    let home_arg = home_file.to_str().unwrap_or_default();

    let output = runner
        .run_output(
            "diff",
            &["-u", "--", skel_arg, home_arg],
            &CommandOptions::default(),
        )
        .context("Failed to run diff")?;

    if output.status.success() {
        Ok(DiffResult::Identical)
    } else {
        Ok(DiffResult::Different(
            String::from_utf8_lossy(&output.stdout).to_string(),
        ))
    }
}

/// Print a colored unified diff
fn print_colored_diff(diff: &str) {
    for line in diff.lines() {
        if line.starts_with("+++") || line.starts_with("---") {
            // File headers - bold
            println!("{}", line.bold());
        } else if line.starts_with("@@") {
            // Hunk headers - cyan
            println!("{}", line.cyan());
        } else if line.starts_with('+') {
            // Additions - green
            println!("{}", line.green());
        } else if line.starts_with('-') {
            // Deletions - red
            println!("{}", line.red());
        } else {
            // Context lines
            println!("{}", line);
        }
    }
}

pub fn run(args: SkelArgs, plan: &ExecutionPlan) -> Result<()> {
    let runner = plan.runner();

    match args.action {
        SkelAction::Add { file } => {
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

            // Skel add always copies to the repo's skel/ directory (not a manifest)
            // We don't use should_update_local_manifest because this is a file copy, not manifest update
            if !plan.dry_run {
                // Create parent directories if needed
                if let Some(parent) = dest.parent() {
                    fs::create_dir_all(parent).with_context(|| {
                        format!("Failed to create directory {}", parent.display())
                    })?;
                }

                // Copy the file
                fs::copy(&source, &dest).with_context(|| {
                    format!("Failed to copy {} to {}", source.display(), dest.display())
                })?;

                Output::success(format!("Added to skel: {}", file));
                Output::hint(format!("{} → {}", source.display(), dest.display()));
            } else {
                Output::dry_run(format!("Would copy {} to skel/{}", source.display(), file));
            }

            if plan.should_create_pr() {
                // Read the file content for the PR. In dry-run mode, the destination file
                // does not exist yet, so read from the source instead.
                let pr_source = if plan.dry_run { &source } else { &dest };
                let content = fs::read_to_string(pr_source)
                    .with_context(|| format!("Failed to read {}", pr_source.display()))?;

                plan.maybe_create_pr("skel", "add", &file, &format!("skel/{}", file), &content)?;
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

                println!("\n{}", format!("━━━ {} ━━━", file).bold());
                match diff_files(&skel_file, &home_file, runner)? {
                    DiffResult::Identical => Output::success("Files are identical"),
                    DiffResult::Different(diff) => print_colored_diff(&diff),
                    DiffResult::MissingInHome => {
                        println!("  {} File missing in $HOME", "⚠".yellow());
                        println!("  Run {} to create it", "bkt skel sync".cyan());
                    }
                    DiffResult::MissingInSkel => {
                        println!("  {} File missing in skel/", "⚠".yellow());
                    }
                }
            } else {
                // Diff all skel files
                let files = list_skel_files(&skel)?;

                if files.is_empty() {
                    Output::info("No files in skel/");
                    return Ok(());
                }

                let mut identical_files = Vec::new();
                let mut different_files = Vec::new();
                let mut missing_files = Vec::new();

                // First pass: categorize files
                for file in &files {
                    let skel_file = skel.join(file);
                    let home_file = home.join(file);

                    match diff_files(&skel_file, &home_file, runner)? {
                        DiffResult::Identical => identical_files.push(file.clone()),
                        DiffResult::Different(_) => different_files.push(file.clone()),
                        DiffResult::MissingInHome => missing_files.push(file.clone()),
                        DiffResult::MissingInSkel => {} // Shouldn't happen when iterating skel files
                    }
                }

                // Print header
                println!("\n{}", "bkt skel diff".bold());
                println!();

                // Show summary first
                println!(
                    "  {} {} synced, {} {}, {} {}",
                    "Summary:".dimmed(),
                    identical_files.len().to_string().green(),
                    different_files.len().to_string().yellow(),
                    "differ".yellow(),
                    missing_files.len().to_string().cyan(),
                    "missing in $HOME".cyan()
                );
                println!();

                // Show differing files with diffs
                if !different_files.is_empty() {
                    for file in &different_files {
                        let skel_file = skel.join(file);
                        let home_file = home.join(file);

                        println!("{}", format!("━━━ {} ━━━", file.display()).bold().yellow());
                        println!("  {} skel/{}", "←".red(), file.display());
                        println!("  {} $HOME/{}", "→".green(), file.display());
                        println!();

                        if let DiffResult::Different(diff) =
                            diff_files(&skel_file, &home_file, runner)?
                        {
                            print_colored_diff(&diff);
                        }
                        println!();
                    }
                }

                // Show missing files
                if !missing_files.is_empty() {
                    println!("{}", "━━━ Missing in $HOME ━━━".bold().cyan());
                    for file in &missing_files {
                        println!("  {} {}", "⚠".yellow(), file.display());
                    }
                    println!("\n  Run {} to create these files", "bkt skel sync".cyan());
                    println!();
                }

                // Show identical files (collapsed)
                if !identical_files.is_empty() {
                    println!(
                        "{} {} files are in sync",
                        "✓".green(),
                        identical_files.len()
                    );
                }
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
        SkelAction::Sync { force } => {
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
                    if plan.dry_run {
                        Output::dry_run(format!("Would skip (exists): {}", file.display()));
                    }
                    skipped += 1;
                    continue;
                }

                if plan.dry_run {
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
            if plan.dry_run {
                Output::info(format!(
                    "Dry run: {} would be copied, {} would be skipped",
                    copied, skipped
                ));
            } else {
                Output::info(format!(
                    "Sync complete: {} copied, {} skipped",
                    copied, skipped
                ));
            }
        }
    }
    Ok(())
}
