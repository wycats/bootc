//! Fetchbin manifest types for host binaries.

use anyhow::{Context, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct HostBinariesManifest {
    #[serde(rename = "$schema", default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    pub binaries: Vec<HostBinary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct HostBinary {
    pub name: String,
    pub source: HostBinarySource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum HostBinarySource {
    Npm {
        package: String,
    },
    Cargo {
        crate_name: String,
    },
    Github {
        repo: String,
        #[serde(default)]
        asset_pattern: Option<String>,
    },
}

impl HostBinariesManifest {
    /// Manifest filename.
    pub const FILENAME: &'static str = "host-binaries.json";

    /// Load a manifest from a path.
    pub fn load(path: &PathBuf) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = fs::read_to_string(path).with_context(|| {
            format!(
                "Failed to read host binaries manifest from {}",
                path.display()
            )
        })?;
        let manifest: Self = serde_json::from_str(&content).with_context(|| {
            format!(
                "Failed to parse host binaries manifest from {}",
                path.display()
            )
        })?;
        Ok(manifest)
    }

    /// Load from a manifests directory.
    pub fn load_from_dir(manifests_dir: &Path) -> Result<Self> {
        Self::load(&manifests_dir.join(Self::FILENAME))
    }

    /// Save the manifest to a path.
    pub fn save(&self, path: &PathBuf) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory {}", parent.display()))?;
        }
        let content = serde_json::to_string_pretty(self)
            .context("Failed to serialize host binaries manifest")?;
        fs::write(path, content + "\n").with_context(|| {
            format!(
                "Failed to write host binaries manifest to {}",
                path.display()
            )
        })?;
        Ok(())
    }

    /// Save to a manifests directory.
    pub fn save_to_dir(&self, manifests_dir: &Path) -> Result<()> {
        self.save(&manifests_dir.join(Self::FILENAME))
    }

    /// Find a binary by name.
    pub fn find(&self, name: &str) -> Option<&HostBinary> {
        self.binaries.iter().find(|b| b.name == name)
    }

    /// Add or update a binary. Returns true if updated.
    pub fn upsert(&mut self, binary: HostBinary) -> bool {
        if let Some(existing) = self.binaries.iter_mut().find(|b| b.name == binary.name) {
            *existing = binary;
            true
        } else {
            self.binaries.push(binary);
            self.binaries.sort_by(|a, b| a.name.cmp(&b.name));
            false
        }
    }

    /// Remove a binary by name. Returns true if removed.
    pub fn remove(&mut self, name: &str) -> bool {
        let len_before = self.binaries.len();
        self.binaries.retain(|b| b.name != name);
        self.binaries.len() < len_before
    }
}
