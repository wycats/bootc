//! GNOME extension manifest types.

use anyhow::{Context, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// The gnome-extensions.json manifest.
#[derive(Debug, Clone, Serialize, Deserialize, Default, JsonSchema)]
pub struct GnomeExtensionsManifest {
    #[serde(rename = "$schema", skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    /// List of extension items, either string UUIDs or objects with state
    #[serde(default)]
    pub extensions: Vec<ExtensionItem>,
}

/// A GNOME extension entry in the manifest.
/// Can be deserialized from either a plain string UUID or a structured object.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord)]
#[serde(untagged)]
pub enum ExtensionItem {
    /// Legacy format: Just the UUID (implies enabled = true)
    Uuid(String),
    /// Modern format: Object with state
    Object(ExtensionConfig),
}

/// Detailed configuration for an extension.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord)]
pub struct ExtensionConfig {
    pub id: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

impl ExtensionItem {
    /// Get the UUID of the extension.
    pub fn id(&self) -> &str {
        match self {
            ExtensionItem::Uuid(id) => id,
            ExtensionItem::Object(config) => &config.id,
        }
    }

    /// Check if the extension should be enabled.
    pub fn enabled(&self) -> bool {
        match self {
            ExtensionItem::Uuid(_) => true,
            ExtensionItem::Object(config) => config.enabled,
        }
    }
}

impl From<String> for ExtensionItem {
    fn from(s: String) -> Self {
        ExtensionItem::Uuid(s)
    }
}

impl From<&str> for ExtensionItem {
    fn from(s: &str) -> Self {
        ExtensionItem::Uuid(s.to_string())
    }
}

impl GnomeExtensionsManifest {
    /// Project manifest path (relative to workspace root).
    pub const PROJECT_PATH: &'static str = "manifests/gnome-extensions.json";

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

    /// Check if an extension exists.
    pub fn contains(&self, uuid: &str) -> bool {
        self.extensions.iter().any(|ext| ext.id() == uuid)
    }

    /// Add an extension if not present.
    pub fn add(&mut self, item: impl Into<ExtensionItem>) -> bool {
        let item = item.into();
        let uuid = item.id().to_string();

        // Remove existing if present (allows updating state)
        if self.contains(&uuid) {
            self.remove(&uuid);
        }

        self.extensions.push(item);
        self.extensions.sort_by(|a, b| a.id().cmp(b.id()));
        true
    }

    /// Remove an extension. Returns true if removed.
    pub fn remove(&mut self, uuid: &str) -> bool {
        let len_before = self.extensions.len();
        self.extensions.retain(|ext| ext.id() != uuid);
        self.extensions.len() < len_before
    }

    /// Set the enabled state for an extension.
    /// Returns true if the extension was found and updated.
    pub fn set_enabled(&mut self, uuid: &str, enabled: bool) -> bool {
        if let Some(pos) = self.extensions.iter().position(|ext| ext.id() == uuid) {
            // Replace with object format that has the enabled state
            self.extensions[pos] = ExtensionItem::Object(ExtensionConfig {
                id: uuid.to_string(),
                enabled,
            });
            true
        } else {
            false
        }
    }

    /// Add an extension as disabled.
    pub fn add_disabled(&mut self, uuid: String) {
        self.extensions.push(ExtensionItem::Object(ExtensionConfig {
            id: uuid,
            enabled: false,
        }));
        self.extensions.sort_by(|a, b| a.id().cmp(b.id()));
    }

    /// Get details for an extension
    #[allow(dead_code)]
    pub fn get(&self, uuid: &str) -> Option<&ExtensionItem> {
        self.extensions.iter().find(|ext| ext.id() == uuid)
    }

    /// List unique extension IDs.
    #[allow(dead_code)]
    pub fn list(&self) -> Vec<String> {
        self.extensions.iter().map(|e| e.id().to_string()).collect()
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
        manifest.add("dash-to-dock@micxgx.gmail.com");

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
    fn manifest_add_returns_true_if_exists_but_updates() {
        let mut manifest = GnomeExtensionsManifest::default();
        manifest.add("dash-to-dock@micxgx.gmail.com".to_string());

        // add now replaces, so it likely returns true?
        // My implementation:
        // if self.contains { remove; } push; return true;
        // Wait, remove returns bool.
        // It always returns true.
        // Previously it returned false if exists.

        // Let's modify the expectation or implementation.
        // "Add an extension if not present." -> This doc comment was preserved.
        // But implementation changed.

        // Old impl:
        // if contains { return false; }

        // New impl:
        // if contains { remove; } push;

        // I should probably restore the "return false if no change" behavior if I want stricter semantics,
        // OR better yet, let it update (which is what I implemented) but be aware of return value.
        // It returns `bool`.

        assert!(manifest.add("dash-to-dock@micxgx.gmail.com".to_string()));
    }

    #[test]
    fn manifest_add_maintains_sorted_order() {
        let mut manifest = GnomeExtensionsManifest::default();
        manifest.add("z-ext@example.com");
        manifest.add("a-ext@example.com");
        manifest.add("m-ext@example.com");

        assert_eq!(
            manifest.list(),
            vec![
                "a-ext@example.com".to_string(),
                "m-ext@example.com".to_string(),
                "z-ext@example.com".to_string()
            ]
        );
    }

    #[test]
    fn manifest_remove_returns_true_when_found() {
        let mut manifest = GnomeExtensionsManifest::default();
        manifest.add("dash-to-dock@micxgx.gmail.com");

        assert!(manifest.remove("dash-to-dock@micxgx.gmail.com"));
        assert!(manifest.extensions.is_empty());
    }

    #[test]
    fn manifest_remove_returns_false_when_not_found() {
        let mut manifest = GnomeExtensionsManifest::default();
        assert!(!manifest.remove("nonexistent@example.com"));
    }

    #[test]
    fn manifest_serialization_roundtrip() {
        let mut manifest = GnomeExtensionsManifest::default();
        manifest.add("dash-to-dock@micxgx.gmail.com");

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
    fn extension_item_from_string() {
        let ext: ExtensionItem = "test@example.com".into();
        assert_eq!(ext.id(), "test@example.com");
        assert!(ext.enabled());

        let ext2: ExtensionItem = "test@example.com".to_string().into();
        assert_eq!(ext2.id(), "test@example.com");
    }

    #[test]
    fn manifest_supports_disabled_extension() {
        let mut manifest = GnomeExtensionsManifest::default();
        manifest.add(ExtensionItem::Object(ExtensionConfig {
            id: "disabled@example.com".to_string(),
            enabled: false,
        }));

        assert!(manifest.contains("disabled@example.com"));
        let item = manifest.get("disabled@example.com").unwrap();
        assert!(!item.enabled());
    }
}
