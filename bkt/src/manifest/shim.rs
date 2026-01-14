//! Host shim manifest types.

use crate::component::Resource;
use anyhow::{Context, Result};
use directories::BaseDirs;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// A host shim entry.
///
/// Shims are wrapper scripts that call commands on the host system
/// via flatpak-spawn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
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

impl Resource for Shim {
    type Id = String;

    fn id(&self) -> String {
        self.name.clone()
    }
}

/// The host-shims.json manifest.
#[derive(Debug, Clone, Serialize, Deserialize, Default, JsonSchema)]
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
        let content =
            serde_json::to_string_pretty(self).context("Failed to serialize shims manifest")?;
        fs::write(path, content)
            .with_context(|| format!("Failed to write shims manifest to {}", path.display()))?;
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
        config_dir.join("bootc").join("host-shims.json")
    }

    /// Get the shims directory path.
    ///
    /// Respects `$HOME` environment variable for test isolation.
    pub fn shims_dir() -> PathBuf {
        // Prefer $HOME for test isolation, fall back to BaseDirs
        let home = std::env::var("HOME")
            .ok()
            .map(PathBuf::from)
            .or_else(|| BaseDirs::new().map(|d| d.home_dir().to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."));
        home.join(".local").join("toolbox").join("shims")
    }

    /// Load the system manifest.
    pub fn load_system() -> Result<Self> {
        Self::load(&PathBuf::from(Self::SYSTEM_PATH))
    }

    /// Load from the repository's manifests directory.
    ///
    /// This is used for containerfile generation where we need to read
    /// the manifest from the repo rather than the installed system path.
    pub fn load_repo() -> Result<Self> {
        let repo_path = crate::repo::find_repo_path()?;
        let manifest_path = repo_path.join("manifests").join("host-shims.json");
        Self::load(&manifest_path)
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

        Self {
            schema: None,
            shims,
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_shim(name: &str) -> Shim {
        Shim {
            name: name.to_string(),
            host: None,
        }
    }

    fn sample_shim_with_host(name: &str, host: &str) -> Shim {
        Shim {
            name: name.to_string(),
            host: Some(host.to_string()),
        }
    }

    #[test]
    fn shim_host_cmd_defaults_to_name() {
        let shim = sample_shim("podman");
        assert_eq!(shim.host_cmd(), "podman");
    }

    #[test]
    fn shim_host_cmd_uses_explicit_host() {
        let shim = sample_shim_with_host("docker", "podman");
        assert_eq!(shim.host_cmd(), "podman");
    }

    #[test]
    fn manifest_default_is_empty() {
        let manifest = ShimsManifest::default();
        assert!(manifest.shims.is_empty());
        assert!(manifest.schema.is_none());
    }

    #[test]
    fn manifest_find_returns_matching_shim() {
        let mut manifest = ShimsManifest::default();
        manifest.shims.push(sample_shim("podman"));
        manifest.shims.push(sample_shim("flatpak"));

        let found = manifest.find("podman");
        assert!(found.is_some());
        assert_eq!(found.unwrap().name, "podman");
    }

    #[test]
    fn manifest_find_returns_none_for_missing() {
        let manifest = ShimsManifest::default();
        assert!(manifest.find("nonexistent").is_none());
    }

    #[test]
    fn manifest_upsert_adds_new_shim() {
        let mut manifest = ShimsManifest::default();
        manifest.upsert(sample_shim("podman"));

        assert_eq!(manifest.shims.len(), 1);
        assert_eq!(manifest.shims[0].name, "podman");
    }

    #[test]
    fn manifest_upsert_updates_existing_shim() {
        let mut manifest = ShimsManifest::default();
        manifest.upsert(sample_shim("podman"));
        manifest.upsert(sample_shim_with_host("podman", "docker"));

        assert_eq!(manifest.shims.len(), 1);
        assert_eq!(manifest.shims[0].host_cmd(), "docker");
    }

    #[test]
    fn manifest_upsert_maintains_sorted_order() {
        let mut manifest = ShimsManifest::default();
        manifest.upsert(sample_shim("zsh"));
        manifest.upsert(sample_shim("bash"));
        manifest.upsert(sample_shim("fish"));

        let names: Vec<_> = manifest.shims.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["bash", "fish", "zsh"]);
    }

    #[test]
    fn manifest_remove_returns_true_when_found() {
        let mut manifest = ShimsManifest::default();
        manifest.shims.push(sample_shim("podman"));

        assert!(manifest.remove("podman"));
        assert!(manifest.shims.is_empty());
    }

    #[test]
    fn manifest_remove_returns_false_when_not_found() {
        let mut manifest = ShimsManifest::default();
        assert!(!manifest.remove("nonexistent"));
    }

    #[test]
    fn manifest_merged_combines_system_and_user() {
        let mut system = ShimsManifest::default();
        system.shims.push(sample_shim("podman"));
        system.shims.push(sample_shim("flatpak"));

        let mut user = ShimsManifest::default();
        user.shims.push(sample_shim("custom"));

        let merged = ShimsManifest::merged(&system, &user);

        assert_eq!(merged.shims.len(), 3);
        assert!(merged.find("podman").is_some());
        assert!(merged.find("flatpak").is_some());
        assert!(merged.find("custom").is_some());
    }

    #[test]
    fn manifest_merged_user_overrides_system() {
        let mut system = ShimsManifest::default();
        system.shims.push(sample_shim("podman"));

        let mut user = ShimsManifest::default();
        user.shims.push(sample_shim_with_host("podman", "docker"));

        let merged = ShimsManifest::merged(&system, &user);

        assert_eq!(merged.shims.len(), 1);
        assert_eq!(merged.find("podman").unwrap().host_cmd(), "docker");
    }

    #[test]
    fn manifest_merged_result_is_sorted() {
        let mut system = ShimsManifest::default();
        system.shims.push(sample_shim("zsh"));

        let mut user = ShimsManifest::default();
        user.shims.push(sample_shim("bash"));

        let merged = ShimsManifest::merged(&system, &user);

        let names: Vec<_> = merged.shims.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["bash", "zsh"]);
    }

    #[test]
    fn manifest_serialization_roundtrip() {
        let mut manifest = ShimsManifest::default();
        manifest.shims.push(sample_shim("podman"));
        manifest
            .shims
            .push(sample_shim_with_host("docker", "podman"));

        let json = serde_json::to_string(&manifest).unwrap();
        let parsed: ShimsManifest = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.shims.len(), 2);
        assert_eq!(parsed.find("podman").unwrap().host, None);
        assert_eq!(
            parsed.find("docker").unwrap().host,
            Some("podman".to_string())
        );
    }

    #[test]
    fn manifest_load_save_roundtrip() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let path = tmp_dir.path().join("test-shims.json");

        let mut manifest = ShimsManifest::default();
        manifest.shims.push(sample_shim("podman"));
        manifest
            .shims
            .push(sample_shim_with_host("docker", "podman"));

        manifest.save(&path).unwrap();
        let loaded = ShimsManifest::load(&path).unwrap();

        assert_eq!(loaded.shims.len(), 2);
        assert!(loaded.find("podman").is_some());
        assert!(loaded.find("docker").is_some());
    }

    #[test]
    fn manifest_load_nonexistent_returns_default() {
        let path = PathBuf::from("/nonexistent/path/shims.json");
        let manifest = ShimsManifest::load(&path).unwrap();
        assert!(manifest.shims.is_empty());
    }
}
