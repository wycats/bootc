//! GNOME extension manifest types.

use anyhow::{Context, Result};
use directories::BaseDirs;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

/// The gnome-extensions.json manifest.
#[derive(Debug, Clone, Serialize, Deserialize, Default, JsonSchema)]
pub struct GnomeExtensionsManifest {
    #[serde(rename = "$schema", skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    /// List of extension UUIDs (e.g., "dash-to-dock@micxgx.gmail.com")
    pub extensions: Vec<String>,
}

impl GnomeExtensionsManifest {
    /// System manifest path (baked into image).
    pub const SYSTEM_PATH: &'static str = "/usr/share/bootc-bootstrap/gnome-extensions.json";

    /// Load a manifest from a path.
    pub fn load(path: &PathBuf) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = fs::read_to_string(path).with_context(|| {
            format!("Failed to read extensions manifest from {}", path.display())
        })?;
        let manifest: Self = serde_json::from_str(&content).with_context(|| {
            format!(
                "Failed to parse extensions manifest from {}",
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
            .context("Failed to serialize extensions manifest")?;
        fs::write(path, content).with_context(|| {
            format!("Failed to write extensions manifest to {}", path.display())
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
        config_dir.join("bootc").join("gnome-extensions.json")
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

    /// Merge system and user manifests (union, sorted).
    pub fn merged(system: &Self, user: &Self) -> Self {
        let mut all: HashSet<String> = HashSet::new();

        for uuid in &system.extensions {
            all.insert(uuid.clone());
        }
        for uuid in &user.extensions {
            all.insert(uuid.clone());
        }

        let mut extensions: Vec<String> = all.into_iter().collect();
        extensions.sort();

        Self {
            schema: None,
            extensions,
        }
    }

    /// Check if an extension exists.
    pub fn contains(&self, uuid: &str) -> bool {
        self.extensions.iter().any(|u| u == uuid)
    }

    /// Add an extension if not present.
    pub fn add(&mut self, uuid: String) -> bool {
        if self.contains(&uuid) {
            return false;
        }
        self.extensions.push(uuid);
        self.extensions.sort();
        true
    }

    /// Remove an extension. Returns true if removed.
    pub fn remove(&mut self, uuid: &str) -> bool {
        let len_before = self.extensions.len();
        self.extensions.retain(|u| u != uuid);
        self.extensions.len() < len_before
    }
}

/// A GNOME Shell extension.
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GnomeExtension {
    /// Extension UUID (e.g., "dash-to-dock@micxgx.gmail.com")
    pub uuid: String,
}

impl From<String> for GnomeExtension {
    fn from(uuid: String) -> Self {
        Self { uuid }
    }
}

impl From<&str> for GnomeExtension {
    fn from(uuid: &str) -> Self {
        Self {
            uuid: uuid.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_default_is_empty() {
        let manifest = GnomeExtensionsManifest::default();
        assert!(manifest.extensions.is_empty());
        assert!(manifest.schema.is_none());
    }

    #[test]
    fn manifest_contains_checks_existence() {
        let mut manifest = GnomeExtensionsManifest::default();
        manifest
            .extensions
            .push("dash-to-dock@micxgx.gmail.com".to_string());

        assert!(manifest.contains("dash-to-dock@micxgx.gmail.com"));
        assert!(!manifest.contains("nonexistent@example.com"));
    }

    #[test]
    fn manifest_add_inserts_new_extension() {
        let mut manifest = GnomeExtensionsManifest::default();

        assert!(manifest.add("dash-to-dock@micxgx.gmail.com".to_string()));
        assert_eq!(manifest.extensions.len(), 1);
    }

    #[test]
    fn manifest_add_returns_false_if_exists() {
        let mut manifest = GnomeExtensionsManifest::default();
        manifest.add("dash-to-dock@micxgx.gmail.com".to_string());

        assert!(!manifest.add("dash-to-dock@micxgx.gmail.com".to_string()));
        assert_eq!(manifest.extensions.len(), 1);
    }

    #[test]
    fn manifest_add_maintains_sorted_order() {
        let mut manifest = GnomeExtensionsManifest::default();
        manifest.add("z-ext@example.com".to_string());
        manifest.add("a-ext@example.com".to_string());
        manifest.add("m-ext@example.com".to_string());

        assert_eq!(
            manifest.extensions,
            vec![
                "a-ext@example.com",
                "m-ext@example.com",
                "z-ext@example.com"
            ]
        );
    }

    #[test]
    fn manifest_remove_returns_true_when_found() {
        let mut manifest = GnomeExtensionsManifest::default();
        manifest
            .extensions
            .push("dash-to-dock@micxgx.gmail.com".to_string());

        assert!(manifest.remove("dash-to-dock@micxgx.gmail.com"));
        assert!(manifest.extensions.is_empty());
    }

    #[test]
    fn manifest_remove_returns_false_when_not_found() {
        let mut manifest = GnomeExtensionsManifest::default();
        assert!(!manifest.remove("nonexistent@example.com"));
    }

    #[test]
    fn manifest_merged_combines_extensions() {
        let mut system = GnomeExtensionsManifest::default();
        system.extensions.push("system-ext@example.com".to_string());

        let mut user = GnomeExtensionsManifest::default();
        user.extensions.push("user-ext@example.com".to_string());

        let merged = GnomeExtensionsManifest::merged(&system, &user);

        assert_eq!(merged.extensions.len(), 2);
        assert!(merged.contains("system-ext@example.com"));
        assert!(merged.contains("user-ext@example.com"));
    }

    #[test]
    fn manifest_merged_deduplicates() {
        let mut system = GnomeExtensionsManifest::default();
        system.extensions.push("shared@example.com".to_string());

        let mut user = GnomeExtensionsManifest::default();
        user.extensions.push("shared@example.com".to_string());

        let merged = GnomeExtensionsManifest::merged(&system, &user);

        assert_eq!(merged.extensions.len(), 1);
    }

    #[test]
    fn manifest_merged_is_sorted() {
        let mut system = GnomeExtensionsManifest::default();
        system.extensions.push("z@example.com".to_string());

        let mut user = GnomeExtensionsManifest::default();
        user.extensions.push("a@example.com".to_string());

        let merged = GnomeExtensionsManifest::merged(&system, &user);

        assert_eq!(merged.extensions, vec!["a@example.com", "z@example.com"]);
    }

    #[test]
    fn manifest_serialization_roundtrip() {
        let mut manifest = GnomeExtensionsManifest::default();
        manifest
            .extensions
            .push("dash-to-dock@micxgx.gmail.com".to_string());

        let json = serde_json::to_string(&manifest).unwrap();
        let parsed: GnomeExtensionsManifest = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.extensions.len(), 1);
        assert!(parsed.contains("dash-to-dock@micxgx.gmail.com"));
    }

    #[test]
    fn manifest_load_save_roundtrip() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let path = tmp_dir.path().join("test-extensions.json");

        let mut manifest = GnomeExtensionsManifest::default();
        manifest.add("dash-to-dock@micxgx.gmail.com".to_string());

        manifest.save(&path).unwrap();
        let loaded = GnomeExtensionsManifest::load(&path).unwrap();

        assert_eq!(loaded.extensions.len(), 1);
    }

    #[test]
    fn manifest_load_nonexistent_returns_default() {
        let path = PathBuf::from("/nonexistent/path/extensions.json");
        let manifest = GnomeExtensionsManifest::load(&path).unwrap();
        assert!(manifest.extensions.is_empty());
    }

    #[test]
    fn gnome_extension_from_string() {
        let ext: GnomeExtension = "test@example.com".into();
        assert_eq!(ext.uuid, "test@example.com");

        let ext2: GnomeExtension = "test@example.com".to_string().into();
        assert_eq!(ext2.uuid, "test@example.com");
    }
}
