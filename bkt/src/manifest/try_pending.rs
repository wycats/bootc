//! Pending `bkt try` state tracked across the current boot.
//!
//! This manifest tracks transient overlay installs that are pending a PR merge.
//! It is stored in `~/.local/state/bkt/try-pending.json` and invalidates when
//! the boot ID changes.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use directories::BaseDirs;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// A pending `bkt try` package entry.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TryPendingEntry {
    /// Package name (as installed by dnf5).
    pub package: String,
    /// Timestamp when the package was installed.
    pub installed_at: DateTime<Utc>,
    /// PR number, if a PR exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr: Option<u64>,
    /// Git branch name for the try PR.
    pub branch: String,
    /// Services enabled as a side effect of the try install.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub services_enabled: Vec<String>,
}

/// Manifest that records pending `bkt try` state for the current boot.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct TryPendingManifest {
    /// Schema reference for tooling.
    #[serde(rename = "$schema", skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,

    /// Boot ID when this manifest was created.
    #[serde(default)]
    pub boot_id: String,

    /// Pending packages keyed by package name.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub packages: HashMap<String, TryPendingEntry>,
}

impl TryPendingManifest {
    /// Path to the try-pending manifest file.
    pub fn path() -> PathBuf {
        let state_dir = std::env::var("XDG_STATE_HOME")
            .ok()
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var("HOME")
                    .ok()
                    .map(|h| PathBuf::from(h).join(".local/state"))
            })
            .or_else(|| BaseDirs::new().map(|d| d.home_dir().join(".local/state")))
            .unwrap_or_else(|| PathBuf::from(".local/state"));
        state_dir.join("bkt").join("try-pending.json")
    }

    /// Read the current boot ID from the kernel.
    pub fn current_boot_id() -> Result<String> {
        #[cfg(target_os = "linux")]
        {
            fs::read_to_string("/proc/sys/kernel/random/boot_id")
                .map(|s| s.trim().to_string())
                .context("Failed to read boot ID from /proc/sys/kernel/random/boot_id")
        }

        #[cfg(not(target_os = "linux"))]
        {
            Ok("non-linux-boot-id-unsupported".to_string())
        }
    }

    /// Load the try-pending manifest from disk.
    ///
    /// Returns an empty manifest if the file doesn't exist.
    pub fn load() -> Result<Self> {
        let path = Self::path();
        if !path.exists() {
            return Ok(Self::default());
        }

        let content = fs::read_to_string(&path).with_context(|| {
            format!(
                "Failed to read try-pending manifest from {}",
                path.display()
            )
        })?;

        serde_json::from_str(&content).with_context(|| {
            format!(
                "Failed to parse try-pending manifest from {}",
                path.display()
            )
        })
    }

    /// Save the try-pending manifest to disk.
    pub fn save(&self) -> Result<()> {
        let path = Self::path();

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory {}", parent.display()))?;
        }

        let content = serde_json::to_string_pretty(self)
            .context("Failed to serialize try-pending manifest")?;

        fs::write(&path, content).with_context(|| {
            format!("Failed to write try-pending manifest to {}", path.display())
        })?;

        Ok(())
    }

    /// Add or update a package entry.
    ///
    /// Returns true if the package was newly added.
    pub fn add_package(&mut self, entry: TryPendingEntry) -> bool {
        let key = entry.package.clone();
        let is_new = !self.packages.contains_key(&key);
        self.packages.insert(key, entry);
        is_new
    }

    /// Check whether this manifest is valid for the current boot.
    pub fn is_valid(&self) -> Result<bool> {
        let current_boot_id = Self::current_boot_id()?;
        if self.boot_id.is_empty() {
            return Ok(true);
        }
        Ok(self.boot_id == current_boot_id)
    }

    /// Initialize a new manifest with the provided boot ID.
    pub fn new_with_boot_id(boot_id: String) -> Self {
        Self {
            schema: None,
            boot_id,
            packages: HashMap::new(),
        }
    }
}
