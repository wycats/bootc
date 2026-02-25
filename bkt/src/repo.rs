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

/// Cached repo path location.
///
/// Written after successful discovery so that `find_repo_path()` works from
/// any working directory, not just from inside the repo.
fn cache_path() -> Option<PathBuf> {
    // Prefer XDG_STATE_HOME, fall back to ~/.local/state
    let state_dir = std::env::var("XDG_STATE_HOME")
        .ok()
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|h| PathBuf::from(h).join(".local/state"))
        })?;
    Some(state_dir.join("bkt").join("repo-path"))
}

/// Write the repo path to the cache file atomically.
///
/// Uses write-to-temp + rename to avoid partial writes.
fn write_cache(repo_path: &std::path::Path) {
    if let Some(cache) = cache_path() {
        write_cache_to(&cache, repo_path);
    }
}

fn write_cache_to(cache: &std::path::Path, repo_path: &std::path::Path) {
    if let Some(parent) = cache.parent() {
        let _ = std::fs::create_dir_all(parent);
        let tmp = parent.join(".repo-path.tmp");
        if std::fs::write(&tmp, repo_path.to_string_lossy().as_bytes()).is_ok() {
            let _ = std::fs::rename(&tmp, cache);
        }
    }
}

/// Read the repo path from the cache file, validating it still exists.
///
/// If the cache points to a directory that no longer has `manifests/`,
/// the stale cache file is removed.
fn read_cache() -> Option<PathBuf> {
    let cache = cache_path()?;
    read_cache_from(&cache)
}

fn read_cache_from(cache: &std::path::Path) -> Option<PathBuf> {
    let content = std::fs::read_to_string(cache).ok()?;
    let path = PathBuf::from(content.trim());
    // Validate the cached path still has a manifests/ directory
    if path.join("manifests").is_dir() {
        Some(path)
    } else {
        // Stale cache — clean it up
        let _ = std::fs::remove_file(cache);
        None
    }
}

/// Find the root path of the bootc repository.
///
/// Uses a three-step fallback chain:
/// 1. Walk up from cwd looking for a `manifests/` directory
/// 2. Read cached path from `~/.local/state/bkt/repo-path`
/// 3. Fail with an actionable error message
///
/// When found via cwd walk-up, the path is cached so future invocations
/// from any directory can find the repo.
pub fn find_repo_path() -> Result<PathBuf> {
    // Step 0: Explicit override (for testing and CI)
    if let Ok(explicit) = std::env::var("BKT_REPO_PATH") {
        let path = PathBuf::from(explicit);
        if path.join("manifests").is_dir() {
            return Ok(path);
        }
    }

    // Step 1: Walk up from cwd
    if let Ok(found) = find_repo_path_from_cwd() {
        write_cache(&found);
        return Ok(found);
    }

    // Step 2: Read from cache
    if let Some(cached) = read_cache() {
        return Ok(cached);
    }

    // Step 3: Fail
    let current_dir = std::env::current_dir()
        .map(|d| d.display().to_string())
        .unwrap_or_else(|_| "(unknown)".to_string());
    let cache_hint = match cache_path() {
        Some(p) if p.exists() => format!("Stale cache at {} (removed)", p.display()),
        Some(p) => format!("No cache file at {}", p.display()),
        None => "Could not determine cache path".to_string(),
    };
    anyhow::bail!(
        "Could not find bootc repository root.\n\
         Searched upward from: {}\n\
         {}\n\n\
         To fix this, either:\n  \
         • Run bkt from inside your repo checkout\n  \
         • Set BKT_REPO_PATH to your repo root",
        current_dir,
        cache_hint,
    )
}

/// Walk up from cwd looking for a `manifests/` directory.
fn find_repo_path_from_cwd() -> Result<PathBuf> {
    let current_dir = std::env::current_dir().context("Failed to get current directory")?;

    let mut path = current_dir.as_path();
    loop {
        let manifests_dir = path.join("manifests");
        if manifests_dir.is_dir() {
            return Ok(path.to_path_buf());
        }

        match path.parent() {
            Some(parent) => path = parent,
            None => break,
        }
    }

    anyhow::bail!(
        "No manifests/ directory found above {}",
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

    // --- Cache tests ---
    //
    // These tests use the _to/_from variants directly to avoid mutating
    // environment variables (which is unsafe and racy in parallel tests).

    #[test]
    fn cache_write_and_read_round_trips() {
        let tempdir = tempfile::tempdir().unwrap();
        let cache_file = tempdir.path().join("bkt").join("repo-path");

        // Create a fake repo with manifests/
        let fake_repo = tempdir.path().join("my-repo");
        std::fs::create_dir_all(fake_repo.join("manifests")).unwrap();

        write_cache_to(&cache_file, &fake_repo);

        let cached = read_cache_from(&cache_file);
        assert_eq!(cached, Some(fake_repo));
    }

    #[test]
    fn cache_returns_none_when_no_cache_file() {
        let tempdir = tempfile::tempdir().unwrap();
        let cache_file = tempdir.path().join("nonexistent").join("repo-path");

        let cached = read_cache_from(&cache_file);
        assert!(cached.is_none());
    }

    #[test]
    fn stale_cache_is_cleaned_up() {
        let tempdir = tempfile::tempdir().unwrap();
        let cache_file = tempdir.path().join("bkt").join("repo-path");

        // Create a fake repo, cache it, then delete the repo
        let fake_repo = tempdir.path().join("deleted-repo");
        std::fs::create_dir_all(fake_repo.join("manifests")).unwrap();

        write_cache_to(&cache_file, &fake_repo);
        assert!(cache_file.exists(), "cache file should exist");

        // Delete the repo
        std::fs::remove_dir_all(&fake_repo).unwrap();

        // read_cache_from should return None and clean up the stale file
        let cached = read_cache_from(&cache_file);
        assert!(cached.is_none());
        assert!(!cache_file.exists(), "stale cache file should be removed");
    }

    #[test]
    fn atomic_write_does_not_leave_partial_file() {
        let tempdir = tempfile::tempdir().unwrap();
        let cache_file = tempdir.path().join("bkt").join("repo-path");

        let fake_repo = tempdir.path().join("repo");
        std::fs::create_dir_all(fake_repo.join("manifests")).unwrap();

        write_cache_to(&cache_file, &fake_repo);

        // The temp file should not exist after write
        let tmp_path = tempdir.path().join("bkt").join(".repo-path.tmp");
        assert!(
            !tmp_path.exists(),
            "temp file should be cleaned up by rename"
        );

        // The cache file should have the full path
        let content = std::fs::read_to_string(&cache_file).unwrap();
        assert_eq!(content, fake_repo.to_string_lossy());
    }
}
