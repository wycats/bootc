//! Host shim manifest types.

use anyhow::{Context, Result};
use directories::BaseDirs;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// A host shim entry.
///
/// Shims are wrapper scripts that call commands on the host system
/// via flatpak-spawn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Shim {
    /// Name of the shim (command name in toolbox)
    pub name: String,
    /// Name of the command on the host (defaults to name if not specified)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
}

impl Shim {
    /// Get the host command name (defaults to shim name if not specified).
    pub fn host_cmd(&self) -> &str {
        self.host.as_deref().unwrap_or(&self.name)
    }
}

/// The host-shims.json manifest.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ShimsManifest {
    #[serde(rename = "$schema", skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    pub shims: Vec<Shim>,
}

impl ShimsManifest {
    /// System manifest path (baked into image).
    pub const SYSTEM_PATH: &'static str = "/usr/share/bootc-bootstrap/host-shims.json";

    /// Load a manifest from a path.
    pub fn load(path: &PathBuf) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read shims manifest from {}", path.display()))?;
        let manifest: Self = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse shims manifest from {}", path.display()))?;
        Ok(manifest)
    }

    /// Save a manifest to a path.
    pub fn save(&self, path: &PathBuf) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory {}", parent.display()))?;
        }
        let content = serde_json::to_string_pretty(self)
            .context("Failed to serialize shims manifest")?;
        fs::write(path, content)
            .with_context(|| format!("Failed to write shims manifest to {}", path.display()))?;
        Ok(())
    }

    /// Get the user manifest path.
    pub fn user_path() -> PathBuf {
        let config_dir = BaseDirs::new()
            .map(|d| d.config_dir().to_path_buf())
            .unwrap_or_else(|| {
                PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config")
            });
        config_dir.join("bootc").join("host-shims.json")
    }

    /// Get the shims directory path.
    pub fn shims_dir() -> PathBuf {
        let home = BaseDirs::new()
            .map(|d| d.home_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap_or_default()));
        home.join(".local").join("toolbox").join("shims")
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

    /// Merge system and user manifests (user overrides system by name).
    pub fn merged(system: &Self, user: &Self) -> Self {
        let mut by_name: HashMap<String, Shim> = HashMap::new();

        // Add system shims first
        for shim in &system.shims {
            by_name.insert(shim.name.clone(), shim.clone());
        }

        // User shims override
        for shim in &user.shims {
            by_name.insert(shim.name.clone(), shim.clone());
        }

        let mut shims: Vec<Shim> = by_name.into_values().collect();
        shims.sort_by(|a, b| a.name.cmp(&b.name));

        Self { schema: None, shims }
    }

    /// Find a shim by name.
    pub fn find(&self, name: &str) -> Option<&Shim> {
        self.shims.iter().find(|s| s.name == name)
    }

    /// Add or update a shim.
    pub fn upsert(&mut self, shim: Shim) {
        if let Some(existing) = self.shims.iter_mut().find(|s| s.name == shim.name) {
            *existing = shim;
        } else {
            self.shims.push(shim);
        }
        self.shims.sort_by(|a, b| a.name.cmp(&b.name));
    }

    /// Remove a shim by name. Returns true if removed.
    pub fn remove(&mut self, name: &str) -> bool {
        let len_before = self.shims.len();
        self.shims.retain(|s| s.name != name);
        self.shims.len() < len_before
    }
}
