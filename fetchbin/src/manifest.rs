use crate::error::ManifestError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Manifest {
    #[serde(default)]
    pub runtime_versions: RuntimeManifest,
    #[serde(default)]
    pub binaries: HashMap<String, InstalledBinary>,
}

impl Manifest {
    pub fn load(path: &Path) -> Result<Self, ManifestError> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = fs::read_to_string(path)?;
        let manifest = serde_json::from_str(&content)?;
        Ok(manifest)
    }

    pub fn save(&self, path: &Path) -> Result<(), ManifestError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(self)?;
        fs::write(path, content)?;
        Ok(())
    }

    pub fn default_path() -> Option<PathBuf> {
        let base = dirs::data_dir()?;
        Some(base.join("fetchbin").join("manifest.json"))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RuntimeManifest {
    #[serde(default)]
    pub node: NodeRuntimeManifest,
    #[serde(default)]
    pub pnpm: PnpmRuntimeManifest,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NodeRuntimeManifest {
    pub default: Option<String>,
    #[serde(default)]
    pub installed: Vec<String>,
    pub last_updated: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PnpmRuntimeManifest {
    pub version: Option<String>,
    pub last_updated: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledBinary {
    pub source: SourceSpec,
    pub binary: String,
    pub sha256: String,
    pub installed_at: String,
    pub runtime: Option<RuntimeVersionSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum SourceSpec {
    Npm {
        package: String,
        version: String,
    },
    Cargo {
        crate_name: String,
        version: String,
    },
    Github {
        repo: String,
        asset: String,
        version: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum RuntimeVersionSpec {
    Node { version: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_roundtrip() {
        let mut manifest = Manifest::default();
        manifest.binaries.insert(
            "turbo".to_string(),
            InstalledBinary {
                source: SourceSpec::Npm {
                    package: "turbo".to_string(),
                    version: "2.3.4".to_string(),
                },
                binary: "turbo".to_string(),
                sha256: "abc123".to_string(),
                installed_at: "2026-01-27T10:00:00Z".to_string(),
                runtime: Some(RuntimeVersionSpec::Node {
                    version: "22.2.0".to_string(),
                }),
            },
        );

        let json = serde_json::to_string(&manifest).expect("serialize");
        let restored: Manifest = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(restored.binaries.len(), 1);
        assert!(restored.binaries.contains_key("turbo"));
    }
}
