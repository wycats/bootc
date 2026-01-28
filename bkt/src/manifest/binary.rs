use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BinaryManifest {
    #[serde(default)]
    pub binaries: Vec<BinaryEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinaryEntry {
    pub name: String,
    pub source: BinarySource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum BinarySource {
    Npm { package: String, version: String },
    Cargo { crate_name: String, version: String },
    Github { repo: String, version: String, asset: Option<String> },
}

impl BinaryManifest {
    /// Manifest filename.
    pub const FILENAME: &'static str = "host-binaries.json";

    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read binary manifest from {}", path.display()))?;
        let manifest: Self = serde_json::from_str(&content).with_context(|| {
            format!("Failed to parse binary manifest from {}", path.display())
        })?;
        Ok(manifest)
    }

    pub fn load_from_dir(manifests_dir: &Path) -> Result<Self> {
        Self::load(&manifests_dir.join(Self::FILENAME))
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory {}", parent.display()))?;
        }
        let content = serde_json::to_string_pretty(self)
            .context("Failed to serialize binary manifest")?;
        fs::write(path, content + "\n")
            .with_context(|| format!("Failed to write binary manifest to {}", path.display()))?;
        Ok(())
    }

    pub fn save_to_dir(&self, manifests_dir: &Path) -> Result<()> {
        self.save(&manifests_dir.join(Self::FILENAME))
    }

    pub fn find(&self, name: &str) -> Option<&BinaryEntry> {
        self.binaries.iter().find(|b| b.name == name)
    }

    pub fn find_mut(&mut self, name: &str) -> Option<&mut BinaryEntry> {
        self.binaries.iter_mut().find(|b| b.name == name)
    }

    pub fn upsert(&mut self, entry: BinaryEntry) -> bool {
        if let Some(existing) = self.binaries.iter_mut().find(|b| b.name == entry.name) {
            *existing = entry;
            true
        } else {
            self.binaries.push(entry);
            self.binaries.sort_by(|a, b| a.name.cmp(&b.name));
            false
        }
    }

    pub fn remove(&mut self, name: &str) -> bool {
        let len_before = self.binaries.len();
        self.binaries.retain(|b| b.name != name);
        self.binaries.len() < len_before
    }
}
