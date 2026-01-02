//! Base image assumptions manifest type.
//!
//! This module provides types for tracking what the base image (Bazzite) provides.
//! By tracking assumptions, we can:
//! - Detect when the base image no longer provides expected packages
//! - Distinguish between "our additions" and "base image content"
//! - Get early warning of breaking changes in upstream

use anyhow::{Context, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::PathBuf;

use super::find_repo_root;

/// Information about the base container image.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct BaseImageInfo {
    /// The source image reference (e.g., "ghcr.io/ublue-os/bazzite-gnome:stable")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,

    /// The last verified sha256 digest
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_verified_digest: Option<String>,

    /// When the assumptions were last verified against the image
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_verified_at: Option<String>,
}

/// A package assumption - a package expected to be in the base image.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PackageAssumption {
    /// Package name
    pub name: String,

    /// Why this package is assumed (optional documentation)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Base image assumptions manifest.
///
/// Stored at `manifests/base-image-assumptions.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct BaseImageAssumptions {
    /// Information about the base image
    #[serde(default)]
    pub base_image: BaseImageInfo,

    /// Packages assumed to be provided by the base image
    #[serde(default)]
    pub packages: Vec<PackageAssumption>,

    /// Systemd services assumed to be provided
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub services: Vec<String>,

    /// Paths assumed to exist
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,
}

impl BaseImageAssumptions {
    /// Load assumptions from the repository.
    pub fn load_from_repo() -> Result<Self> {
        let cwd = env::current_dir().context("Failed to get current directory")?;
        let repo_root = find_repo_root(&cwd).context("Not in a git repository")?;
        let path = repo_root
            .join("manifests")
            .join("base-image-assumptions.json");
        Self::load(&path)
    }

    /// Load assumptions from a specific path.
    pub fn load(path: &PathBuf) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }

        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let manifest: Self = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse {}", path.display()))?;
        Ok(manifest)
    }

    /// Save assumptions to the repository.
    pub fn save_to_repo(&self) -> Result<()> {
        let cwd = env::current_dir().context("Failed to get current directory")?;
        let repo_root = find_repo_root(&cwd).context("Not in a git repository")?;
        let path = repo_root
            .join("manifests")
            .join("base-image-assumptions.json");
        self.save(&path)
    }

    /// Save assumptions to a specific path.
    pub fn save(&self, path: &PathBuf) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory {}", parent.display()))?;
        }

        let content = serde_json::to_string_pretty(self)
            .context("Failed to serialize base image assumptions")?;
        fs::write(path, content).with_context(|| format!("Failed to write {}", path.display()))?;
        Ok(())
    }

    /// Add a package assumption.
    pub fn add_package(&mut self, name: &str, reason: Option<&str>) {
        // Check if already exists
        if self.packages.iter().any(|p| p.name == name) {
            return;
        }

        self.packages.push(PackageAssumption {
            name: name.to_string(),
            reason: reason.map(String::from),
        });

        // Keep sorted
        self.packages.sort_by(|a, b| a.name.cmp(&b.name));
    }

    /// Remove a package assumption. Returns true if it was removed.
    pub fn remove_package(&mut self, name: &str) -> bool {
        let len_before = self.packages.len();
        self.packages.retain(|p| p.name != name);
        self.packages.len() < len_before
    }

    /// Check if a package is assumed.
    #[allow(dead_code)]
    pub fn has_package(&self, name: &str) -> bool {
        self.packages.iter().any(|p| p.name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_default_is_empty() {
        let assumptions = BaseImageAssumptions::default();
        assert!(assumptions.packages.is_empty());
        assert!(assumptions.services.is_empty());
        assert!(assumptions.paths.is_empty());
    }

    #[test]
    fn test_add_package() {
        let mut assumptions = BaseImageAssumptions::default();
        assumptions.add_package("flatpak", Some("Required for app installation"));

        assert_eq!(assumptions.packages.len(), 1);
        assert_eq!(assumptions.packages[0].name, "flatpak");
        assert_eq!(
            assumptions.packages[0].reason,
            Some("Required for app installation".to_string())
        );
    }

    #[test]
    fn test_add_package_idempotent() {
        let mut assumptions = BaseImageAssumptions::default();
        assumptions.add_package("flatpak", None);
        assumptions.add_package("flatpak", Some("Different reason"));

        assert_eq!(assumptions.packages.len(), 1);
        assert!(assumptions.packages[0].reason.is_none());
    }

    #[test]
    fn test_add_package_sorted() {
        let mut assumptions = BaseImageAssumptions::default();
        assumptions.add_package("zzz-package", None);
        assumptions.add_package("aaa-package", None);
        assumptions.add_package("mmm-package", None);

        let names: Vec<_> = assumptions
            .packages
            .iter()
            .map(|p| p.name.as_str())
            .collect();
        assert_eq!(names, vec!["aaa-package", "mmm-package", "zzz-package"]);
    }

    #[test]
    fn test_remove_package() {
        let mut assumptions = BaseImageAssumptions::default();
        assumptions.add_package("flatpak", None);

        assert!(assumptions.remove_package("flatpak"));
        assert!(assumptions.packages.is_empty());

        // Removing again should return false
        assert!(!assumptions.remove_package("flatpak"));
    }

    #[test]
    fn test_has_package() {
        let mut assumptions = BaseImageAssumptions::default();
        assumptions.add_package("flatpak", None);

        assert!(assumptions.has_package("flatpak"));
        assert!(!assumptions.has_package("nonexistent"));
    }

    #[test]
    fn test_load_save_roundtrip() -> Result<()> {
        let temp = TempDir::new()?;
        let path = temp.path().join("base-image-assumptions.json");

        let mut assumptions = BaseImageAssumptions::default();
        assumptions.base_image.source = Some("ghcr.io/test/image:stable".to_string());
        assumptions.add_package("flatpak", Some("Core functionality"));
        assumptions.add_package("gnome-shell", None);

        assumptions.save(&path)?;

        let loaded = BaseImageAssumptions::load(&path)?;
        assert_eq!(loaded.packages.len(), 2);
        assert_eq!(
            loaded.base_image.source,
            Some("ghcr.io/test/image:stable".to_string())
        );

        Ok(())
    }

    #[test]
    fn test_load_nonexistent_returns_default() {
        let path = PathBuf::from("/nonexistent/path/assumptions.json");
        let result = BaseImageAssumptions::load(&path);
        assert!(result.is_ok());
        assert!(result.unwrap().packages.is_empty());
    }

    #[test]
    fn test_serialization_roundtrip() {
        let mut assumptions = BaseImageAssumptions::default();
        assumptions.base_image.source = Some("ghcr.io/ublue-os/bazzite-gnome:stable".to_string());
        assumptions.base_image.last_verified_digest = Some("sha256:abc123".to_string());
        assumptions.add_package("flatpak", Some("Required"));
        assumptions.services.push("gdm.service".to_string());
        assumptions.paths.push("/usr/bin/flatpak".to_string());

        let json = serde_json::to_string_pretty(&assumptions).unwrap();
        let parsed: BaseImageAssumptions = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.packages.len(), 1);
        assert_eq!(parsed.services.len(), 1);
        assert_eq!(parsed.paths.len(), 1);
    }
}
