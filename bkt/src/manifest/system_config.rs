//! System configuration manifest types.
//!
//! Tracks system-level configuration like kernel arguments, systemd units,
//! and other administrative settings.

use anyhow::{Context, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// Kernel arguments configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default, JsonSchema)]
pub struct KargsConfig {
    /// Arguments to append to the kernel command line
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub append: Vec<String>,
    /// Arguments to remove from the kernel command line
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub remove: Vec<String>,
}

/// Systemd units configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default, JsonSchema)]
pub struct SystemdConfig {
    /// Units to enable
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub enable: Vec<String>,
    /// Units to disable
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub disable: Vec<String>,
    /// Units to mask
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mask: Vec<String>,
    /// Custom unit files
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub custom: Vec<String>,
}

/// Udev configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default, JsonSchema)]
pub struct UdevConfig {
    /// Udev rules files
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<String>,
}

/// SELinux configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default, JsonSchema)]
pub struct SelinuxConfig {
    /// SELinux booleans to set
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub booleans: HashMap<String, bool>,
}

/// The system-config.json manifest.
///
/// Tracks system configuration applied at image build time.
#[derive(Debug, Clone, Serialize, Deserialize, Default, JsonSchema)]
pub struct SystemConfigManifest {
    #[serde(rename = "$schema", skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,

    /// Kernel arguments
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kargs: Option<KargsConfig>,

    /// Systemd configuration
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub systemd: Option<SystemdConfig>,

    /// Udev configuration
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub udev: Option<UdevConfig>,

    /// SELinux configuration
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selinux: Option<SelinuxConfig>,

    /// Firmware notes/reminders
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub firmware_notes: Vec<String>,
}

impl SystemConfigManifest {
    /// Resolve the path to the system-config.json file in the repo.
    pub fn path() -> Result<PathBuf> {
        let repo_path = crate::repo::find_repo_path()?;
        Ok(repo_path.join("manifests").join("system-config.json"))
    }

    /// Load the manifest from the repository.
    pub fn load() -> Result<Self> {
        let path = Self::path()?;
        Self::load_from_path(&path)
    }

    /// Load a manifest from a specific path.
    pub fn load_from_path(path: &PathBuf) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = fs::read_to_string(path).with_context(|| {
            format!(
                "Failed to read system config manifest from {}",
                path.display()
            )
        })?;
        let manifest: Self = serde_json::from_str(&content).with_context(|| {
            format!(
                "Failed to parse system config manifest from {}",
                path.display()
            )
        })?;
        Ok(manifest)
    }

    /// Save the manifest to the repository.
    pub fn save(&self) -> Result<()> {
        let path = Self::path()?;
        self.save_to_path(&path)
    }

    /// Save a manifest to a specific path.
    pub fn save_to_path(&self, path: &PathBuf) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory {}", parent.display()))?;
        }
        let content = serde_json::to_string_pretty(self)
            .context("Failed to serialize system config manifest")?;
        fs::write(path, content).with_context(|| {
            format!(
                "Failed to write system config manifest to {}",
                path.display()
            )
        })?;
        Ok(())
    }
}
