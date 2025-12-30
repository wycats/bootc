//! GNOME extension manifest types.

use anyhow::{Context, Result};
use directories::BaseDirs;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

/// The gnome-extensions.json manifest.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
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
            format!(
                "Failed to read extensions manifest from {}",
                path.display()
            )
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
            format!(
                "Failed to write extensions manifest to {}",
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
