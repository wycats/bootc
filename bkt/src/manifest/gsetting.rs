//! GSettings manifest types.

use anyhow::{Context, Result};
use directories::BaseDirs;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// A GSettings entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GSetting {
    /// Schema name (e.g., "org.gnome.settings-daemon.plugins.power")
    pub schema: String,
    /// Key name (e.g., "sleep-inactive-ac-type")
    pub key: String,
    /// Value as a GVariant string (e.g., "'nothing'" or "0")
    pub value: String,
    /// Optional comment explaining the setting
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

impl GSetting {
    /// Get a unique key for this setting (schema + key).
    pub fn unique_key(&self) -> String {
        format!("{}.{}", self.schema, self.key)
    }
}

/// The gsettings.json manifest.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GSettingsManifest {
    #[serde(rename = "$schema", skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    pub settings: Vec<GSetting>,
}

impl GSettingsManifest {
    /// System manifest path (baked into image).
    pub const SYSTEM_PATH: &'static str = "/usr/share/bootc-bootstrap/gsettings.json";

    /// Load a manifest from a path.
    pub fn load(path: &PathBuf) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = fs::read_to_string(path).with_context(|| {
            format!("Failed to read gsettings manifest from {}", path.display())
        })?;
        let manifest: Self = serde_json::from_str(&content).with_context(|| {
            format!(
                "Failed to parse gsettings manifest from {}",
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
        let content =
            serde_json::to_string_pretty(self).context("Failed to serialize gsettings manifest")?;
        fs::write(path, content).with_context(|| {
            format!(
                "Failed to write gsettings manifest to {}",
                path.display()
            )
        })?;
        Ok(())
    }

    /// Get the user manifest path.
    pub fn user_path() -> PathBuf {
        let config_dir = BaseDirs::new()
            .map(|d| d.config_dir().to_path_buf())
            .unwrap_or_else(|| {
                PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config")
            });
        config_dir.join("bootc").join("gsettings.json")
    }

    /// Load the system manifest.
    pub fn load_system() -> Result<Self> {
        Self::load(&PathBuf::from(Self::SYSTEM_PATH))
    }

    /// Load the user manifest.
    pub fn load_user() -> Result<Self> {
        Self::load(&Self::user_path())
    }

    /// Save the user manifest.
    pub fn save_user(&self) -> Result<()> {
        self.save(&Self::user_path())
    }

    /// Merge system and user manifests (user overrides by schema+key).
    pub fn merged(system: &Self, user: &Self) -> Self {
        let mut by_key: HashMap<String, GSetting> = HashMap::new();

        for setting in &system.settings {
            by_key.insert(setting.unique_key(), setting.clone());
        }
        for setting in &user.settings {
            by_key.insert(setting.unique_key(), setting.clone());
        }

        let mut settings: Vec<GSetting> = by_key.into_values().collect();
        settings.sort_by(|a, b| a.unique_key().cmp(&b.unique_key()));

        Self {
            schema: None,
            settings,
        }
    }

    /// Find a setting by schema and key.
    pub fn find(&self, schema: &str, key: &str) -> Option<&GSetting> {
        self.settings
            .iter()
            .find(|s| s.schema == schema && s.key == key)
    }

    /// Add or update a setting.
    pub fn upsert(&mut self, setting: GSetting) {
        if let Some(existing) = self
            .settings
            .iter_mut()
            .find(|s| s.schema == setting.schema && s.key == setting.key)
        {
            *existing = setting;
        } else {
            self.settings.push(setting);
        }
        self.settings
            .sort_by(|a, b| a.unique_key().cmp(&b.unique_key()));
    }

    /// Remove a setting. Returns true if removed.
    pub fn remove(&mut self, schema: &str, key: &str) -> bool {
        let len_before = self.settings.len();
        self.settings
            .retain(|s| !(s.schema == schema && s.key == key));
        self.settings.len() < len_before
    }
}
