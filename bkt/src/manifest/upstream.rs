//! Upstream dependency manifest types.
//!
//! Manages external resources (themes, icons, fonts, tools) with
//! version pinning and cryptographic verification.

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Source of an upstream dependency.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum UpstreamSource {
    /// GitHub repository (release, tag, or branch)
    GitHub {
        /// Repository in owner/repo format
        repo: String,
        /// Optional glob pattern to match release asset
        #[serde(skip_serializing_if = "Option::is_none")]
        asset_pattern: Option<String>,
        /// How to fetch: release, tag, or branch
        #[serde(default)]
        release_type: ReleaseType,
    },
    /// Direct URL download
    Url {
        /// Download URL. Use {version} placeholder for version substitution.
        url: String,
    },
}

/// How to fetch a GitHub dependency.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ReleaseType {
    /// GitHub Release (default)
    #[default]
    Release,
    /// Git tag
    Tag,
    /// Git branch (pinned to commit)
    Branch,
}

/// Pinned version information.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct PinnedVersion {
    /// Version string (tag, commit SHA, or "latest")
    pub version: String,

    /// Git commit SHA (for GitHub sources)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,

    /// Resolved download URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,

    /// SHA256 checksum of the downloaded asset
    pub sha256: String,

    /// Whether GPG signature was verified
    #[serde(default)]
    pub gpg_verified: bool,

    /// When this version was pinned
    pub pinned_at: DateTime<Utc>,
}

/// How to install an upstream dependency.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum InstallConfig {
    /// Extract an archive
    Archive {
        /// Target directory for extraction
        extract_to: String,
        /// Number of leading path components to strip
        #[serde(default)]
        strip_components: u32,
    },
    /// Install a binary
    Binary {
        /// Target installation path
        install_path: String,
    },
    /// Run an install script
    Script {
        /// Script command to run
        command: String,
    },
}

/// An upstream dependency entry.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct Upstream {
    /// Unique identifier for this upstream
    pub name: String,

    /// Human-readable description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Source of the dependency
    pub source: UpstreamSource,

    /// Pinned version information
    pub pinned: PinnedVersion,

    /// Installation configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install: Option<InstallConfig>,
}

impl Upstream {
    /// Create a new upstream from a GitHub source.
    #[allow(dead_code)]
    pub fn new_github(name: String, repo: String, version: String, sha256: String) -> Self {
        Self {
            name,
            description: None,
            source: UpstreamSource::GitHub {
                repo,
                asset_pattern: None,
                release_type: ReleaseType::Release,
            },
            pinned: PinnedVersion {
                version,
                commit: None,
                url: None,
                sha256,
                gpg_verified: false,
                pinned_at: Utc::now(),
            },
            install: None,
        }
    }

    /// Create a new upstream from a URL source.
    #[allow(dead_code)]
    pub fn new_url(name: String, url: String, version: String, sha256: String) -> Self {
        Self {
            name,
            description: None,
            source: UpstreamSource::Url { url },
            pinned: PinnedVersion {
                version,
                commit: None,
                url: None,
                sha256,
                gpg_verified: false,
                pinned_at: Utc::now(),
            },
            install: None,
        }
    }

    /// Get the GitHub repo if this is a GitHub source.
    #[allow(dead_code)]
    pub fn github_repo(&self) -> Option<&str> {
        match &self.source {
            UpstreamSource::GitHub { repo, .. } => Some(repo),
            _ => None,
        }
    }
}

/// The upstream manifest (upstream/manifest.json).
#[derive(Debug, Clone, Serialize, Deserialize, Default, JsonSchema)]
pub struct UpstreamManifest {
    #[serde(rename = "$schema", skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,

    /// List of upstream dependencies
    #[serde(default)]
    pub upstreams: Vec<Upstream>,
}

impl UpstreamManifest {
    /// Default manifest path in the repository.
    pub const MANIFEST_PATH: &'static str = "upstream/manifest.json";

    /// Verified manifest hash path.
    pub const VERIFIED_PATH: &'static str = "upstream/manifest.verified";

    /// Load the manifest from the repository root.
    pub fn load() -> Result<Self> {
        let path = Self::manifest_path()?;
        Self::load_from(&path)
    }

    /// Load a manifest from a specific path.
    pub fn load_from(path: &PathBuf) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read upstream manifest from {}", path.display()))?;
        let manifest: Self = serde_json::from_str(&content).with_context(|| {
            format!("Failed to parse upstream manifest from {}", path.display())
        })?;
        Ok(manifest)
    }

    /// Save the manifest to the repository root.
    pub fn save(&self) -> Result<()> {
        let path = Self::manifest_path()?;
        self.save_to(&path)
    }

    /// Save the manifest to a specific path.
    pub fn save_to(&self, path: &PathBuf) -> Result<()> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(self)?;
        fs::write(path, content)
            .with_context(|| format!("Failed to write upstream manifest to {}", path.display()))?;
        Ok(())
    }

    /// Get the manifest path in the repository.
    fn manifest_path() -> Result<PathBuf> {
        // Try to find the repository root
        let mut current = std::env::current_dir()?;
        loop {
            let manifest = current.join(Self::MANIFEST_PATH);
            let git = current.join(".git");
            if manifest.exists() || git.exists() {
                return Ok(current.join(Self::MANIFEST_PATH));
            }
            if !current.pop() {
                bail!("Not in a git repository");
            }
        }
    }

    /// Find an upstream by name.
    pub fn find(&self, name: &str) -> Option<&Upstream> {
        self.upstreams.iter().find(|u| u.name == name)
    }

    /// Find an upstream by name (mutable).
    pub fn find_mut(&mut self, name: &str) -> Option<&mut Upstream> {
        self.upstreams.iter_mut().find(|u| u.name == name)
    }

    /// Add or update an upstream.
    pub fn upsert(&mut self, upstream: Upstream) {
        if let Some(existing) = self.find_mut(&upstream.name) {
            *existing = upstream;
        } else {
            self.upstreams.push(upstream);
        }
        // Keep sorted by name
        self.upstreams.sort_by(|a, b| a.name.cmp(&b.name));
    }

    /// Remove an upstream by name.
    pub fn remove(&mut self, name: &str) -> bool {
        let len_before = self.upstreams.len();
        self.upstreams.retain(|u| u.name != name);
        self.upstreams.len() != len_before
    }

    /// Check if an upstream exists.
    pub fn contains(&self, name: &str) -> bool {
        self.find(name).is_some()
    }

    /// Generate individual files for each upstream (for Containerfile caching).
    pub fn generate_files(&self) -> Result<()> {
        let base = Self::upstream_dir()?;
        for upstream in &self.upstreams {
            let dir = base.join(&upstream.name);
            fs::create_dir_all(&dir)?;

            fs::write(dir.join("version"), &upstream.pinned.version)?;
            fs::write(dir.join("sha256"), &upstream.pinned.sha256)?;

            if let Some(url) = &upstream.pinned.url {
                fs::write(dir.join("url"), url)?;
            }

            if let Some(commit) = &upstream.pinned.commit {
                fs::write(dir.join("commit"), commit)?;
            }
        }
        Ok(())
    }

    /// Get the upstream directory path.
    fn upstream_dir() -> Result<PathBuf> {
        let mut current = std::env::current_dir()?;
        loop {
            let upstream = current.join("upstream");
            let git = current.join(".git");
            if upstream.exists() || git.exists() {
                return Ok(current.join("upstream"));
            }
            if !current.pop() {
                bail!("Not in a git repository");
            }
        }
    }

    /// Compute SHA256 of the manifest for verification.
    pub fn compute_hash(&self) -> Result<String> {
        use sha2::{Digest, Sha256};
        let content = serde_json::to_string_pretty(self)?;
        let hash = Sha256::digest(content.as_bytes());
        Ok(hex::encode(hash))
    }

    /// Write the verified manifest hash.
    pub fn write_verified_hash(&self) -> Result<()> {
        let path = Self::verified_path()?;
        let hash = self.compute_hash()?;
        fs::write(path, hash)?;
        Ok(())
    }

    /// Read the verified manifest hash.
    #[allow(dead_code)]
    pub fn read_verified_hash() -> Result<Option<String>> {
        let path = Self::verified_path()?;
        if !path.exists() {
            return Ok(None);
        }
        let hash = fs::read_to_string(path)?.trim().to_string();
        Ok(Some(hash))
    }

    /// Verify the manifest against the stored hash.
    #[allow(dead_code)]
    pub fn verify_hash(&self) -> Result<bool> {
        let stored = Self::read_verified_hash()?;
        match stored {
            Some(stored_hash) => {
                let current_hash = self.compute_hash()?;
                Ok(stored_hash == current_hash)
            }
            None => Ok(false),
        }
    }

    /// Get the verified hash file path.
    fn verified_path() -> Result<PathBuf> {
        let mut current = std::env::current_dir()?;
        loop {
            let upstream = current.join("upstream");
            let git = current.join(".git");
            if upstream.exists() || git.exists() {
                return Ok(current.join(Self::VERIFIED_PATH));
            }
            if !current.pop() {
                bail!("Not in a git repository");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_upstream_new_github() {
        let upstream = Upstream::new_github(
            "bibata".to_string(),
            "ful1e5/Bibata_Cursor".to_string(),
            "v2.0.7".to_string(),
            "abc123".to_string(),
        );
        assert_eq!(upstream.name, "bibata");
        assert_eq!(upstream.github_repo(), Some("ful1e5/Bibata_Cursor"));
        assert_eq!(upstream.pinned.version, "v2.0.7");
    }

    #[test]
    fn test_manifest_upsert() {
        let mut manifest = UpstreamManifest::default();
        let upstream1 = Upstream::new_github(
            "bibata".to_string(),
            "ful1e5/Bibata_Cursor".to_string(),
            "v2.0.6".to_string(),
            "abc123".to_string(),
        );
        manifest.upsert(upstream1);
        assert!(manifest.contains("bibata"));

        // Update with new version
        let upstream2 = Upstream::new_github(
            "bibata".to_string(),
            "ful1e5/Bibata_Cursor".to_string(),
            "v2.0.7".to_string(),
            "def456".to_string(),
        );
        manifest.upsert(upstream2);
        assert_eq!(manifest.upstreams.len(), 1);
        assert_eq!(manifest.find("bibata").unwrap().pinned.version, "v2.0.7");
    }

    #[test]
    fn test_manifest_remove() {
        let mut manifest = UpstreamManifest::default();
        let upstream = Upstream::new_github(
            "bibata".to_string(),
            "ful1e5/Bibata_Cursor".to_string(),
            "v2.0.7".to_string(),
            "abc123".to_string(),
        );
        manifest.upsert(upstream);
        assert!(manifest.remove("bibata"));
        assert!(!manifest.contains("bibata"));
        assert!(!manifest.remove("bibata")); // Already removed
    }

    #[test]
    fn test_serialize_github_source() {
        let upstream = Upstream::new_github(
            "test".to_string(),
            "owner/repo".to_string(),
            "v1.0.0".to_string(),
            "abc123def456".to_string(),
        );
        let json = serde_json::to_string_pretty(&upstream).unwrap();
        assert!(json.contains("\"type\": \"github\""));
        assert!(json.contains("\"repo\": \"owner/repo\""));
    }

    #[test]
    fn test_serialize_url_source() {
        let upstream = Upstream::new_url(
            "test".to_string(),
            "https://example.com/file.tar.gz".to_string(),
            "v1.0.0".to_string(),
            "abc123def456".to_string(),
        );
        let json = serde_json::to_string_pretty(&upstream).unwrap();
        assert!(json.contains("\"type\": \"url\""));
        assert!(json.contains("https://example.com/file.tar.gz"));
    }
}
