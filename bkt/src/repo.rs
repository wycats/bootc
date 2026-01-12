//! Repository configuration and discovery.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::PathBuf;

/// Repository identity and metadata.
#[derive(Debug, Clone, Deserialize)]
pub struct RepoConfig {
    pub owner: String,
    pub name: String,
    pub url: String,
    pub default_branch: String,
}

impl RepoConfig {
    /// Default path for the repo configuration file.
    pub const DEFAULT_PATH: &'static str = "/usr/share/bootc/repo.json";

    /// Host-accessible path when running in a toolbox.
    /// Toolbox mounts host filesystem at /run/host.
    pub const HOST_PATH: &'static str = "/run/host/usr/share/bootc/repo.json";

    /// Load the repository configuration from the default location.
    ///
    /// Checks both the standard path and the host-accessible path (for toolbox).
    pub fn load() -> Result<Self> {
        Self::load_with_paths(Self::DEFAULT_PATH, Self::HOST_PATH)
    }

    fn load_with_paths(default_path: &str, host_path: &str) -> Result<Self> {
        match Self::load_from(default_path) {
            Ok(config) => Ok(config),
            Err(default_err) => match Self::load_from(host_path) {
                Ok(config) => Ok(config),
                Err(host_err) => anyhow::bail!(
                    "Repository config not found at {} or {}\n\
                     Error for {}: {}\n\
                     Error for {}: {}\n\
                     Ensure bkt is running from a properly built bootc image",
                    default_path,
                    host_path,
                    default_path,
                    default_err,
                    host_path,
                    host_err,
                ),
            },
        }
    }

    /// Load the repository configuration from a specific path.
    pub fn load_from(path: &str) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read repo config from {}", path))?;
        let config: RepoConfig = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse repo config from {}", path))?;
        Ok(config)
    }
}

/// Find the root path of the bootc repository.
///
/// Searches upward from the current directory for a directory containing
/// the `manifests/` folder, which indicates the repository root.
pub fn find_repo_path() -> Result<PathBuf> {
    let current_dir = std::env::current_dir().context("Failed to get current directory")?;

    let mut path = current_dir.as_path();
    loop {
        // Check if this directory contains the manifests folder
        let manifests_dir = path.join("manifests");
        if manifests_dir.is_dir() {
            return Ok(path.to_path_buf());
        }

        // Move up one directory
        match path.parent() {
            Some(parent) => path = parent,
            None => break,
        }
    }

    anyhow::bail!(
        "Could not find bootc repository root (no manifests/ directory found above {})",
        current_dir.display()
    )
}

/// Get the path to the manifests directory.
#[allow(dead_code)]
pub fn manifests_path() -> Result<PathBuf> {
    Ok(find_repo_path()?.join("manifests"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn write_repo_json(path: &Path) {
        std::fs::write(
            path,
            r#"{
    "owner": "test-owner",
    "name": "test-repo",
    "url": "https://github.com/test-owner/test-repo",
    "default_branch": "main"
}"#,
        )
        .unwrap();
    }

    #[test]
    fn load_prefers_default_path_when_present() {
        let tempdir = tempfile::tempdir().unwrap();
        let default_path = tempdir.path().join("repo-default.json");
        let host_path = tempdir.path().join("repo-host.json");

        write_repo_json(&default_path);
        write_repo_json(&host_path);

        let config = RepoConfig::load_with_paths(
            default_path.to_str().unwrap(),
            host_path.to_str().unwrap(),
        )
        .unwrap();

        assert_eq!(config.owner, "test-owner");
        assert_eq!(config.name, "test-repo");
        assert_eq!(config.default_branch, "main");
    }

    #[test]
    fn load_falls_back_to_host_path_when_default_missing() {
        let tempdir = tempfile::tempdir().unwrap();
        let default_path = tempdir.path().join("missing.json");
        let host_path = tempdir.path().join("repo-host.json");

        write_repo_json(&host_path);

        let config = RepoConfig::load_with_paths(
            default_path.to_str().unwrap(),
            host_path.to_str().unwrap(),
        )
        .unwrap();

        assert_eq!(config.owner, "test-owner");
        assert_eq!(config.name, "test-repo");
    }

    #[test]
    fn load_errors_when_neither_path_exists() {
        let tempdir = tempfile::tempdir().unwrap();
        let default_path = tempdir.path().join("missing-default.json");
        let host_path = tempdir.path().join("missing-host.json");

        let err = RepoConfig::load_with_paths(
            default_path.to_str().unwrap(),
            host_path.to_str().unwrap(),
        )
        .unwrap_err();

        let msg = err.to_string();
        assert!(msg.contains("Repository config not found at"));
        assert!(msg.contains(default_path.to_str().unwrap()));
        assert!(msg.contains(host_path.to_str().unwrap()));
        assert!(msg.contains("Error for"));
        assert!(msg.contains("Ensure bkt is running from a properly built bootc image"));
    }
}
