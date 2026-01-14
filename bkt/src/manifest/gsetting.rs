//! GSettings manifest types.

use crate::component::Resource;
use anyhow::{Context, Result};
use directories::BaseDirs;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// A GSettings entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
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

impl Resource for GSetting {
    type Id = String;

    fn id(&self) -> String {
        self.unique_key()
    }
}

/// The gsettings.json manifest.
#[derive(Debug, Clone, Serialize, Deserialize, Default, JsonSchema)]
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
            format!("Failed to parse gsettings manifest from {}", path.display())
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
        fs::write(path, content)
            .with_context(|| format!("Failed to write gsettings manifest to {}", path.display()))?;
        Ok(())
    }

    /// Get the user manifest path.
    ///
    /// Respects `$HOME` environment variable for test isolation.
    pub fn user_path() -> PathBuf {
        // Prefer $HOME for test isolation, fall back to BaseDirs
        let config_dir = std::env::var("HOME")
            .ok()
            .map(|h| PathBuf::from(h).join(".config"))
            .or_else(|| BaseDirs::new().map(|d| d.config_dir().to_path_buf()))
            .unwrap_or_else(|| PathBuf::from(".config"));
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
        settings.sort_by_key(|a| a.unique_key());

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
        self.settings.sort_by_key(|a| a.unique_key());
    }

    /// Remove a setting. Returns true if removed.
    pub fn remove(&mut self, schema: &str, key: &str) -> bool {
        let len_before = self.settings.len();
        self.settings
            .retain(|s| !(s.schema == schema && s.key == key));
        self.settings.len() < len_before
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_setting(schema: &str, key: &str, value: &str) -> GSetting {
        GSetting {
            schema: schema.to_string(),
            key: key.to_string(),
            value: value.to_string(),
            comment: None,
        }
    }

    fn sample_setting_with_comment(
        schema: &str,
        key: &str,
        value: &str,
        comment: &str,
    ) -> GSetting {
        GSetting {
            schema: schema.to_string(),
            key: key.to_string(),
            value: value.to_string(),
            comment: Some(comment.to_string()),
        }
    }

    #[test]
    fn gsetting_unique_key() {
        let setting = sample_setting(
            "org.gnome.desktop.interface",
            "color-scheme",
            "'prefer-dark'",
        );
        assert_eq!(
            setting.unique_key(),
            "org.gnome.desktop.interface.color-scheme"
        );
    }

    #[test]
    fn manifest_default_is_empty() {
        let manifest = GSettingsManifest::default();
        assert!(manifest.settings.is_empty());
        assert!(manifest.schema.is_none());
    }

    #[test]
    fn manifest_find_returns_matching_setting() {
        let mut manifest = GSettingsManifest::default();
        manifest.settings.push(sample_setting(
            "org.gnome.desktop.interface",
            "color-scheme",
            "'prefer-dark'",
        ));

        let found = manifest.find("org.gnome.desktop.interface", "color-scheme");
        assert!(found.is_some());
        assert_eq!(found.unwrap().value, "'prefer-dark'");
    }

    #[test]
    fn manifest_find_returns_none_for_missing() {
        let manifest = GSettingsManifest::default();
        assert!(manifest.find("nonexistent.schema", "key").is_none());
    }

    #[test]
    fn manifest_upsert_adds_new_setting() {
        let mut manifest = GSettingsManifest::default();
        manifest.upsert(sample_setting(
            "org.gnome.desktop.interface",
            "color-scheme",
            "'prefer-dark'",
        ));

        assert_eq!(manifest.settings.len(), 1);
    }

    #[test]
    fn manifest_upsert_updates_existing_setting() {
        let mut manifest = GSettingsManifest::default();
        manifest.upsert(sample_setting(
            "org.gnome.desktop.interface",
            "color-scheme",
            "'prefer-dark'",
        ));
        manifest.upsert(sample_setting(
            "org.gnome.desktop.interface",
            "color-scheme",
            "'prefer-light'",
        ));

        assert_eq!(manifest.settings.len(), 1);
        assert_eq!(
            manifest
                .find("org.gnome.desktop.interface", "color-scheme")
                .unwrap()
                .value,
            "'prefer-light'"
        );
    }

    #[test]
    fn manifest_upsert_maintains_sorted_order() {
        let mut manifest = GSettingsManifest::default();
        manifest.upsert(sample_setting("z.schema", "key", "val"));
        manifest.upsert(sample_setting("a.schema", "key", "val"));

        assert_eq!(manifest.settings[0].schema, "a.schema");
        assert_eq!(manifest.settings[1].schema, "z.schema");
    }

    #[test]
    fn manifest_remove_returns_true_when_found() {
        let mut manifest = GSettingsManifest::default();
        manifest.settings.push(sample_setting(
            "org.gnome.desktop.interface",
            "color-scheme",
            "'prefer-dark'",
        ));

        assert!(manifest.remove("org.gnome.desktop.interface", "color-scheme"));
        assert!(manifest.settings.is_empty());
    }

    #[test]
    fn manifest_remove_returns_false_when_not_found() {
        let mut manifest = GSettingsManifest::default();
        assert!(!manifest.remove("nonexistent.schema", "key"));
    }

    #[test]
    fn manifest_merged_combines_settings() {
        let mut system = GSettingsManifest::default();
        system
            .settings
            .push(sample_setting("system.schema", "key", "sys-val"));

        let mut user = GSettingsManifest::default();
        user.settings
            .push(sample_setting("user.schema", "key", "user-val"));

        let merged = GSettingsManifest::merged(&system, &user);

        assert_eq!(merged.settings.len(), 2);
        assert!(merged.find("system.schema", "key").is_some());
        assert!(merged.find("user.schema", "key").is_some());
    }

    #[test]
    fn manifest_merged_user_overrides_system() {
        let mut system = GSettingsManifest::default();
        system
            .settings
            .push(sample_setting("shared.schema", "key", "system-value"));

        let mut user = GSettingsManifest::default();
        user.settings
            .push(sample_setting("shared.schema", "key", "user-value"));

        let merged = GSettingsManifest::merged(&system, &user);

        assert_eq!(merged.settings.len(), 1);
        assert_eq!(
            merged.find("shared.schema", "key").unwrap().value,
            "user-value"
        );
    }

    #[test]
    fn manifest_merged_is_sorted() {
        let mut system = GSettingsManifest::default();
        system
            .settings
            .push(sample_setting("z.schema", "key", "val"));

        let mut user = GSettingsManifest::default();
        user.settings.push(sample_setting("a.schema", "key", "val"));

        let merged = GSettingsManifest::merged(&system, &user);

        assert_eq!(merged.settings[0].schema, "a.schema");
        assert_eq!(merged.settings[1].schema, "z.schema");
    }

    #[test]
    fn manifest_serialization_roundtrip() {
        let mut manifest = GSettingsManifest::default();
        manifest.settings.push(sample_setting_with_comment(
            "org.gnome.desktop.interface",
            "color-scheme",
            "'prefer-dark'",
            "Enable dark mode",
        ));

        let json = serde_json::to_string(&manifest).unwrap();
        let parsed: GSettingsManifest = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.settings.len(), 1);
        assert_eq!(
            parsed.settings[0].comment,
            Some("Enable dark mode".to_string())
        );
    }

    #[test]
    fn manifest_load_save_roundtrip() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let path = tmp_dir.path().join("test-gsettings.json");

        let mut manifest = GSettingsManifest::default();
        manifest.upsert(sample_setting(
            "org.gnome.desktop.interface",
            "color-scheme",
            "'prefer-dark'",
        ));

        manifest.save(&path).unwrap();
        let loaded = GSettingsManifest::load(&path).unwrap();

        assert_eq!(loaded.settings.len(), 1);
    }

    #[test]
    fn manifest_load_nonexistent_returns_default() {
        let path = PathBuf::from("/nonexistent/path/gsettings.json");
        let manifest = GSettingsManifest::load(&path).unwrap();
        assert!(manifest.settings.is_empty());
    }
}
