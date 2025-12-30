//! Flatpak command implementation.

use anyhow::Result;
use clap::{Args, Subcommand};

#[derive(Debug, Args)]
pub struct FlatpakArgs {
    #[command(subcommand)]
    pub action: FlatpakAction,
}

#[derive(Debug, Subcommand)]
pub enum FlatpakAction {
    /// Add a Flatpak app to the manifest
    Add {
        /// Application ID (e.g., org.gnome.Calculator)
        app_id: String,
        /// Remote name (default: flathub)
        #[arg(short, long, default_value = "flathub")]
        remote: String,
        /// Installation scope
        #[arg(short, long, default_value = "system")]
        scope: String,
        /// Create a PR with the change
        #[arg(long)]
        pr: bool,
    },
    /// Remove a Flatpak app from the manifest
    Remove {
        /// Application ID to remove
        app_id: String,
        /// Create a PR with the change
        #[arg(long)]
        pr: bool,
    },
    /// List all Flatpak apps in the manifest
    List {
        /// Output format (table, json)
        #[arg(short, long, default_value = "table")]
        format: String,
    },
    /// Sync manifest with installed apps
    Sync {
        /// Show what would be done without making changes
        #[arg(long)]
        dry_run: bool,
        /// Create a PR with the changes
        #[arg(long)]
        pr: bool,
    },
}

pub fn run(args: FlatpakArgs) -> Result<()> {
    match args.action {
        FlatpakAction::Add {
            app_id,
            remote,
            scope,
            pr,
        } => {
            println!(
                "TODO: Add flatpak {} from {} ({}) [pr={}]",
                app_id, remote, scope, pr
            );
        }
        FlatpakAction::Remove { app_id, pr } => {
            println!("TODO: Remove flatpak {} [pr={}]", app_id, pr);
        }
        FlatpakAction::List { format } => {
            println!("TODO: List flatpaks (format={})", format);
        }
        FlatpakAction::Sync { dry_run, pr } => {
            println!("TODO: Sync flatpaks [dry_run={}, pr={}]", dry_run, pr);
        }
    }
    Ok(())
}
