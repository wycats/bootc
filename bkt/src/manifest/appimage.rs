//! AppImage manifest types for GearLever integration.
//!
//! This module provides types for managing AppImages through GearLever.
//! The manifest format is simplified and backend-agnostic, with conversion
//! to GearLever's native format happening at sync time.

use anyhow::{Context, Result};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

// ============================================================================
// Our Simplified Manifest Format (appimage-apps.json)
// ============================================================================

/// An AppImage entry in our simplified, human-friendly format.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AppImageApp {
    /// Human-readable app name (e.g., "OrcaSlicer").
    pub name: String,
    /// GitHub repository in "owner/repo" format.
    pub repo: String,
    /// Asset filename pattern (glob supported, e.g., "*.AppImage").
    pub asset: String,
    /// Whether to include prereleases/nightlies.
    #[serde(default)]
    pub prereleases: bool,
    /// Whether this app is disabled (won't sync).
    #[serde(default, skip_serializing_if = "is_false")]
    pub disabled: bool,
}

#[allow(dead_code)]
fn is_false(b: &bool) -> bool {
    !*b
}

impl AppImageApp {
    /// Create a new AppImage entry.
    #[allow(dead_code)]
    pub fn new(name: impl Into<String>, repo: impl Into<String>, asset: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            repo: repo.into(),
            asset: asset.into(),
            prereleases: false,
            disabled: false,
        }
    }

    /// Enable prereleases for this app.
    #[allow(dead_code)]
    pub fn with_prereleases(mut self) -> Self {
        self.prereleases = true;
        self
    }

    /// Generate base64 key for GearLever's format.
    pub fn b64_key(&self) -> String {
        BASE64.encode(&self.name)
    }

    /// Convert to GearLever's native format.
    pub fn to_gearlever_entry(&self) -> GearLeverNativeEntry {
        GearLeverNativeEntry {
            b64name: self.b64_key(),
            name: self.name.clone(),
            update_url: format!(
                "https://github.com/{}/releases/download/*/{}",
                self.repo, self.asset
            ),
            update_url_manager: "GithubUpdater".to_string(),
            update_manager_config: GearLeverUpdateConfig {
                allow_prereleases: self.prereleases,
                repo_url: format!("https://github.com/{}", self.repo),
                repo_filename: self.asset.clone(),
            },
        }
    }
}

/// The appimage-apps.json manifest.
#[derive(Debug, Clone, Serialize, Deserialize, Default, JsonSchema)]
pub struct AppImageAppsManifest {
    #[serde(rename = "$schema", skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    pub apps: Vec<AppImageApp>,
}

impl AppImageAppsManifest {
    /// Manifest filename.
    pub const FILENAME: &'static str = "appimage-apps.json";

    /// Load a manifest from a path.
    pub fn load(path: &PathBuf) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read appimage manifest from {}", path.display()))?;
        let manifest: Self = serde_json::from_str(&content).with_context(|| {
            format!("Failed to parse appimage manifest from {}", path.display())
        })?;
        Ok(manifest)
    }

    /// Load from a manifests directory.
    pub fn load_from_dir(manifests_dir: &std::path::Path) -> Result<Self> {
        Self::load(&manifests_dir.join(Self::FILENAME))
    }

    /// Save the manifest to a path.
    pub fn save(&self, path: &PathBuf) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(self)?;
        fs::write(path, content + "\n")?;
        Ok(())
    }

    /// Save to a manifests directory.
    pub fn save_to_dir(&self, manifests_dir: &std::path::Path) -> Result<()> {
        self.save(&manifests_dir.join(Self::FILENAME))
    }

    /// Find an app by name.
    pub fn find(&self, name: &str) -> Option<&AppImageApp> {
        self.apps.iter().find(|a| a.name == name)
    }

    /// Find an app by name (mutable).
    pub fn find_mut(&mut self, name: &str) -> Option<&mut AppImageApp> {
        self.apps.iter_mut().find(|a| a.name == name)
    }

    /// Add or update an app. Returns true if it was an update.
    pub fn upsert(&mut self, app: AppImageApp) -> bool {
        if let Some(existing) = self.apps.iter_mut().find(|a| a.name == app.name) {
            *existing = app;
            true
        } else {
            self.apps.push(app);
            self.apps.sort_by(|a, b| a.name.cmp(&b.name));
            false
        }
    }

    /// Remove an app by name. Returns true if removed.
    pub fn remove(&mut self, name: &str) -> bool {
        let len_before = self.apps.len();
        self.apps.retain(|a| a.name != name);
        self.apps.len() < len_before
    }

    /// Get all enabled (non-disabled) apps.
    pub fn enabled_apps(&self) -> impl Iterator<Item = &AppImageApp> {
        self.apps.iter().filter(|a| !a.disabled)
    }
}

// ============================================================================
// GearLever's Native Format
// ============================================================================

/// GearLever's update configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GearLeverUpdateConfig {
    /// Whether to allow prereleases.
    pub allow_prereleases: bool,
    /// GitHub repo URL.
    pub repo_url: String,
    /// Filename pattern for the release asset.
    pub repo_filename: String,
}

/// A single entry in GearLever's apps.json.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GearLeverNativeEntry {
    /// Base64-encoded app name (also used as key).
    pub b64name: String,
    /// Human-readable name.
    pub name: String,
    /// Update URL pattern with wildcards.
    pub update_url: String,
    /// Update manager type (usually "GithubUpdater").
    pub update_url_manager: String,
    /// Update configuration.
    pub update_manager_config: GearLeverUpdateConfig,
}

impl GearLeverNativeEntry {
    /// Convert from GearLever's native format to our simplified format.
    pub fn to_appimage_app(&self) -> Option<AppImageApp> {
        // Extract owner/repo from repo_url like "https://github.com/owner/repo"
        let repo = self
            .update_manager_config
            .repo_url
            .strip_prefix("https://github.com/")?
            .to_string();

        Some(AppImageApp {
            name: self.name.clone(),
            repo,
            asset: self.update_manager_config.repo_filename.clone(),
            prereleases: self.update_manager_config.allow_prereleases,
            disabled: false,
        })
    }
}

/// GearLever's native apps.json format.
/// Keys are base64-encoded app names.
#[derive(Debug, Clone, Default)]
pub struct GearLeverNativeManifest {
    /// Map of base64-key -> entry.
    pub entries: HashMap<String, GearLeverNativeEntry>,
}

impl GearLeverNativeManifest {
    /// Path to GearLever's config directory.
    pub fn config_dir() -> Option<PathBuf> {
        let home = std::env::var("HOME").ok()?;
        let path = PathBuf::from(home).join(".var/app/it.mijorus.gearlever/config");
        if path.exists() { Some(path) } else { None }
    }

    /// Path to GearLever's apps.json.
    pub fn apps_json_path() -> Option<PathBuf> {
        Self::config_dir().map(|p| p.join("apps.json"))
    }

    /// Check if GearLever is available (config dir exists).
    #[allow(dead_code)]
    pub fn is_available() -> bool {
        Self::config_dir().is_some()
    }

    /// Load from GearLever's apps.json.
    pub fn load() -> Result<Self> {
        let path = Self::apps_json_path().ok_or_else(|| {
            anyhow::anyhow!(
                "GearLever not found. Install it first:\n\n  \
                 flatpak install flathub it.mijorus.gearlever\n\n\
                 Then run GearLever once to initialize its config directory."
            )
        })?;

        if !path.exists() {
            return Ok(Self::default());
        }

        let content = fs::read_to_string(&path).with_context(|| {
            format!("Failed to read GearLever apps.json from {}", path.display())
        })?;

        let entries: HashMap<String, GearLeverNativeEntry> = serde_json::from_str(&content)
            .with_context(|| {
                format!(
                    "Failed to parse GearLever apps.json from {}",
                    path.display()
                )
            })?;

        Ok(Self { entries })
    }

    /// Save to GearLever's apps.json.
    pub fn save(&self) -> Result<()> {
        let path = Self::apps_json_path()
            .ok_or_else(|| anyhow::anyhow!("GearLever config directory not found"))?;

        let content = serde_json::to_string_pretty(&self.entries)?;
        fs::write(&path, content + "\n")?;
        Ok(())
    }

    /// Get an entry by name (not base64 key).
    pub fn find_by_name(&self, name: &str) -> Option<&GearLeverNativeEntry> {
        self.entries.values().find(|e| e.name == name)
    }

    /// Upsert an entry from our format.
    pub fn upsert(&mut self, app: &AppImageApp) {
        let entry = app.to_gearlever_entry();
        self.entries.insert(entry.b64name.clone(), entry);
    }

    /// Remove an entry by name.
    pub fn remove_by_name(&mut self, name: &str) -> bool {
        let key = self
            .entries
            .iter()
            .find(|(_, e)| e.name == name)
            .map(|(k, _)| k.clone());

        if let Some(key) = key {
            self.entries.remove(&key);
            true
        } else {
            false
        }
    }

    /// Retain only entries matching a predicate on the name.
    #[allow(dead_code)]
    pub fn retain_by_name<F>(&mut self, mut f: F)
    where
        F: FnMut(&str) -> bool,
    {
        self.entries.retain(|_, e| f(&e.name));
    }

    /// Get all entries as AppImageApp.
    pub fn to_appimage_apps(&self) -> Vec<AppImageApp> {
        self.entries
            .values()
            .filter_map(|e| e.to_appimage_app())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_b64_key_generation() {
        let app = AppImageApp::new("OrcaSlicer", "OrcaSlicer/OrcaSlicer", "test.AppImage");
        assert_eq!(app.b64_key(), "T3JjYVNsaWNlcg==");
    }

    #[test]
    fn test_to_gearlever_entry() {
        let app = AppImageApp::new("OrcaSlicer", "OrcaSlicer/OrcaSlicer", "test.AppImage")
            .with_prereleases();

        let entry = app.to_gearlever_entry();
        assert_eq!(entry.name, "OrcaSlicer");
        assert_eq!(entry.b64name, "T3JjYVNsaWNlcg==");
        assert_eq!(
            entry.update_url,
            "https://github.com/OrcaSlicer/OrcaSlicer/releases/download/*/test.AppImage"
        );
        assert_eq!(entry.update_url_manager, "GithubUpdater");
        assert!(entry.update_manager_config.allow_prereleases);
        assert_eq!(
            entry.update_manager_config.repo_url,
            "https://github.com/OrcaSlicer/OrcaSlicer"
        );
    }

    #[test]
    fn test_manifest_upsert() {
        let mut manifest = AppImageAppsManifest::default();
        let app1 = AppImageApp::new("App1", "owner/app1", "app1.AppImage");
        let app2 = AppImageApp::new("App2", "owner/app2", "app2.AppImage");

        assert!(!manifest.upsert(app1.clone()));
        assert!(!manifest.upsert(app2.clone()));
        assert_eq!(manifest.apps.len(), 2);

        // Upsert existing should return true
        let app1_updated = AppImageApp::new("App1", "owner/app1", "new.AppImage");
        assert!(manifest.upsert(app1_updated));
        assert_eq!(manifest.apps.len(), 2);
        assert_eq!(manifest.find("App1").unwrap().asset, "new.AppImage");
    }

    #[test]
    fn test_manifest_remove() {
        let mut manifest = AppImageAppsManifest::default();
        manifest.upsert(AppImageApp::new("App1", "owner/app1", "app1.AppImage"));
        manifest.upsert(AppImageApp::new("App2", "owner/app2", "app2.AppImage"));

        assert!(manifest.remove("App1"));
        assert!(!manifest.remove("App1")); // Already removed
        assert_eq!(manifest.apps.len(), 1);
        assert!(manifest.find("App2").is_some());
    }

    #[test]
    fn test_enabled_apps_filter() {
        let mut manifest = AppImageAppsManifest::default();
        manifest.upsert(AppImageApp::new("App1", "owner/app1", "app1.AppImage"));
        manifest.upsert(AppImageApp {
            name: "App2".to_string(),
            repo: "owner/app2".to_string(),
            asset: "app2.AppImage".to_string(),
            prereleases: false,
            disabled: true,
        });

        let enabled: Vec<_> = manifest.enabled_apps().collect();
        assert_eq!(enabled.len(), 1);
        assert_eq!(enabled[0].name, "App1");
    }

    #[test]
    fn test_gearlever_roundtrip() {
        let app = AppImageApp::new("TestApp", "owner/repo", "test.AppImage").with_prereleases();

        let entry = app.to_gearlever_entry();
        let recovered = entry.to_appimage_app().unwrap();

        assert_eq!(recovered.name, app.name);
        assert_eq!(recovered.repo, app.repo);
        assert_eq!(recovered.asset, app.asset);
        assert_eq!(recovered.prereleases, app.prereleases);
    }
}
