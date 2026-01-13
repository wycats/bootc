//! Kargs subcommand implementation for `bkt admin kargs`.
//!
//! Manages persistent kernel arguments in the `system-config.json` manifest.

use anyhow::Result;
use clap::Subcommand;

use crate::manifest::system_config::SystemConfigManifest;
use crate::output::Output;
use crate::pipeline::ExecutionPlan;

/// Kernel arguments operations.
#[derive(Debug, Subcommand)]
pub enum KargsAction {
    /// Append kernel arguments
    Append {
        /// Arguments to append
        #[arg(required = true)]
        args: Vec<String>,
    },

    /// Remove kernel arguments
    Remove {
        /// Arguments to remove
        #[arg(required = true)]
        args: Vec<String>,
    },

    /// List managed kernel arguments
    List,
}

impl KargsAction {
    pub fn execute(self, _plan: &ExecutionPlan) -> Result<()> {
        let mut manifest = SystemConfigManifest::load()?;
        let mut kargs = manifest.kargs.unwrap_or_default();

        match self {
            KargsAction::Append { args } => {
                for arg in args {
                    if !kargs.append.contains(&arg) {
                        kargs.append.push(arg.clone());
                        Output::success(format!("Added karg: {}", arg));
                    } else {
                        Output::info(format!("Karg already exists: {}", arg));
                    }
                    // If it was in remove list, remove it from there
                    if let Some(pos) = kargs.remove.iter().position(|x| x == &arg) {
                        kargs.remove.remove(pos);
                    }
                }
            }
            KargsAction::Remove { args } => {
                for arg in args {
                    if !kargs.remove.contains(&arg) {
                        kargs.remove.push(arg.clone());
                        Output::success(format!("Arranged removal of karg: {}", arg));
                    } else {
                        Output::info(format!("Karg removal already arranged: {}", arg));
                    }
                    // If it was in append list, remove it from there
                    if let Some(pos) = kargs.append.iter().position(|x| x == &arg) {
                        kargs.append.remove(pos);
                    }
                }
            }
            KargsAction::List => {
                Output::subheader("Kernel Arguments (Manifest)");
                if !kargs.append.is_empty() {
                    Output::kv("Append", "");
                    for arg in &kargs.append {
                        Output::list_item(arg);
                    }
                } else {
                    Output::info("No arguments to append.");
                }

                if !kargs.remove.is_empty() {
                    Output::kv("Remove", "");
                    for arg in &kargs.remove {
                        Output::list_item(arg);
                    }
                }

                return Ok(());
            }
        }

        manifest.kargs = Some(kargs);
        manifest.save()?;

        Ok(())
    }
}
