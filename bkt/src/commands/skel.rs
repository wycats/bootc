//! Skel (skeleton files) command implementation.
//!
//! Manages dotfiles that get copied to /etc/skel in the image.

use crate::pr::{PrChange, run_pr_workflow};
use crate::repo::find_repo_path;
use anyhow::{Context, Result};
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
fn home_dir() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/home").join(whoami::username()))
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

pub fn run(args: SkelArgs) -> Result<()> {
    match args.action {
        SkelAction::Add { file, pr } => {
            let home = home_dir();
            let skel = skel_dir()?;

            // Normalize the file path (remove leading ./ or ~/)
            let file = file
                .trim_start_matches("./")
                .trim_start_matches("~/")
                .to_string();

            let source = home.join(&file);
            let dest = skel.join(&file);

            if !source.exists() {
                anyhow::bail!("Source file does not exist: {}", source.display());
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

            println!("Added to skel: {}", file);
            println!("  {} → {}", source.display(), dest.display());

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
                run_pr_workflow(&change, &content)?;
            }
        }
        SkelAction::Diff { file } => {
            let home = home_dir();
            let skel = skel_dir()?;

            if let Some(file) = file {
                // Diff specific file
                let file = file
                    .trim_start_matches("./")
                    .trim_start_matches("~/")
                    .to_string();

                let skel_file = skel.join(&file);
                let home_file = home.join(&file);

                println!("=== {} ===\n", file);
                match diff_files(&skel_file, &home_file)? {
                    Some(diff) => println!("{}", diff),
                    None => println!("✓ Files are identical\n"),
                }
            } else {
                // Diff all skel files
                let files = list_skel_files(&skel)?;

                if files.is_empty() {
                    println!("No files in skel/");
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
                                println!("=== {} ===\n{}", file.display(), diff);
                                different += 1;
                            } else {
                                println!("=== {} ===\n  (missing in $HOME)\n", file.display());
                                missing += 1;
                            }
                        }
                        None => {
                            identical += 1;
                        }
                    }
                }

                println!(
                    "\nSummary: {} identical, {} different, {} missing in $HOME",
                    identical, different, missing
                );
            }
        }
        SkelAction::List => {
            let skel = skel_dir()?;
            let files = list_skel_files(&skel)?;

            if files.is_empty() {
                println!("No files in skel/");
                return Ok(());
            }

            println!("Files in skel/ ({}):\n", files.len());
            for file in &files {
                println!("  {}", file.display());
            }
        }
        SkelAction::Sync { dry_run, force } => {
            let home = home_dir();
            let skel = skel_dir()?;
            let files = list_skel_files(&skel)?;

            if files.is_empty() {
                println!("No files in skel/");
                return Ok(());
            }

            let mut copied = 0;
            let mut skipped = 0;

            for file in &files {
                let skel_file = skel.join(file);
                let home_file = home.join(file);

                if home_file.exists() && !force {
                    if dry_run {
                        println!("Would skip (exists): {}", file.display());
                    }
                    skipped += 1;
                    continue;
                }

                if dry_run {
                    println!(
                        "Would copy: {} → {}",
                        skel_file.display(),
                        home_file.display()
                    );
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
                    println!("Copied: {}", file.display());
                }
                copied += 1;
            }

            if dry_run {
                println!(
                    "\nDry run: {} would be copied, {} would be skipped",
                    copied, skipped
                );
            } else {
                println!("\nSync complete: {} copied, {} skipped", copied, skipped);
            }
        }
    }
    Ok(())
}
