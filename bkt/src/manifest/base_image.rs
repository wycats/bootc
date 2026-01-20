//! Base image package extraction and diffing.
//!
//! This module provides functionality to extract package lists from container
//! images and compute diffs between them, enabling build descriptions to show
//! what changed in the upstream base image (e.g., kernel updates, security patches).
//!
//! # Host Command Requirement
//!
//! Package extraction requires `podman` and `skopeo` which are available on the
//! host but not in toolbox. When running from toolbox, commands are automatically
//! delegated to the host via `flatpak-spawn --host`.

use crate::context::{is_in_toolbox, run_host_command};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use super::build_info::{BaseImageChange, PackageChanges, PackageUpdate};

/// A parsed RPM package entry.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct RpmPackage {
    pub name: String,
    pub version: String,
    pub release: String,
    pub arch: String,
}

impl RpmPackage {
    /// Parse an RPM NEVRA string: name-version-release.arch
    pub fn parse(nevra: &str) -> Option<Self> {
        // RPM NEVRA format: name-version-release.arch
        // The tricky part: name can contain hyphens, so we parse from the right
        let (name_version_release, arch) = nevra.rsplit_once('.')?;
        let (name_version, release) = name_version_release.rsplit_once('-')?;
        let (name, version) = name_version.rsplit_once('-')?;

        Some(Self {
            name: name.to_string(),
            version: version.to_string(),
            release: release.to_string(),
            arch: arch.to_string(),
        })
    }

    /// Full version string (version-release)
    pub fn full_version(&self) -> String {
        format!("{}-{}", self.version, self.release)
    }

    /// NEVRA string for display
    #[allow(dead_code)]
    pub fn nevra(&self) -> String {
        format!(
            "{}-{}-{}.{}",
            self.name, self.version, self.release, self.arch
        )
    }
}

/// Result of extracting packages from an image.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ImagePackageList {
    /// Image reference (e.g., ghcr.io/ublue-os/bazzite:stable)
    pub image: String,
    /// Image digest
    pub digest: String,
    /// Map of package name → full version (for diffing)
    pub packages: BTreeMap<String, String>,
}

/// Get the digest of an image using skopeo.
pub fn get_image_digest(image_ref: &str) -> Result<String> {
    let output = run_host_command(
        "skopeo",
        &[
            "inspect",
            "--format",
            "{{.Digest}}",
            &format!("docker://{}", image_ref),
        ],
    )?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("Failed to inspect image {}: {}", image_ref, stderr.trim());
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Extract package list from a container image by running `rpm -qa` inside it.
///
/// This uses `podman run --rm` to start a temporary container and extract
/// the package list. The container is immediately removed after extraction.
pub fn extract_packages_from_image(image_ref: &str) -> Result<ImagePackageList> {
    tracing::info!("Extracting packages from {}", image_ref);

    // First get the digest
    let digest = get_image_digest(image_ref)?;

    // Use podman to run rpm -qa inside the container
    // We use --entrypoint to override any custom entrypoint
    let output = if is_in_toolbox() {
        let mut cmd = Command::new("flatpak-spawn");
        cmd.args([
            "--host",
            "podman",
            "run",
            "--rm",
            "--entrypoint",
            "rpm",
            image_ref,
            "-qa",
            "--qf",
            "%{NAME}-%{VERSION}-%{RELEASE}.%{ARCH}\n",
        ]);
        cmd.output()
            .context("Failed to run podman via flatpak-spawn")?
    } else {
        Command::new("podman")
            .args([
                "run",
                "--rm",
                "--entrypoint",
                "rpm",
                image_ref,
                "-qa",
                "--qf",
                "%{NAME}-%{VERSION}-%{RELEASE}.%{ARCH}\n",
            ])
            .output()
            .context("Failed to run podman")?
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "Failed to extract packages from {}: {}",
            image_ref,
            stderr.trim()
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut packages = BTreeMap::new();

    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(pkg) = RpmPackage::parse(line) {
            // Use name-arch as key to handle multilib packages
            let key = format!("{}.{}", pkg.name, pkg.arch);
            packages.insert(key, pkg.full_version());
        }
    }

    tracing::info!("Extracted {} packages from {}", packages.len(), image_ref);

    Ok(ImagePackageList {
        image: image_ref.to_string(),
        digest,
        packages,
    })
}

/// Cache file for package lists to avoid repeated expensive extractions.
fn cache_path(repo_path: &Path, digest: &str) -> std::path::PathBuf {
    // Use a .cache directory in the repo
    let cache_dir = repo_path.join(".cache/base-image-packages");
    let digest_hash = &digest.replace("sha256:", "")[..16]; // First 16 chars
    cache_dir.join(format!("{}.json", digest_hash))
}

/// Try to load a cached package list.
pub fn load_cached_packages(repo_path: &Path, digest: &str) -> Option<ImagePackageList> {
    let path = cache_path(repo_path, digest);
    if path.exists() {
        match std::fs::read_to_string(&path) {
            Ok(content) => match serde_json::from_str(&content) {
                Ok(list) => {
                    tracing::debug!("Loaded cached package list from {}", path.display());
                    return Some(list);
                }
                Err(e) => {
                    tracing::warn!("Failed to parse cached package list: {}", e);
                }
            },
            Err(e) => {
                tracing::warn!("Failed to read cache file: {}", e);
            }
        }
    }
    None
}

/// Save a package list to cache.
pub fn save_cached_packages(repo_path: &Path, list: &ImagePackageList) -> Result<()> {
    let path = cache_path(repo_path, &list.digest);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create cache directory {}", parent.display()))?;
    }
    let content = serde_json::to_string_pretty(list)?;
    std::fs::write(&path, content)
        .with_context(|| format!("Failed to write cache file {}", path.display()))?;
    tracing::debug!("Cached package list to {}", path.display());
    Ok(())
}

/// Get package list for an image, using cache if available.
pub fn get_packages_cached(
    repo_path: &Path,
    image_ref: &str,
    digest: &str,
) -> Result<ImagePackageList> {
    // Check cache first
    if let Some(cached) = load_cached_packages(repo_path, digest) {
        return Ok(cached);
    }

    // Extract and cache
    let list = extract_packages_from_image(image_ref)?;
    save_cached_packages(repo_path, &list)?;
    Ok(list)
}

/// Compute the diff between two package lists.
pub fn diff_packages(old: &ImagePackageList, new: &ImagePackageList) -> PackageChanges {
    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut updated = Vec::new();

    // Find removed and updated packages
    for (name, old_version) in &old.packages {
        match new.packages.get(name) {
            None => {
                // Package was removed
                removed.push(format!("{}-{}", name, old_version));
            }
            Some(new_version) if old_version != new_version => {
                // Package was updated
                // Extract base name (remove .arch suffix)
                let base_name = name.rsplit_once('.').map(|(n, _)| n).unwrap_or(name);
                updated.push(PackageUpdate {
                    name: base_name.to_string(),
                    from: old_version.clone(),
                    to: new_version.clone(),
                });
            }
            Some(_) => {
                // Same version, no change
            }
        }
    }

    // Find added packages
    for (name, new_version) in &new.packages {
        if !old.packages.contains_key(name) {
            added.push(format!("{}-{}", name, new_version));
        }
    }

    // Sort for consistent output
    added.sort();
    removed.sort();
    updated.sort_by(|a, b| a.name.cmp(&b.name));

    PackageChanges {
        added,
        removed,
        updated,
    }
}

/// Compute base image changes between two digests.
pub fn diff_base_image(
    repo_path: &Path,
    image_name: &str,
    old_digest: &str,
    new_digest: &str,
) -> Result<BaseImageChange> {
    // Get package lists for both images
    let old_ref = format!("{}@{}", image_name, old_digest);
    let new_ref = format!("{}@{}", image_name, new_digest);

    let old_packages = get_packages_cached(repo_path, &old_ref, old_digest)?;
    let new_packages = get_packages_cached(repo_path, &new_ref, new_digest)?;

    let package_changes = diff_packages(&old_packages, &new_packages);

    let packages = if package_changes.added.is_empty()
        && package_changes.removed.is_empty()
        && package_changes.updated.is_empty()
    {
        None
    } else {
        Some(package_changes)
    };

    Ok(BaseImageChange {
        name: image_name.to_string(),
        previous_digest: old_digest.to_string(),
        current_digest: new_digest.to_string(),
        packages,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_rpm_package() {
        let pkg = RpmPackage::parse("kernel-6.12.4-200.fc41.x86_64").unwrap();
        assert_eq!(pkg.name, "kernel");
        assert_eq!(pkg.version, "6.12.4");
        assert_eq!(pkg.release, "200.fc41");
        assert_eq!(pkg.arch, "x86_64");
    }

    #[test]
    fn test_parse_rpm_package_with_hyphen_in_name() {
        let pkg = RpmPackage::parse("mesa-vulkan-drivers-24.2.3-1.fc41.x86_64").unwrap();
        assert_eq!(pkg.name, "mesa-vulkan-drivers");
        assert_eq!(pkg.version, "24.2.3");
        assert_eq!(pkg.release, "1.fc41");
        assert_eq!(pkg.arch, "x86_64");
    }

    #[test]
    fn test_diff_packages_added() {
        let old = ImagePackageList {
            image: "test".into(),
            digest: "sha256:old".into(),
            packages: BTreeMap::new(),
        };
        let mut new_packages = BTreeMap::new();
        new_packages.insert("kernel.x86_64".into(), "6.12.5-201.fc41".into());
        let new = ImagePackageList {
            image: "test".into(),
            digest: "sha256:new".into(),
            packages: new_packages,
        };

        let diff = diff_packages(&old, &new);
        assert_eq!(diff.added.len(), 1);
        assert!(diff.added[0].contains("kernel"));
        assert!(diff.removed.is_empty());
        assert!(diff.updated.is_empty());
    }

    #[test]
    fn test_diff_packages_updated() {
        let mut old_packages = BTreeMap::new();
        old_packages.insert("kernel.x86_64".into(), "6.12.4-200.fc41".into());
        let old = ImagePackageList {
            image: "test".into(),
            digest: "sha256:old".into(),
            packages: old_packages,
        };

        let mut new_packages = BTreeMap::new();
        new_packages.insert("kernel.x86_64".into(), "6.12.5-201.fc41".into());
        let new = ImagePackageList {
            image: "test".into(),
            digest: "sha256:new".into(),
            packages: new_packages,
        };

        let diff = diff_packages(&old, &new);
        assert!(diff.added.is_empty());
        assert!(diff.removed.is_empty());
        assert_eq!(diff.updated.len(), 1);
        assert_eq!(diff.updated[0].name, "kernel");
        assert_eq!(diff.updated[0].from, "6.12.4-200.fc41");
        assert_eq!(diff.updated[0].to, "6.12.5-201.fc41");
    }
}
