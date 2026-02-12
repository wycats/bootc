//! Upstream dependency manifest types.
//!
//! Manages external resources (themes, icons, fonts, tools) with
//! version pinning and cryptographic verification.

use anyhow::{Context, Result, bail};
use std::fs;
use std::path::PathBuf;

pub use bkt_common::manifest::*;

pub const MANIFEST_PATH: &str = "upstream/manifest.json";
pub const VERIFIED_PATH: &str = "upstream/manifest.verified";

pub trait ManifestRepo {
    fn load() -> Result<UpstreamManifest>;
    fn save(&self) -> Result<()>;
    fn generate_files(&self) -> Result<()>;
    fn compute_hash(&self) -> Result<String>;
    fn write_verified_hash(&self) -> Result<()>;
    #[allow(dead_code)]
    fn read_verified_hash() -> Result<Option<String>>;
    #[allow(dead_code)]
    fn verify_hash(&self) -> Result<bool>;
}

impl ManifestRepo for UpstreamManifest {
    fn load() -> Result<Self> {
        let path = manifest_path()?;
        Self::load_from(&path)
            .with_context(|| format!("Failed to read upstream manifest from {}", path.display()))
    }

    fn save(&self) -> Result<()> {
        let path = manifest_path()?;
        self.save_to(&path)
            .with_context(|| format!("Failed to write upstream manifest to {}", path.display()))
    }

    fn generate_files(&self) -> Result<()> {
        let base = upstream_dir()?;
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

    fn compute_hash(&self) -> Result<String> {
        use sha2::{Digest, Sha256};
        let content = serde_json::to_string_pretty(self)?;
        let hash = Sha256::digest(content.as_bytes());
        Ok(hex::encode(hash))
    }

    fn write_verified_hash(&self) -> Result<()> {
        let path = verified_path()?;
        let hash = self.compute_hash()?;
        fs::write(path, hash)?;
        Ok(())
    }

    fn read_verified_hash() -> Result<Option<String>> {
        let path = verified_path()?;
        if !path.exists() {
            return Ok(None);
        }
        let hash = fs::read_to_string(path)?.trim().to_string();
        Ok(Some(hash))
    }

    fn verify_hash(&self) -> Result<bool> {
        let stored = Self::read_verified_hash()?;
        match stored {
            Some(stored_hash) => {
                let current_hash = self.compute_hash()?;
                Ok(stored_hash == current_hash)
            }
            None => Ok(false),
        }
    }
}

fn manifest_path() -> Result<PathBuf> {
    let mut current = std::env::current_dir()?;
    loop {
        let manifest = current.join(MANIFEST_PATH);
        let git = current.join(".git");
        if manifest.exists() || git.exists() {
            return Ok(current.join(MANIFEST_PATH));
        }
        if !current.pop() {
            bail!("Not in a git repository");
        }
    }
}

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

fn verified_path() -> Result<PathBuf> {
    let mut current = std::env::current_dir()?;
    loop {
        let upstream = current.join("upstream");
        let git = current.join(".git");
        if upstream.exists() || git.exists() {
            return Ok(current.join(VERIFIED_PATH));
        }
        if !current.pop() {
            bail!("Not in a git repository");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn github_upstream(name: &str, repo: &str, version: &str, sha256: &str) -> Upstream {
        Upstream {
            name: name.to_string(),
            description: None,
            source: UpstreamSource::GitHub {
                repo: repo.to_string(),
                asset_pattern: None,
                release_type: ReleaseType::Release,
            },
            pinned: PinnedVersion {
                version: version.to_string(),
                commit: None,
                url: None,
                sha256: sha256.to_string(),
                gpg_verified: false,
                pinned_at: Utc::now(),
            },
            install: None,
        }
    }

    fn url_upstream(name: &str, url: &str, version: &str, sha256: &str) -> Upstream {
        Upstream {
            name: name.to_string(),
            description: None,
            source: UpstreamSource::Url {
                url: url.to_string(),
            },
            pinned: PinnedVersion {
                version: version.to_string(),
                commit: None,
                url: None,
                sha256: sha256.to_string(),
                gpg_verified: false,
                pinned_at: Utc::now(),
            },
            install: None,
        }
    }

    #[test]
    fn test_manifest_upsert() {
        let mut manifest = UpstreamManifest::default();
        let upstream1 = github_upstream("bibata", "ful1e5/Bibata_Cursor", "v2.0.6", "abc123");
        manifest.upsert(upstream1);
        assert!(manifest.contains("bibata"));

        let upstream2 = github_upstream("bibata", "ful1e5/Bibata_Cursor", "v2.0.7", "def456");
        manifest.upsert(upstream2);
        assert_eq!(manifest.upstreams.len(), 1);
        assert_eq!(manifest.find("bibata").unwrap().pinned.version, "v2.0.7");
    }

    #[test]
    fn test_manifest_remove() {
        let mut manifest = UpstreamManifest::default();
        let upstream = github_upstream("bibata", "ful1e5/Bibata_Cursor", "v2.0.7", "abc123");
        manifest.upsert(upstream);
        assert!(manifest.remove("bibata"));
        assert!(!manifest.contains("bibata"));
        assert!(!manifest.remove("bibata"));
    }

    #[test]
    fn test_serialize_github_source() {
        let upstream = github_upstream("test", "owner/repo", "v1.0.0", "abc123def456");
        let json = serde_json::to_string_pretty(&upstream).unwrap();
        assert!(json.contains("\"type\": \"github\""));
        assert!(json.contains("\"repo\": \"owner/repo\""));
    }

    #[test]
    fn test_serialize_url_source() {
        let upstream = url_upstream(
            "test",
            "https://example.com/file.tar.gz",
            "v1.0.0",
            "abc123def456",
        );
        let json = serde_json::to_string_pretty(&upstream).unwrap();
        assert!(json.contains("\"type\": \"url\""));
        assert!(json.contains("https://example.com/file.tar.gz"));
    }
}
