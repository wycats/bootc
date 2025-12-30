//! System profile command implementation.

use anyhow::Result;
use clap::{Args, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Args)]
pub struct ProfileArgs {
    #[command(subcommand)]
    pub action: ProfileAction,
}

#[derive(Debug, Subcommand)]
pub enum ProfileAction {
    /// Capture current system profile
    Capture {
        /// Output file (defaults to system_profile.json)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Show diff between captured profile and current system
    Diff {
        /// Profile file to compare (defaults to system_profile.json)
        #[arg(short, long)]
        profile: Option<PathBuf>,
        /// Show only specific sections
        #[arg(short, long)]
        section: Option<String>,
    },
    /// Show unowned files (not in RPM database)
    Unowned {
        /// Directory to scan
        #[arg(short, long, default_value = "/usr/local/bin")]
        dir: PathBuf,
        /// Output format (table, json)
        #[arg(short, long, default_value = "table")]
        format: String,
    },
}

pub fn run(args: ProfileArgs) -> Result<()> {
    match args.action {
        ProfileAction::Capture { output } => {
            println!("TODO: Capture profile to {:?}", output);
        }
        ProfileAction::Diff { profile, section } => {
            println!(
                "TODO: Diff profile {:?} (section={:?})",
                profile, section
            );
        }
        ProfileAction::Unowned { dir, format } => {
            println!(
                "TODO: Show unowned files in {:?} (format={})",
                dir, format
            );
        }
    }
    Ok(())
}
