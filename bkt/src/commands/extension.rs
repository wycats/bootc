//! GNOME extension command implementation.

use anyhow::Result;
use clap::{Args, Subcommand};

#[derive(Debug, Args)]
pub struct ExtensionArgs {
    #[command(subcommand)]
    pub action: ExtensionAction,
}

#[derive(Debug, Subcommand)]
pub enum ExtensionAction {
    /// Add a GNOME extension to the manifest
    Add {
        /// Extension UUID (e.g., dash-to-dock@micxgx.gmail.com)
        uuid: String,
        /// Create a PR with the change
        #[arg(long)]
        pr: bool,
    },
    /// Remove a GNOME extension from the manifest
    Remove {
        /// Extension UUID to remove
        uuid: String,
        /// Create a PR with the change
        #[arg(long)]
        pr: bool,
    },
    /// List all GNOME extensions in the manifest
    List {
        /// Output format (table, json)
        #[arg(short, long, default_value = "table")]
        format: String,
    },
}

pub fn run(args: ExtensionArgs) -> Result<()> {
    match args.action {
        ExtensionAction::Add { uuid, pr } => {
            println!("TODO: Add extension {} [pr={}]", uuid, pr);
        }
        ExtensionAction::Remove { uuid, pr } => {
            println!("TODO: Remove extension {} [pr={}]", uuid, pr);
        }
        ExtensionAction::List { format } => {
            println!("TODO: List extensions (format={})", format);
        }
    }
    Ok(())
}
