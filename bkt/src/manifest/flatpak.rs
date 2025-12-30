//! Flatpak manifest types.

use anyhow::{Context, Result};
use directories::BaseDirs;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// Scope for Flatpak apps and remotes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlatpakApp {
    /// Application ID (e.g., "org.gnome.Calculator")
    pub id: String,
    /// Remote name (e.g., "flathub")
    pub remote: String,
    /// Installation scope
    pub scope: FlatpakScope,
}

/// The flatpak-apps.json manifest.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FlatpakAppsManifest {
    #[serde(rename = "$schema", skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    pub apps: Vec<FlatpakApp>,
}

impl FlatpakAppsManifest {
    /// System manifest path (baked into image).
    pub const SYSTEM_PATH: &'static str = "/usr/share/bootc-bootstrap/flatpak-apps.json";

    /// Load a manifest from a path.
    pub fn load(path: &PathBuf) -> Result<Self> {
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
    pub fn user_path() -> PathBuf {
        let config_dir = BaseDirs::new()
            .map(|d| d.config_dir().to_path_buf())
            .unwrap_or_else(|| {
                PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config")
            });
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FlatpakRemotesManifest {
    #[serde(rename = "$schema", skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    pub remotes: Vec<FlatpakRemote>,
}
