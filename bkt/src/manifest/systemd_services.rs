//! Systemd services manifest types.

use anyhow::{Context, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// The expected unit file state for a systemd service.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ServiceState {
    Enabled,
    Disabled,
    Masked,
}

impl ServiceState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Enabled => "enabled",
            Self::Disabled => "disabled",
            Self::Masked => "masked",
        }
    }

    /// Check if a systemd unit file state matches this expected state.
    ///
    /// Systemd reports various states beyond enabled/disabled/masked:
    /// - "static" — unit has no [Install] section, always available
    /// - "indirect" — unit is enabled via an alias
    /// - "generated" — unit was generated dynamically
    /// - "enabled-runtime" — enabled temporarily (until reboot)
    /// - "masked-runtime" — masked temporarily (until reboot)
    ///
    /// This method normalizes these to our three-state model.
    pub fn matches_systemd_state(&self, actual: &str) -> bool {
        match self {
            Self::Enabled => matches!(
                actual,
                "enabled" | "enabled-runtime" | "static" | "indirect" | "generated"
            ),
            Self::Disabled => matches!(actual, "disabled"),
            Self::Masked => matches!(actual, "masked" | "masked-runtime"),
        }
    }
}

/// The systemd-services.json manifest.
#[derive(Debug, Clone, Serialize, Deserialize, Default, JsonSchema)]
pub struct SystemdServicesManifest {
    #[serde(rename = "$schema", skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub services: HashMap<String, ServiceState>,
}

impl SystemdServicesManifest {
    /// Project manifest path (relative to workspace root).
    pub const PROJECT_PATH: &'static str = "manifests/systemd-services.json";

    /// Load a manifest from a path.
    pub fn load(path: &PathBuf) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = fs::read_to_string(path).with_context(|| {
            format!(
                "Failed to read systemd services manifest from {}",
                path.display()
            )
        })?;
        let manifest: Self = serde_json::from_str(&content).with_context(|| {
            format!(
                "Failed to parse systemd services manifest from {}",
                path.display()
            )
        })?;
        Ok(manifest)
    }

    /// Save a manifest to a path.
    pub fn save(&self, path: &PathBuf) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory {}", parent.display()))?;
        }
        let content = serde_json::to_string_pretty(self)
            .context("Failed to serialize systemd services manifest")?;
        fs::write(path, content).with_context(|| {
            format!(
                "Failed to write systemd services manifest to {}",
                path.display()
            )
        })?;
        Ok(())
    }

    /// Load from the repository's manifests directory.
    pub fn load_repo() -> Result<Self> {
        let repo = crate::repo::find_repo_path()?;
        Self::load(&repo.join(Self::PROJECT_PATH))
    }

    /// Save to the repository's manifests directory.
    pub fn save_repo(&self) -> Result<()> {
        let repo = crate::repo::find_repo_path()?;
        self.save(&repo.join(Self::PROJECT_PATH))
    }
}
