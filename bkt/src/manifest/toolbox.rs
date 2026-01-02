//! Toolbox packages manifest types.
//!
//! Unlike system-packages.json which tracks host RPM packages,
//! toolbox-packages.json tracks packages installed in the development
//! toolbox container.

use super::dnf::{CoprRepo, SystemPackagesManifest};
use anyhow::{Context, Result};
use directories::BaseDirs;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// The toolbox-packages.json manifest.
///
/// Tracks DNF packages installed in the development toolbox.
/// Uses the same core fields as SystemPackagesManifest but with
/// a different storage location and additional toolbox-specific
/// fields (planned for future phases).
#[derive(Debug, Clone, Serialize, Deserialize, Default, JsonSchema)]
pub struct ToolboxPackagesManifest {
    #[serde(rename = "$schema", skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,

    /// Individual packages to install
    #[serde(default)]
    pub packages: Vec<String>,

    /// Package groups (e.g., "@development-tools")
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<String>,

    /// Packages to exclude from groups
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub excluded: Vec<String>,

    /// COPR repositories
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub copr_repos: Vec<CoprRepo>,
}

#[allow(dead_code)]
impl ToolboxPackagesManifest {
    /// Project manifest path (relative to workspace root).
    pub const PROJECT_PATH: &'static str = "manifests/toolbox-packages.json";

    /// Load a manifest from a path.
    pub fn load(path: &PathBuf) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = fs::read_to_string(path).with_context(|| {
            format!(
                "Failed to read toolbox packages manifest from {}",
                path.display()
            )
        })?;
        let manifest: Self = serde_json::from_str(&content).with_context(|| {
            format!(
                "Failed to parse toolbox packages manifest from {}",
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
            .context("Failed to serialize toolbox packages manifest")?;
        fs::write(path, content).with_context(|| {
            format!(
                "Failed to write toolbox packages manifest to {}",
                path.display()
            )
        })?;
        Ok(())
    }

    /// Get the user manifest path.
    ///
    /// Toolbox manifest is stored in user config only (no system manifest).
    /// Respects `$HOME` environment variable for test isolation.
    pub fn user_path() -> PathBuf {
        let config_dir = std::env::var("HOME")
            .ok()
            .map(|h| PathBuf::from(h).join(".config"))
            .or_else(|| BaseDirs::new().map(|d| d.config_dir().to_path_buf()))
            .unwrap_or_else(|| PathBuf::from(".config"));
        config_dir.join("bootc").join("toolbox-packages.json")
    }

    /// Load the user manifest.
    pub fn load_user() -> Result<Self> {
        Self::load(&Self::user_path())
    }

    /// Save the user manifest.
    pub fn save_user(&self) -> Result<()> {
        self.save(&Self::user_path())
    }

    /// Convert to SystemPackagesManifest for shared DNF operations.
    ///
    /// This allows reusing DNF command logic that operates on SystemPackagesManifest.
    pub fn as_system_manifest(&self) -> SystemPackagesManifest {
        SystemPackagesManifest {
            schema: None,
            packages: self.packages.clone(),
            groups: self.groups.clone(),
            excluded: self.excluded.clone(),
            copr_repos: self.copr_repos.clone(),
        }
    }

    /// Update from a SystemPackagesManifest after DNF operations.
    ///
    /// This syncs back the core fields after DNF command handlers modify them.
    pub fn update_from(&mut self, manifest: &SystemPackagesManifest) {
        self.packages = manifest.packages.clone();
        self.groups = manifest.groups.clone();
        self.excluded = manifest.excluded.clone();
        self.copr_repos = manifest.copr_repos.clone();
    }

    /// Find a package by name.
    pub fn find_package(&self, name: &str) -> bool {
        self.packages.iter().any(|p| p == name)
    }

    /// Add a package to the manifest.
    pub fn add_package(&mut self, pkg: String) {
        if !self.packages.contains(&pkg) {
            self.packages.push(pkg);
            self.packages.sort();
        }
    }

    /// Remove a package from the manifest.
    pub fn remove_package(&mut self, pkg: &str) -> bool {
        let len = self.packages.len();
        self.packages.retain(|p| p != pkg);
        self.packages.len() < len
    }

    /// Check if manifest is empty.
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.packages.is_empty()
            && self.groups.is_empty()
            && self.excluded.is_empty()
            && self.copr_repos.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_default_manifest() {
        let manifest = ToolboxPackagesManifest::default();
        assert!(manifest.packages.is_empty());
        assert!(manifest.groups.is_empty());
        assert!(manifest.copr_repos.is_empty());
    }

    #[test]
    fn test_add_remove_package() {
        let mut manifest = ToolboxPackagesManifest::default();
        manifest.add_package("gcc".to_string());
        assert!(manifest.find_package("gcc"));
        assert!(manifest.remove_package("gcc"));
        assert!(!manifest.find_package("gcc"));
    }

    #[test]
    fn test_as_system_manifest() {
        let mut manifest = ToolboxPackagesManifest::default();
        manifest.add_package("gcc".to_string());
        manifest.add_package("cmake".to_string());

        let system = manifest.as_system_manifest();
        assert!(system.find_package("gcc"));
        assert!(system.find_package("cmake"));
    }

    #[test]
    fn test_update_from_system_manifest() {
        let mut manifest = ToolboxPackagesManifest::default();
        manifest.add_package("old-pkg".to_string());

        let mut system = SystemPackagesManifest::default();
        system.add_package("new-pkg".to_string());

        manifest.update_from(&system);
        assert!(!manifest.find_package("old-pkg"));
        assert!(manifest.find_package("new-pkg"));
    }

    #[test]
    fn test_save_load_round_trip() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("toolbox-packages.json");

        let mut manifest = ToolboxPackagesManifest::default();
        manifest.add_package("ripgrep".to_string());
        manifest.add_package("fd-find".to_string());
        manifest.save(&path).unwrap();

        let loaded = ToolboxPackagesManifest::load(&path).unwrap();
        assert!(loaded.find_package("ripgrep"));
        assert!(loaded.find_package("fd-find"));
    }
}
