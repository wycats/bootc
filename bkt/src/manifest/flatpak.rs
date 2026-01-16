//! Flatpak manifest types.

use anyhow::{Context, Result};
use directories::BaseDirs;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Scope for Flatpak apps and remotes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum FlatpakScope {
    #[default]
    System,
    User,
}

impl std::fmt::Display for FlatpakScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FlatpakScope::System => write!(f, "system"),
            FlatpakScope::User => write!(f, "user"),
        }
    }
}

impl std::str::FromStr for FlatpakScope {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "system" => Ok(FlatpakScope::System),
            "user" => Ok(FlatpakScope::User),
            _ => anyhow::bail!("Invalid scope: {}. Use 'system' or 'user'", s),
        }
    }
}

/// A Flatpak application entry.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FlatpakApp {
    /// Application ID (e.g., "org.gnome.Calculator")
    pub id: String,
    /// Remote name (e.g., "flathub")
    pub remote: String,
    /// Installation scope
    pub scope: FlatpakScope,
    /// Branch (e.g., "stable", "1.2")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// Commit hash
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    /// Overrides (e.g. "--filesystem=home")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overrides: Option<Vec<String>>,
}

/// The flatpak-apps.json manifest.
#[derive(Debug, Clone, Serialize, Deserialize, Default, JsonSchema)]
pub struct FlatpakAppsManifest {
    #[serde(rename = "$schema", skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    pub apps: Vec<FlatpakApp>,
}

impl FlatpakAppsManifest {
    /// System manifest path (baked into image).
    pub const SYSTEM_PATH: &'static str = "/usr/share/bootc-bootstrap/flatpak-apps.json";

    /// Load a manifest from a path.
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read flatpak manifest from {}", path.display()))?;
        let manifest: Self = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse flatpak manifest from {}", path.display()))?;
        Ok(manifest)
    }

    /// Save a manifest to a path.
    pub fn save(&self, path: &PathBuf) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory {}", parent.display()))?;
        }
        let content =
            serde_json::to_string_pretty(self).context("Failed to serialize flatpak manifest")?;
        fs::write(path, content)
            .with_context(|| format!("Failed to write flatpak manifest to {}", path.display()))?;
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
        config_dir.join("bootc").join("flatpak-apps.json")
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

    /// Merge system and user manifests (user overrides system by id).
    pub fn merged(system: &Self, user: &Self) -> Self {
        let mut by_id: HashMap<String, FlatpakApp> = HashMap::new();

        // Add system apps first
        for app in &system.apps {
            by_id.insert(app.id.clone(), app.clone());
        }

        // User apps override
        for app in &user.apps {
            by_id.insert(app.id.clone(), app.clone());
        }

        let mut apps: Vec<FlatpakApp> = by_id.into_values().collect();
        apps.sort_by(|a, b| a.id.cmp(&b.id));

        Self { schema: None, apps }
    }

    /// Find an app by id.
    pub fn find(&self, id: &str) -> Option<&FlatpakApp> {
        self.apps.iter().find(|a| a.id == id)
    }

    /// Add or update an app.
    pub fn upsert(&mut self, app: FlatpakApp) {
        if let Some(existing) = self.apps.iter_mut().find(|a| a.id == app.id) {
            *existing = app;
        } else {
            self.apps.push(app);
        }
        self.apps.sort_by(|a, b| a.id.cmp(&b.id));
    }

    /// Remove an app by id. Returns true if removed.
    pub fn remove(&mut self, id: &str) -> bool {
        let len_before = self.apps.len();
        self.apps.retain(|a| a.id != id);
        self.apps.len() < len_before
    }
}

/// A Flatpak remote entry.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FlatpakRemote {
    /// Remote name
    pub name: String,
    /// Remote URL
    pub url: String,
    /// Installation scope
    pub scope: FlatpakScope,
    /// Whether the remote is filtered (Flathub verified only)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filtered: Option<bool>,
}

/// The flatpak-remotes.json manifest.
#[derive(Debug, Clone, Serialize, Deserialize, Default, JsonSchema)]
pub struct FlatpakRemotesManifest {
    #[serde(rename = "$schema", skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    pub remotes: Vec<FlatpakRemote>,
}

impl FlatpakRemotesManifest {
    /// System manifest path (baked into image).
    #[allow(dead_code)]
    pub const SYSTEM_PATH: &'static str = "/usr/share/bootc-bootstrap/flatpak-remotes.json";

    /// Load a manifest from a path.
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = fs::read_to_string(path).with_context(|| {
            format!(
                "Failed to read flatpak remotes manifest from {}",
                path.display()
            )
        })?;
        let manifest: Self = serde_json::from_str(&content).with_context(|| {
            format!(
                "Failed to parse flatpak remotes manifest from {}",
                path.display()
            )
        })?;
        Ok(manifest)
    }

    /// Load the system manifest.
    #[allow(dead_code)]
    pub fn load_system() -> Result<Self> {
        Self::load(&PathBuf::from(Self::SYSTEM_PATH))
    }

    /// Load from current working directory (for manifest repos).
    pub fn load_cwd() -> Result<Self> {
        Self::load(&PathBuf::from("manifests/flatpak-remotes.json"))
    }

    /// Check if a remote name is managed by this manifest.
    pub fn has_remote(&self, name: &str) -> bool {
        self.remotes.iter().any(|r| r.name == name)
    }

    /// Get all remote names.
    #[allow(dead_code)]
    pub fn remote_names(&self) -> Vec<&str> {
        self.remotes.iter().map(|r| r.name.as_str()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_app(id: &str) -> FlatpakApp {
        FlatpakApp {
            id: id.to_string(),
            remote: "flathub".to_string(),
            scope: FlatpakScope::System,
            branch: None,
            commit: None,
            overrides: None,
        }
    }

    fn sample_app_user(id: &str) -> FlatpakApp {
        FlatpakApp {
            id: id.to_string(),
            remote: "flathub".to_string(),
            scope: FlatpakScope::User,
            branch: None,
            commit: None,
            overrides: None,
        }
    }

    // FlatpakScope tests
    #[test]
    fn scope_display() {
        assert_eq!(FlatpakScope::System.to_string(), "system");
        assert_eq!(FlatpakScope::User.to_string(), "user");
    }

    #[test]
    fn scope_from_str() {
        assert_eq!(
            "system".parse::<FlatpakScope>().unwrap(),
            FlatpakScope::System
        );
        assert_eq!("user".parse::<FlatpakScope>().unwrap(), FlatpakScope::User);
        assert_eq!("USER".parse::<FlatpakScope>().unwrap(), FlatpakScope::User);
        assert!("invalid".parse::<FlatpakScope>().is_err());
    }

    // FlatpakAppsManifest tests
    #[test]
    fn manifest_default_is_empty() {
        let manifest = FlatpakAppsManifest::default();
        assert!(manifest.apps.is_empty());
        assert!(manifest.schema.is_none());
    }

    #[test]
    fn manifest_find_returns_matching_app() {
        let mut manifest = FlatpakAppsManifest::default();
        manifest.apps.push(sample_app("org.gnome.Calculator"));

        let found = manifest.find("org.gnome.Calculator");
        assert!(found.is_some());
        assert_eq!(found.unwrap().id, "org.gnome.Calculator");
    }

    #[test]
    fn manifest_find_returns_none_for_missing() {
        let manifest = FlatpakAppsManifest::default();
        assert!(manifest.find("nonexistent").is_none());
    }

    #[test]
    fn manifest_upsert_adds_new_app() {
        let mut manifest = FlatpakAppsManifest::default();
        manifest.upsert(sample_app("org.gnome.Calculator"));

        assert_eq!(manifest.apps.len(), 1);
        assert_eq!(manifest.apps[0].id, "org.gnome.Calculator");
    }

    #[test]
    fn manifest_upsert_updates_existing_app() {
        let mut manifest = FlatpakAppsManifest::default();
        manifest.upsert(sample_app("org.gnome.Calculator"));
        manifest.upsert(sample_app_user("org.gnome.Calculator"));

        assert_eq!(manifest.apps.len(), 1);
        assert_eq!(manifest.apps[0].scope, FlatpakScope::User);
    }

    #[test]
    fn manifest_upsert_maintains_sorted_order() {
        let mut manifest = FlatpakAppsManifest::default();
        manifest.upsert(sample_app("org.z.App"));
        manifest.upsert(sample_app("org.a.App"));
        manifest.upsert(sample_app("org.m.App"));

        let ids: Vec<_> = manifest.apps.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, vec!["org.a.App", "org.m.App", "org.z.App"]);
    }

    #[test]
    fn manifest_remove_returns_true_when_found() {
        let mut manifest = FlatpakAppsManifest::default();
        manifest.apps.push(sample_app("org.gnome.Calculator"));

        assert!(manifest.remove("org.gnome.Calculator"));
        assert!(manifest.apps.is_empty());
    }

    #[test]
    fn manifest_remove_returns_false_when_not_found() {
        let mut manifest = FlatpakAppsManifest::default();
        assert!(!manifest.remove("nonexistent"));
    }

    #[test]
    fn manifest_merged_combines_system_and_user() {
        let mut system = FlatpakAppsManifest::default();
        system.apps.push(sample_app("org.gnome.Calculator"));

        let mut user = FlatpakAppsManifest::default();
        user.apps.push(sample_app("org.custom.App"));

        let merged = FlatpakAppsManifest::merged(&system, &user);

        assert_eq!(merged.apps.len(), 2);
        assert!(merged.find("org.gnome.Calculator").is_some());
        assert!(merged.find("org.custom.App").is_some());
    }

    #[test]
    fn manifest_merged_user_overrides_system() {
        let mut system = FlatpakAppsManifest::default();
        system.apps.push(sample_app("org.gnome.Calculator"));

        let mut user = FlatpakAppsManifest::default();
        user.apps.push(sample_app_user("org.gnome.Calculator"));

        let merged = FlatpakAppsManifest::merged(&system, &user);

        assert_eq!(merged.apps.len(), 1);
        assert_eq!(
            merged.find("org.gnome.Calculator").unwrap().scope,
            FlatpakScope::User
        );
    }

    #[test]
    fn manifest_serialization_roundtrip() {
        let mut manifest = FlatpakAppsManifest::default();
        manifest.apps.push(sample_app("org.gnome.Calculator"));
        manifest.apps.push(sample_app_user("org.custom.App"));

        let json = serde_json::to_string(&manifest).unwrap();
        let parsed: FlatpakAppsManifest = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.apps.len(), 2);
    }

    #[test]
    fn manifest_load_save_roundtrip() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let path = tmp_dir.path().join("test-flatpak.json");

        let mut manifest = FlatpakAppsManifest::default();
        manifest.apps.push(sample_app("org.gnome.Calculator"));

        manifest.save(&path).unwrap();
        let loaded = FlatpakAppsManifest::load(&path).unwrap();

        assert_eq!(loaded.apps.len(), 1);
        assert!(loaded.find("org.gnome.Calculator").is_some());
    }

    #[test]
    fn manifest_load_nonexistent_returns_default() {
        let path = PathBuf::from("/nonexistent/path/flatpak.json");
        let manifest = FlatpakAppsManifest::load(&path).unwrap();
        assert!(manifest.apps.is_empty());
    }
}
