//! System packages (DNF/RPM) manifest types.

use anyhow::{Context, Result};
use directories::BaseDirs;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// A COPR repository entry.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct CoprRepo {
    /// COPR repository name (e.g., "atim/starship")
    pub name: String,
    /// Whether the repository is enabled
    pub enabled: bool,
    /// Whether to verify GPG signatures (default: true)
    #[serde(default = "default_true")]
    pub gpg_check: bool,
}

fn default_true() -> bool {
    true
}

impl CoprRepo {
    pub fn new(name: String) -> Self {
        Self {
            name,
            enabled: true,
            gpg_check: true,
        }
    }
}

/// The system-packages.json manifest.
///
/// Tracks RPM packages installed via rpm-ostree (host) or dnf (toolbox).
#[derive(Debug, Clone, Serialize, Deserialize, Default, JsonSchema)]
pub struct SystemPackagesManifest {
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

impl SystemPackagesManifest {
    /// System manifest path (baked into image).
    pub const SYSTEM_PATH: &'static str = "/usr/share/bootc-bootstrap/system-packages.json";

    /// Load a manifest from a path.
    pub fn load(path: &PathBuf) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = fs::read_to_string(path).with_context(|| {
            format!(
                "Failed to read system packages manifest from {}",
                path.display()
            )
        })?;
        let manifest: Self = serde_json::from_str(&content).with_context(|| {
            format!(
                "Failed to parse system packages manifest from {}",
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
            .context("Failed to serialize system packages manifest")?;
        fs::write(path, content).with_context(|| {
            format!(
                "Failed to write system packages manifest to {}",
                path.display()
            )
        })?;
        Ok(())
    }

    /// Get the user manifest path.
    ///
    /// Respects `$HOME` environment variable for test isolation.
    pub fn user_path() -> PathBuf {
        let config_dir = std::env::var("HOME")
            .ok()
            .map(|h| PathBuf::from(h).join(".config"))
            .or_else(|| BaseDirs::new().map(|d| d.config_dir().to_path_buf()))
            .unwrap_or_else(|| PathBuf::from(".config"));
        config_dir.join("bootc").join("system-packages.json")
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

    /// Merge system and user manifests.
    pub fn merged(system: &Self, user: &Self) -> Self {
        let mut packages: Vec<String> = system.packages.clone();
        packages.extend(user.packages.clone());
        packages.sort();
        packages.dedup();

        let mut groups: Vec<String> = system.groups.clone();
        groups.extend(user.groups.clone());
        groups.sort();
        groups.dedup();

        let mut excluded: Vec<String> = system.excluded.clone();
        excluded.extend(user.excluded.clone());
        excluded.sort();
        excluded.dedup();

        let mut copr_repos: Vec<CoprRepo> = system.copr_repos.clone();
        for user_copr in &user.copr_repos {
            if !copr_repos.iter().any(|c| c.name == user_copr.name) {
                copr_repos.push(user_copr.clone());
            }
        }
        copr_repos.sort_by(|a, b| a.name.cmp(&b.name));

        Self {
            schema: None,
            packages,
            groups,
            excluded,
            copr_repos,
        }
    }

    /// Find a package by name.
    pub fn find_package(&self, name: &str) -> bool {
        self.packages.iter().any(|p| p == name)
    }

    /// Add a package to the manifest.
    /// Returns true if the package was added, false if it already existed.
    pub fn add_package(&mut self, pkg: String) -> bool {
        if !self.packages.contains(&pkg) {
            self.packages.push(pkg);
            self.packages.sort();
            true
        } else {
            false
        }
    }

    /// Remove a package from the manifest.
    pub fn remove_package(&mut self, pkg: &str) -> bool {
        let len = self.packages.len();
        self.packages.retain(|p| p != pkg);
        self.packages.len() < len
    }

    /// Find a COPR repo by name.
    pub fn find_copr(&self, name: &str) -> Option<&CoprRepo> {
        self.copr_repos.iter().find(|c| c.name == name)
    }

    /// Add or update a COPR repo.
    pub fn upsert_copr(&mut self, copr: CoprRepo) {
        if let Some(existing) = self.copr_repos.iter_mut().find(|c| c.name == copr.name) {
            *existing = copr;
        } else {
            self.copr_repos.push(copr);
        }
        self.copr_repos.sort_by(|a, b| a.name.cmp(&b.name));
    }

    /// Remove a COPR repo.
    pub fn remove_copr(&mut self, name: &str) -> bool {
        let len = self.copr_repos.len();
        self.copr_repos.retain(|c| c.name != name);
        self.copr_repos.len() < len
    }

    /// Check if manifest is empty.
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.packages.is_empty() && self.groups.is_empty() && self.copr_repos.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn manifest_default_is_empty() {
        let manifest = SystemPackagesManifest::default();
        assert!(manifest.is_empty());
        assert!(manifest.packages.is_empty());
        assert!(manifest.groups.is_empty());
        assert!(manifest.copr_repos.is_empty());
    }

    #[test]
    fn manifest_add_package() {
        let mut manifest = SystemPackagesManifest::default();
        manifest.add_package("htop".to_string());
        assert!(manifest.find_package("htop"));
        assert!(!manifest.find_package("neovim"));
    }

    #[test]
    fn manifest_add_package_maintains_sorted_order() {
        let mut manifest = SystemPackagesManifest::default();
        manifest.add_package("zsh".to_string());
        manifest.add_package("htop".to_string());
        manifest.add_package("neovim".to_string());
        assert_eq!(manifest.packages, vec!["htop", "neovim", "zsh"]);
    }

    #[test]
    fn manifest_remove_package() {
        let mut manifest = SystemPackagesManifest::default();
        manifest.add_package("htop".to_string());
        assert!(manifest.remove_package("htop"));
        assert!(!manifest.find_package("htop"));
        assert!(!manifest.remove_package("htop")); // Already removed
    }

    #[test]
    fn manifest_copr_operations() {
        let mut manifest = SystemPackagesManifest::default();
        let copr = CoprRepo::new("atim/starship".to_string());
        manifest.upsert_copr(copr);
        assert!(manifest.find_copr("atim/starship").is_some());
        assert!(manifest.remove_copr("atim/starship"));
        assert!(manifest.find_copr("atim/starship").is_none());
    }

    #[test]
    fn manifest_merged_combines_packages() {
        let mut system = SystemPackagesManifest::default();
        system.add_package("htop".to_string());

        let mut user = SystemPackagesManifest::default();
        user.add_package("neovim".to_string());

        let merged = SystemPackagesManifest::merged(&system, &user);
        assert_eq!(merged.packages, vec!["htop", "neovim"]);
    }

    #[test]
    fn manifest_merged_deduplicates() {
        let mut system = SystemPackagesManifest::default();
        system.add_package("htop".to_string());

        let mut user = SystemPackagesManifest::default();
        user.add_package("htop".to_string());
        user.add_package("neovim".to_string());

        let merged = SystemPackagesManifest::merged(&system, &user);
        assert_eq!(merged.packages, vec!["htop", "neovim"]);
    }

    #[test]
    fn manifest_load_nonexistent_returns_default() {
        let result = SystemPackagesManifest::load(&PathBuf::from("/nonexistent/path.json"));
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn manifest_load_save_roundtrip() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("test-packages.json");

        let mut manifest = SystemPackagesManifest::default();
        manifest.add_package("htop".to_string());
        manifest.add_package("neovim".to_string());
        manifest.upsert_copr(CoprRepo::new("atim/starship".to_string()));

        manifest.save(&path).unwrap();
        let loaded = SystemPackagesManifest::load(&path).unwrap();

        assert_eq!(loaded.packages, manifest.packages);
        assert_eq!(loaded.copr_repos.len(), 1);
        assert_eq!(loaded.copr_repos[0].name, "atim/starship");
    }

    #[test]
    fn manifest_serialization_roundtrip() {
        let mut manifest = SystemPackagesManifest::default();
        manifest.add_package("htop".to_string());
        manifest.groups.push("@development-tools".to_string());
        manifest.upsert_copr(CoprRepo::new("atim/starship".to_string()));

        let json = serde_json::to_string(&manifest).unwrap();
        let parsed: SystemPackagesManifest = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.packages, manifest.packages);
        assert_eq!(parsed.groups, manifest.groups);
        assert_eq!(parsed.copr_repos.len(), 1);
    }
}
