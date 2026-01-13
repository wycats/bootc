//! Systemd subcommand implementation for `bkt admin systemd`.
//!
//! Manages persistent systemd configurations in the `system-config.json` manifest.
//!
//! Note: This is distinct from `bkt admin systemctl` which performs immediate
//! runtime operations. This command manages the *image* configuration.

use anyhow::Result;
use clap::Subcommand;

use crate::manifest::system_config::{SystemConfigManifest, SystemdConfig};
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
            SystemdAction::Enable { units } => Self::apply_enable(&mut systemd, units),
            SystemdAction::Disable { units } => Self::apply_disable(&mut systemd, units),
            SystemdAction::Mask { units } => Self::apply_mask(&mut systemd, units),
            SystemdAction::List => {
                Self::list(&systemd);
                return Ok(());
            }
        }

        manifest.systemd = Some(systemd);
        manifest.save()?;

        Ok(())
    }

    fn apply_enable(config: &mut SystemdConfig, units: Vec<String>) {
        for unit in units {
            if !config.enable.contains(&unit) {
                config.enable.push(unit.clone());
                Output::success(format!("Enabled unit: {}", unit));
            } else {
                Output::info(format!("Unit already enabled: {}", unit));
            }
            // Clean up conflicting states
            if let Some(pos) = config.disable.iter().position(|x| x == &unit) {
                config.disable.remove(pos);
            }
            if let Some(pos) = config.mask.iter().position(|x| x == &unit) {
                config.mask.remove(pos);
            }
        }
    }

    fn apply_disable(config: &mut SystemdConfig, units: Vec<String>) {
        for unit in units {
            if !config.disable.contains(&unit) {
                config.disable.push(unit.clone());
                Output::success(format!("Disabled unit: {}", unit));
            } else {
                Output::info(format!("Unit already disabled: {}", unit));
            }
            // Clean up conflicting states
            if let Some(pos) = config.enable.iter().position(|x| x == &unit) {
                config.enable.remove(pos);
            }
        }
    }

    fn apply_mask(config: &mut SystemdConfig, units: Vec<String>) {
        for unit in units {
            if !config.mask.contains(&unit) {
                config.mask.push(unit.clone());
                Output::success(format!("Masked unit: {}", unit));
            } else {
                Output::info(format!("Unit already masked: {}", unit));
            }
            // Clean up conflicting states
            if let Some(pos) = config.enable.iter().position(|x| x == &unit) {
                config.enable.remove(pos);
            }
        }
    }

    fn list(config: &SystemdConfig) {
        Output::subheader("Systemd Configuration (Manifest)");
        if !config.enable.is_empty() {
            Output::kv("Enable", "");
            for unit in &config.enable {
                Output::list_item(unit);
            }
        }
        if !config.disable.is_empty() {
            Output::kv("Disable", "");
            for unit in &config.disable {
                Output::list_item(unit);
            }
        }
        if !config.mask.is_empty() {
            Output::kv("Mask", "");
            for unit in &config.mask {
                Output::list_item(unit);
            }
        }
        if !config.custom.is_empty() {
            Output::kv("Custom", "");
            for unit in &config.custom {
                Output::list_item(unit);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_enable_removes_conflicts() {
        let mut config = SystemdConfig::default();
        config.disable.push("foo.service".to_string());
        config.mask.push("foo.service".to_string());

        SystemdAction::apply_enable(&mut config, vec!["foo.service".to_string()]);

        assert!(config.enable.contains(&"foo.service".to_string()));
        assert!(!config.disable.contains(&"foo.service".to_string()));
        assert!(!config.mask.contains(&"foo.service".to_string()));
    }

    #[test]
    fn test_disable_removes_enable() {
        let mut config = SystemdConfig::default();
        config.enable.push("foo.service".to_string());

        SystemdAction::apply_disable(&mut config, vec!["foo.service".to_string()]);

        assert!(config.disable.contains(&"foo.service".to_string()));
        assert!(!config.enable.contains(&"foo.service".to_string()));
    }

    #[test]
    fn test_mask_removes_enable() {
        let mut config = SystemdConfig::default();
        config.enable.push("foo.service".to_string());

        SystemdAction::apply_mask(&mut config, vec!["foo.service".to_string()]);

        assert!(config.mask.contains(&"foo.service".to_string()));
        assert!(!config.enable.contains(&"foo.service".to_string()));
    }
}
