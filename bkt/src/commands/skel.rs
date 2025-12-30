//! Skel (skeleton files) command implementation.

use anyhow::Result;
use clap::{Args, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Args)]
pub struct SkelArgs {
    #[command(subcommand)]
    pub action: SkelAction,
}

#[derive(Debug, Subcommand)]
pub enum SkelAction {
    /// Add a file to skel/
    Add {
        /// Source file path
        source: PathBuf,
        /// Destination path within skel (relative to home)
        #[arg(short, long)]
        dest: Option<PathBuf>,
        /// Create a PR with the change
        #[arg(long)]
        pr: bool,
    },
    /// Show diff between skel and current home directory
    Diff {
        /// Specific file to diff (optional)
        file: Option<PathBuf>,
    },
    /// List all files in skel/
    List {
        /// Output format (table, tree, json)
        #[arg(short, long, default_value = "tree")]
        format: String,
    },
    /// Migrate files from home to skel
    Migrate {
        /// File pattern or path to migrate
        pattern: String,
        /// Show what would be done without making changes
        #[arg(long)]
        dry_run: bool,
        /// Create a PR with the change
        #[arg(long)]
        pr: bool,
    },
}

pub fn run(args: SkelArgs) -> Result<()> {
    match args.action {
        SkelAction::Add { source, dest, pr } => {
            println!(
                "TODO: Add to skel {:?} -> {:?} [pr={}]",
                source, dest, pr
            );
        }
        SkelAction::Diff { file } => {
            println!("TODO: Diff skel {:?}", file);
        }
        SkelAction::List { format } => {
            println!("TODO: List skel (format={})", format);
        }
        SkelAction::Migrate {
            pattern,
            dry_run,
            pr,
        } => {
            println!(
                "TODO: Migrate to skel '{}' [dry_run={}, pr={}]",
                pattern, dry_run, pr
            );
        }
    }
    Ok(())
}
