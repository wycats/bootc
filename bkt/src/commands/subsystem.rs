//! Subsystem listing command.
//!
//! `bkt subsystem list` shows all registered subsystems and their capabilities.

use anyhow::Result;
use clap::{Args, Subcommand};
use owo_colors::OwoColorize;

use crate::output::Output;
use crate::subsystem::SubsystemRegistry;

#[derive(Debug, Args)]
pub struct SubsystemArgs {
    #[command(subcommand)]
    pub action: SubsystemAction,
}

#[derive(Debug, Subcommand)]
pub enum SubsystemAction {
    /// List all registered subsystems
    List {
        /// Output format (table, json)
        #[arg(short, long, default_value = "table")]
        format: String,
    },
}

/// Subsystem info for JSON output.
#[derive(Debug, serde::Serialize)]
struct SubsystemInfo {
    id: &'static str,
    name: &'static str,
    supports_capture: bool,
    supports_sync: bool,
}

pub fn run(args: SubsystemArgs) -> Result<()> {
    match args.action {
        SubsystemAction::List { format } => {
            let registry = SubsystemRegistry::builtin();

            if format == "json" {
                let info: Vec<SubsystemInfo> = registry
                    .all()
                    .iter()
                    .map(|s| SubsystemInfo {
                        id: s.id(),
                        name: s.name(),
                        supports_capture: s.supports_capture(),
                        supports_sync: s.supports_sync(),
                    })
                    .collect();
                println!("{}", serde_json::to_string_pretty(&info)?);
                return Ok(());
            }

            // Table format
            Output::header("Registered Subsystems");
            println!(
                "{:<12} {:<20} {:<10} {:<10}",
                "ID".cyan(),
                "NAME".cyan(),
                "CAPTURE".cyan(),
                "SYNC".cyan()
            );
            Output::separator();

            for subsystem in registry.all() {
                let capture = if subsystem.supports_capture() {
                    "✓".green().to_string()
                } else {
                    "—".dimmed().to_string()
                };
                let sync = if subsystem.supports_sync() {
                    "✓".green().to_string()
                } else {
                    "—".dimmed().to_string()
                };

                println!(
                    "{:<12} {:<20} {:<10} {:<10}",
                    subsystem.id(),
                    subsystem.name(),
                    capture,
                    sync
                );
            }

            Output::blank();
            Output::info(format!("{} subsystems registered", registry.all().len()));

            Ok(())
        }
    }
}
