//! GSettings manifest types.

use anyhow::{Context, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// A GSettings entry.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, Serialize, Deserialize, Default, JsonSchema)]
pub struct GSettingsManifest {
    #[serde(rename = "$schema", skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    pub settings: Vec<GSetting>,
}

impl GSettingsManifest {
    /// Project manifest path (relative to workspace root).
    pub const PROJECT_PATH: &'static str = "manifests/gsettings.json";

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
