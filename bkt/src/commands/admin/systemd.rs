//! Systemd subcommand implementation for `bkt admin systemd`.
//!
//! Manages persistent systemd configurations in the `system-config.json` manifest.
//!
//! Note: This is distinct from `bkt admin systemctl` which performs immediate
//! runtime operations. This command manages the *image* configuration.

use anyhow::Result;
use clap::Subcommand;

use crate::manifest::system_config::SystemConfigManifest;
use crate::output::Output;
use crate::pipeline::ExecutionPlan;

/// Systemd operations.
#[derive(Debug, Subcommand)]
pub enum SystemdAction {
    /// Enable systemd units
    Enable {
        /// Units to enable
        #[arg(required = true)]
        units: Vec<String>,
    },

    /// Disable systemd units
    Disable {
        /// Units to disable
        #[arg(required = true)]
        units: Vec<String>,
    },

    /// Mask systemd units
    Mask {
        /// Units to mask
        #[arg(required = true)]
        units: Vec<String>,
    },

    /// List managed systemd units
    List,
}

impl SystemdAction {
    pub fn execute(self, _plan: &ExecutionPlan) -> Result<()> {
        let mut manifest = SystemConfigManifest::load()?;
        let mut systemd = manifest.systemd.unwrap_or_default();

        match self {
            SystemdAction::Enable { units } => {
                for unit in units {
                    if !systemd.enable.contains(&unit) {
                        systemd.enable.push(unit.clone());
                        Output::success(format!("Enabled unit: {}", unit));
                    } else {
                        Output::info(format!("Unit already enabled: {}", unit));
                    }
                    // Clean up conflicting states
                    if let Some(pos) = systemd.disable.iter().position(|x| x == &unit) {
                        systemd.disable.remove(pos);
                    }
                    if let Some(pos) = systemd.mask.iter().position(|x| x == &unit) {
                        systemd.mask.remove(pos);
                    }
                }
            }
            SystemdAction::Disable { units } => {
                for unit in units {
                    if !systemd.disable.contains(&unit) {
                        systemd.disable.push(unit.clone());
                        Output::success(format!("Disabled unit: {}", unit));
                    } else {
                        Output::info(format!("Unit already disabled: {}", unit));
                    }
                    // Clean up conflicting states
                    if let Some(pos) = systemd.enable.iter().position(|x| x == &unit) {
                        systemd.enable.remove(pos);
                    }
                }
            }
            SystemdAction::Mask { units } => {
                for unit in units {
                    if !systemd.mask.contains(&unit) {
                        systemd.mask.push(unit.clone());
                        Output::success(format!("Masked unit: {}", unit));
                    } else {
                        Output::info(format!("Unit already masked: {}", unit));
                    }
                    // Clean up conflicting states
                    if let Some(pos) = systemd.enable.iter().position(|x| x == &unit) {
                        systemd.enable.remove(pos);
                    }
                }
            }
            SystemdAction::List => {
                Output::subheader("Systemd Configuration (Manifest)");
                if !systemd.enable.is_empty() {
                    Output::kv("Enable", "");
                    for unit in &systemd.enable {
                        Output::list_item(unit);
                    }
                }
                if !systemd.disable.is_empty() {
                    Output::kv("Disable", "");
                    for unit in &systemd.disable {
                        Output::list_item(unit);
                    }
                }
                if !systemd.mask.is_empty() {
                    Output::kv("Mask", "");
                    for unit in &systemd.mask {
                        Output::list_item(unit);
                    }
                }
                if !systemd.custom.is_empty() {
                    Output::kv("Custom", "");
                    for unit in &systemd.custom {
                        Output::list_item(unit);
                    }
                }

                return Ok(());
            }
        }

        manifest.systemd = Some(systemd);
        manifest.save()?;

        Ok(())
    }
}
