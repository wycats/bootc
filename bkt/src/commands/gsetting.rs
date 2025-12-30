//! GSettings command implementation.

use anyhow::Result;
use clap::{Args, Subcommand};

#[derive(Debug, Args)]
pub struct GSettingArgs {
    #[command(subcommand)]
    pub action: GSettingAction,
}

#[derive(Debug, Subcommand)]
pub enum GSettingAction {
    /// Set a GSettings value in the manifest
    Set {
        /// Schema name (e.g., org.gnome.desktop.interface)
        schema: String,
        /// Key name
        key: String,
        /// Value (as GVariant string)
        value: String,
        /// Optional comment
        #[arg(short, long)]
        comment: Option<String>,
        /// Create a PR with the change
        #[arg(long)]
        pr: bool,
    },
    /// Remove a GSettings entry from the manifest
    Unset {
        /// Schema name
        schema: String,
        /// Key name
        key: String,
        /// Create a PR with the change
        #[arg(long)]
        pr: bool,
    },
    /// List all GSettings in the manifest
    List {
        /// Output format (table, json)
        #[arg(short, long, default_value = "table")]
        format: String,
    },
    /// Apply all GSettings from the manifest
    Apply {
        /// Show what would be done without making changes
        #[arg(long)]
        dry_run: bool,
    },
}

pub fn run(args: GSettingArgs) -> Result<()> {
    match args.action {
        GSettingAction::Set {
            schema,
            key,
            value,
            comment,
            pr,
        } => {
            println!(
                "TODO: Set gsetting {}.{} = {} (comment={:?}) [pr={}]",
                schema, key, value, comment, pr
            );
        }
        GSettingAction::Unset { schema, key, pr } => {
            println!("TODO: Unset gsetting {}.{} [pr={}]", schema, key, pr);
        }
        GSettingAction::List { format } => {
            println!("TODO: List gsettings (format={})", format);
        }
        GSettingAction::Apply { dry_run } => {
            println!("TODO: Apply gsettings [dry_run={}]", dry_run);
        }
    }
    Ok(())
}
