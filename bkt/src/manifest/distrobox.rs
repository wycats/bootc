//! Distrobox manifest types.

use anyhow::{Context, Result, bail};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

/// Binary export configuration for a container.
#[derive(Debug, Clone, Serialize, Deserialize, Default, JsonSchema)]
pub struct DistroboxBins {
    /// Directories to export all binaries from (absolute or ~ paths)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub from: Vec<String>,

    /// Additional binaries to export (absolute or ~ paths)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub also: Vec<String>,

    /// Path to export binaries to (default: ~/.local/bin)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
}

impl DistroboxBins {
    pub fn is_empty(&self) -> bool {
        self.from.is_empty() && self.also.is_empty() && self.to.is_none()
    }
}

/// The distrobox.json manifest.
#[derive(Debug, Clone, Serialize, Deserialize, Default, JsonSchema)]
pub struct DistroboxManifest {
    #[serde(rename = "$schema", skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    #[serde(default)]
    pub containers: BTreeMap<String, DistroboxContainer>,
}

/// A single Distrobox container definition.
#[derive(Debug, Clone, Serialize, Deserialize, Default, JsonSchema)]
pub struct DistroboxContainer {
    /// Container image (required)
    pub image: String,

    /// Additional packages to install (distrobox additional_packages)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub packages: Vec<String>,

    /// Binary exports
    #[serde(default, skip_serializing_if = "DistroboxBins::is_empty")]
    pub bins: DistroboxBins,

    /// Apps to export (desktop entries)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exported_apps: Vec<String>,

    /// Init hooks (run on each container start).
    ///
    /// Axiom: Init hooks must be idempotent AND bring the container to the same
    /// state as a fresh bootstrap. Valid examples: `rustup update`, `proto install node`.
    /// Invalid examples: appending to files, installing pinned versions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub init_hooks: Vec<String>,

    /// Pre-init hooks (run before init)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pre_init_hooks: Vec<String>,

    /// Volume mounts (distrobox volume)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub volume: Vec<String>,

    /// Pull image on assemble
    #[serde(default)]
    pub pull: bool,

    /// Run init scripts
    #[serde(default)]
    pub init: bool,

    /// Run as root
    #[serde(default)]
    pub root: bool,

    /// PATH entries (shell-agnostic)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path: Vec<String>,

    /// Environment variables (excluding PATH)
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,

    /// Pass-through flags (cannot set PATH)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub additional_flags: Vec<String>,
}

impl DistroboxManifest {
    /// Project manifest path (relative to repo root).
    pub const PROJECT_PATH: &'static str = "manifests/distrobox.json";

    /// Load a manifest from a path.
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = fs::read_to_string(path).with_context(|| {
            format!("Failed to read distrobox manifest from {}", path.display())
        })?;
        let manifest: Self = serde_json::from_str(&content).with_context(|| {
            format!("Failed to parse distrobox manifest from {}", path.display())
        })?;
        Ok(manifest)
    }

    /// Save a manifest to a path.
    pub fn save(&self, path: &PathBuf) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory {}", parent.display()))?;
        }
        let content =
            serde_json::to_string_pretty(self).context("Failed to serialize distrobox manifest")?;
        fs::write(path, content)
            .with_context(|| format!("Failed to write distrobox manifest to {}", path.display()))?;
        Ok(())
    }

    /// Load from a manifest directory (repo root/manifests).
    pub fn load_from_dir(dir: &Path) -> Result<Self> {
        Self::load(&dir.join(Self::PROJECT_PATH))
    }

    /// Save to a manifest directory (repo root/manifests).
    pub fn save_to_dir(&self, dir: &Path) -> Result<()> {
        self.save(&dir.join(Self::PROJECT_PATH))
    }
}

impl DistroboxContainer {
    /// Merge captured packages into the container definition.
    pub fn merge_packages(&mut self, captured: Vec<String>) {
        let existing: HashSet<_> = self.packages.iter().cloned().collect();
        for pkg in captured {
            if !existing.contains(&pkg) {
                self.packages.push(pkg);
            }
        }
        self.packages.sort();
    }

    /// Validate container settings.
    pub fn validate(&self, name: &str) -> Result<()> {
        if self.image.trim().is_empty() {
            bail!(
                "Distrobox container '{}' is missing required field: image",
                name
            );
        }

        if self.env.keys().any(|k| k == "PATH") {
            bail!(
                "Distrobox container '{}' sets PATH in env; use the 'path' field instead",
                name
            );
        }

        for flag in &self.additional_flags {
            if flag_sets_path(flag) {
                bail!(
                    "Distrobox container '{}' sets PATH in additional_flags; use the 'path' field instead",
                    name
                );
            }
        }

        if !self.bins.is_empty() {
            let from_expanded: Vec<String> =
                self.bins.from.iter().map(|p| expand_home(p)).collect();
            for also in &self.bins.also {
                let also_expanded = expand_home(also);
                for from in &from_expanded {
                    if path_is_within(&also_expanded, from) {
                        bail!(
                            "Distrobox container '{}' bins.also entry '{}' is inside bins.from directory '{}'",
                            name,
                            also,
                            collapse_home(from)
                        );
                    }
                }
            }
        }

        Ok(())
    }
}

fn path_is_within(child: &str, parent: &str) -> bool {
    let child_path = Path::new(child);
    let parent_path = Path::new(parent);
    child_path.starts_with(parent_path)
}

fn flag_sets_path(flag: &str) -> bool {
    let trimmed = flag.trim();
    trimmed.starts_with("--env=PATH=")
        || trimmed.starts_with("--env PATH=")
        || trimmed.starts_with("-e PATH=")
        || trimmed.starts_with("-e=PATH=")
}

fn expand_home(value: &str) -> String {
    if let Some(rest) = value.strip_prefix("~/")
        && let Ok(home) = std::env::var("HOME")
    {
        return format!("{}/{}", home, rest);
    }
    value.to_string()
}

fn collapse_home(value: &str) -> String {
    if let Ok(home) = std::env::var("HOME") {
        let prefix = format!("{}/", home);
        if let Some(rest) = value.strip_prefix(&prefix) {
            return format!("~/{}", rest);
        }
        if value == home {
            return "~".to_string();
        }
    }
    value.to_string()
}
