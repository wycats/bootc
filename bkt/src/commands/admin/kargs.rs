//! Kargs subcommand implementation for `bkt admin kargs`.
//!
//! Manages persistent kernel arguments in the `system-config.json` manifest.

use anyhow::Result;
use clap::Subcommand;

use crate::manifest::system_config::{KargsConfig, SystemConfigManifest};
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
            KargsAction::Append { args } => Self::apply_append(&mut kargs, args),
            KargsAction::Remove { args } => Self::apply_remove(&mut kargs, args),
            KargsAction::List => {
                Self::list(&kargs);
                return Ok(());
            }
        }

        manifest.kargs = Some(kargs);
        manifest.save()?;

        Ok(())
    }

    fn apply_append(config: &mut KargsConfig, args: Vec<String>) {
        for arg in args {
            if !config.append.contains(&arg) {
                config.append.push(arg.clone());
                Output::success(format!("Added karg: {}", arg));
            } else {
                Output::info(format!("Karg already exists: {}", arg));
            }
            // If it was in remove list, remove it from there
            if let Some(pos) = config.remove.iter().position(|x| x == &arg) {
                config.remove.remove(pos);
            }
        }
    }

    fn apply_remove(config: &mut KargsConfig, args: Vec<String>) {
        for arg in args {
            if !config.remove.contains(&arg) {
                config.remove.push(arg.clone());
                Output::success(format!("Arranged removal of karg: {}", arg));
            } else {
                Output::info(format!("Karg removal already arranged: {}", arg));
            }
            // If it was in append list, remove it from there
            if let Some(pos) = config.append.iter().position(|x| x == &arg) {
                config.append.remove(pos);
            }
        }
    }

    fn list(config: &KargsConfig) {
        Output::subheader("Kernel Arguments (Manifest)");
        if !config.append.is_empty() {
            Output::kv("Append", "");
            for arg in &config.append {
                Output::list_item(arg);
            }
        } else {
            Output::info("No arguments to append.");
        }

        if !config.remove.is_empty() {
            Output::kv("Remove", "");
            for arg in &config.remove {
                Output::list_item(arg);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_append_removes_from_remove() {
        let mut config = KargsConfig::default();
        config.remove.push("console=ttyS0".to_string());

        KargsAction::apply_append(&mut config, vec!["console=ttyS0".to_string()]);

        assert!(config.append.contains(&"console=ttyS0".to_string()));
        assert!(!config.remove.contains(&"console=ttyS0".to_string()));
    }

    #[test]
    fn test_remove_removes_from_append() {
        let mut config = KargsConfig::default();
        config.append.push("quiet".to_string());

        KargsAction::apply_remove(&mut config, vec!["quiet".to_string()]);

        assert!(config.remove.contains(&"quiet".to_string()));
        assert!(!config.append.contains(&"quiet".to_string()));
    }
}
