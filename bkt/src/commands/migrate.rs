//! Migration utilities for legacy user config files.

use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use std::path::{Path, PathBuf};

use crate::output::Output;
use crate::repo::find_repo_path;

#[derive(Debug, Args)]
pub struct MigrateArgs {
    #[command(subcommand)]
    pub action: MigrateAction,
}

#[derive(Debug, Subcommand)]
pub enum MigrateAction {
    /// Migrate user manifests from ~/.config/bootc/ into the repo
    Manifests {
        /// Overwrite repo manifests with user manifests on conflict
        #[arg(long)]
        force: bool,
    },
}

pub fn run(args: MigrateArgs, dry_run: bool) -> Result<()> {
    match args.action {
        MigrateAction::Manifests { force } => migrate_manifests(dry_run, force),
    }
}

fn migrate_manifests(dry_run: bool, force: bool) -> Result<()> {
    Output::header("bkt migrate manifests");
    Output::info("Scanning ~/.config/bootc/ for user manifests...");
    Output::blank();

    let repo_path = find_repo_path()?;
    let manifests_dir = repo_path.join("manifests");
    if !dry_run && !manifests_dir.is_dir() {
        std::fs::create_dir_all(&manifests_dir).with_context(|| {
            format!("Failed to create manifests directory at {}", manifests_dir.display())
        })?;
    }

    let user_dir = user_config_dir()?;
    if !user_dir.is_dir() {
        Output::info("No user manifests found in ~/.config/bootc/.");
        return Ok(());
    }

    let user_files = list_user_manifests(&user_dir)?;
    if user_files.is_empty() {
        Output::info("No user manifests found in ~/.config/bootc/.");
        return Ok(());
    }

    let mut conflicts = 0usize;
    let mut migrated = 0usize;

    for user_file in user_files {
        let Some(file_name) = user_file.file_name().and_then(|s| s.to_str()) else {
            Output::warning("Skipping file with invalid UTF-8 name in ~/.config/bootc/");
            continue;
        };

        let repo_file = manifests_dir.join(file_name);
        let repo_exists = repo_file.is_file();

        if repo_exists {
            if force {
                Output::step(format!(
                    "{} — overwriting repo manifest with user version",
                    file_name
                ));
                if dry_run {
                    Output::dry_run(format!(
                        "Would migrate {} (overwrite repo)",
                        file_name
                    ));
                    continue;
                }
                overwrite_file(&user_file, &repo_file)?;
                Output::success(format!("Migrated {}", file_name));
                Output::success(format!("Removed {}", user_file.display()));
                migrated += 1;
            } else {
                Output::step(format!(
                    "{} — conflict (both repo and user exist)",
                    file_name
                ));
                Output::hint("Use --force to overwrite repo with user version");
                conflicts += 1;
            }
            continue;
        }

        Output::step(format!("{} — migrating user manifest", file_name));
        if dry_run {
            Output::dry_run(format!("Would migrate {}", file_name));
            continue;
        }

        move_file(&user_file, &repo_file)?;
        Output::success(format!("Migrated {}", file_name));
        Output::success(format!("Removed {}", user_file.display()));
        migrated += 1;
    }

    Output::blank();

    if conflicts > 0 {
        Output::warning(format!(
            "{} conflict{} found. Use --force to overwrite, or manually merge.",
            conflicts,
            if conflicts == 1 { "" } else { "s" }
        ));
        return Ok(());
    }

    if migrated == 0 {
        Output::info("No user manifests needed migration.");
        return Ok(());
    }

    Output::success("Migration complete. Run `git diff manifests/` to review changes.");
    Ok(())
}

fn user_config_dir() -> Result<PathBuf> {
    let home = std::env::var("HOME").context("Cannot determine $HOME")?;
    Ok(PathBuf::from(home).join(".config/bootc"))
}

fn list_user_manifests(dir: &Path) -> Result<Vec<PathBuf>> {
    let entries = std::fs::read_dir(dir)
        .with_context(|| format!("Cannot read {}", dir.display()))?;

    let mut files = Vec::new();
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        files.push(path);
    }

    files.sort_by_key(|path| path.file_name().map(|s| s.to_os_string()));
    Ok(files)
}

fn overwrite_file(src: &Path, dest: &Path) -> Result<()> {
    if let Err(err) = std::fs::rename(src, dest) {
        std::fs::copy(src, dest)
            .with_context(|| format!("Failed to copy {}", src.display()))?;
        std::fs::remove_file(src)
            .with_context(|| format!("Failed to remove {}", src.display()))?;
        // Keep the error detail for debugging if rename failed unexpectedly.
        tracing::debug!("rename failed, copied instead: {}", err);
    }
    Ok(())
}

fn move_file(src: &Path, dest: &Path) -> Result<()> {
    if let Err(err) = std::fs::rename(src, dest) {
        std::fs::copy(src, dest)
            .with_context(|| format!("Failed to copy {}", src.display()))?;
        std::fs::remove_file(src)
            .with_context(|| format!("Failed to remove {}", src.display()))?;
        tracing::debug!("rename failed, copied instead: {}", err);
    }
    Ok(())
}
