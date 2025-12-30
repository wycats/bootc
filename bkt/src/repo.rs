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

    /// Load the repository configuration from the default location.
    pub fn load() -> Result<Self> {
        Self::load_from(Self::DEFAULT_PATH)
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
