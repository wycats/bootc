//! Ephemeral manifest for tracking local-only changes.
//!
//! When `--local` is used, changes are recorded in an ephemeral manifest at
//! `~/.local/share/bkt/ephemeral.json`. This manifest:
//!
//! 1. **Tracks all local-only changes** since last reboot
//! 2. **Invalidates on reboot** (using `/proc/sys/kernel/random/boot_id`)
//! 3. **Enables later promotion** to a proper PR via `bkt local commit`
//!
//! # Boot ID Validation
//!
//! On each `bkt` invocation, the current boot ID is compared against the cached
//! `boot_id`. If they differ, the ephemeral manifest is automatically cleared
//! because local-only changes (especially rpm-ostree overlays) don't survive
//! reboots or image switches.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use directories::BaseDirs;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use crate::subsystem::SubsystemRegistry;

/// The domain of a change (which subsystem it affects).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ChangeDomain {
    /// Flatpak application
    Flatpak,
    /// GNOME Shell extension
    Extension,
    /// GSettings preference
    Gsetting,
    /// Host shim
    Shim,
    /// DNF/RPM package (via rpm-ostree overlay)
    Dnf,
    /// AppImage application (via GearLever)
    AppImage,
}

impl std::fmt::Display for ChangeDomain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChangeDomain::Flatpak => write!(f, "flatpak"),
            ChangeDomain::Extension => write!(f, "extension"),
            ChangeDomain::Gsetting => write!(f, "gsetting"),
            ChangeDomain::Shim => write!(f, "shim"),
            ChangeDomain::Dnf => write!(f, "dnf"),
            ChangeDomain::AppImage => write!(f, "appimage"),
        }
    }
}

impl ChangeDomain {
    /// Return the subsystem registry ID for this change domain.
    pub fn subsystem_id(&self) -> &'static str {
        match self {
            ChangeDomain::Flatpak => "flatpak",
            ChangeDomain::Extension => "extension",
            ChangeDomain::Gsetting => "gsetting",
            ChangeDomain::Shim => "shim",
            ChangeDomain::Dnf => "system",
            ChangeDomain::AppImage => "appimage",
        }
    }

    /// Check if this domain maps to a registered subsystem.
    #[allow(dead_code)]
    pub fn is_registered(&self) -> bool {
        SubsystemRegistry::builtin()
            .find(self.subsystem_id())
            .is_some()
    }
}

/// The action performed on a resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ChangeAction {
    /// Resource was added
    Add,
    /// Resource was removed
    Remove,
    /// Resource was updated/modified
    Update,
}

impl std::fmt::Display for ChangeAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChangeAction::Add => write!(f, "add"),
            ChangeAction::Remove => write!(f, "remove"),
            ChangeAction::Update => write!(f, "update"),
        }
    }
}

/// A single tracked change in the ephemeral manifest.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EphemeralChange {
    /// Which subsystem this change affects
    pub domain: ChangeDomain,
    /// What action was performed
    pub action: ChangeAction,
    /// Primary identifier (e.g., app ID, package name, schema.key)
    pub identifier: String,
    /// When the change was made
    pub timestamp: DateTime<Utc>,
    /// Domain-specific metadata
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, String>,
}

impl EphemeralChange {
    /// Create a new ephemeral change with the current timestamp.
    pub fn new(domain: ChangeDomain, action: ChangeAction, identifier: impl Into<String>) -> Self {
        Self {
            domain,
            action,
            identifier: identifier.into(),
            timestamp: Utc::now(),
            metadata: HashMap::new(),
        }
    }

    /// Add metadata to this change.
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Create a unique key for deduplication (domain + identifier).
    ///
    /// Multiple actions on the same resource collapse to the latest action.
    pub fn dedup_key(&self) -> String {
        format!("{}:{}", self.domain, self.identifier)
    }

    /// Check if this change is an "add" action.
    pub fn is_add(&self) -> bool {
        self.action == ChangeAction::Add
    }

    /// Check if this change is a "remove" action.
    pub fn is_remove(&self) -> bool {
        self.action == ChangeAction::Remove
    }
}

/// The ephemeral manifest tracking local-only changes.
///
/// This manifest is stored at `~/.local/share/bkt/ephemeral.json` and is
/// automatically invalidated when the boot ID changes.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct EphemeralManifest {
    /// Schema reference for tooling
    #[serde(rename = "$schema", skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,

    /// Boot ID when this manifest was created.
    ///
    /// If the current boot ID differs, the manifest is stale and should be cleared.
    #[serde(default)]
    pub boot_id: String,

    /// List of tracked changes.
    ///
    /// Changes are deduplicated by `domain:identifier` - later actions on the
    /// same resource replace earlier ones.
    #[serde(default)]
    pub changes: Vec<EphemeralChange>,
}

impl EphemeralManifest {
    /// Path to the ephemeral manifest file.
    pub fn path() -> PathBuf {
        // Prefer $HOME for test isolation, fall back to BaseDirs
        let data_dir = std::env::var("HOME")
            .ok()
            .map(|h| PathBuf::from(h).join(".local/share"))
            .or_else(|| BaseDirs::new().map(|d| d.data_local_dir().to_path_buf()))
            .unwrap_or_else(|| PathBuf::from(".local/share"));
        data_dir.join("bkt").join("ephemeral.json")
    }

    /// Read the current boot ID from the kernel.
    ///
    /// On Linux, this uses `/proc/sys/kernel/random/boot_id`, which changes on
    /// every reboot and provides a reliable way to detect whether local-only
    /// changes are still valid.
    ///
    /// On non-Linux platforms, `/proc` is not available. In that case we
    /// return a stable placeholder value so that callers can continue to
    /// function, but reboot detection is effectively disabled.
    pub fn current_boot_id() -> Result<String> {
        #[cfg(target_os = "linux")]
        {
            fs::read_to_string("/proc/sys/kernel/random/boot_id")
                .map(|s| s.trim().to_string())
                .context("Failed to read boot ID from /proc/sys/kernel/random/boot_id")
        }

        #[cfg(not(target_os = "linux"))]
        {
            // Non-Linux platforms do not expose a Linux-style boot ID via /proc.
            // We degrade gracefully by returning a constant placeholder.
            Ok("non-linux-boot-id-unsupported".to_string())
        }
    }

    /// Load the ephemeral manifest from disk.
    ///
    /// Returns an empty manifest if the file doesn't exist.
    pub fn load() -> Result<Self> {
        let path = Self::path();
        if !path.exists() {
            return Ok(Self::default());
        }

        let content = fs::read_to_string(&path).with_context(|| {
            format!("Failed to read ephemeral manifest from {}", path.display())
        })?;

        serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse ephemeral manifest from {}", path.display()))
    }

    /// Load and validate the ephemeral manifest.
    ///
    /// If the boot ID has changed since the manifest was created, the manifest
    /// is cleared automatically because local-only changes don't survive reboots.
    pub fn load_validated() -> Result<Self> {
        let mut manifest = Self::load()?;
        let current_boot_id = Self::current_boot_id()?;

        if !manifest.boot_id.is_empty() && manifest.boot_id != current_boot_id {
            // Boot ID changed - clear the manifest
            tracing::info!(
                old_boot_id = %manifest.boot_id,
                new_boot_id = %current_boot_id,
                "Boot ID changed, clearing ephemeral manifest"
            );
            manifest = Self::new_with_boot_id(current_boot_id);
            manifest.save()?;
        } else if manifest.boot_id.is_empty() {
            // Initialize boot ID on first use and save to ensure consistency
            manifest.boot_id = current_boot_id;
            manifest.save()?;
            manifest.save()?;
        }

        Ok(manifest)
    }

    /// Create a new manifest with the specified boot ID.
    fn new_with_boot_id(boot_id: String) -> Self {
        Self {
            schema: None,
            boot_id,
            changes: Vec::new(),
        }
    }

    /// Save the ephemeral manifest to disk.
    pub fn save(&self) -> Result<()> {
        let path = Self::path();

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory {}", parent.display()))?;
        }

        let content =
            serde_json::to_string_pretty(self).context("Failed to serialize ephemeral manifest")?;

        fs::write(&path, content)
            .with_context(|| format!("Failed to write ephemeral manifest to {}", path.display()))?;

        Ok(())
    }

    /// Check if the manifest has any changes.
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    /// Get the count of changes.
    pub fn len(&self) -> usize {
        self.changes.len()
    }

    /// Add or update a change in the manifest.
    ///
    /// If a change with the same `domain:identifier` already exists, it is
    /// normally replaced with the new change so we track only the latest
    /// state of each resource.
    ///
    /// However, if the existing and new changes are inverse operations
    /// (e.g. Add followed by Remove, or Remove followed by Add), they
    /// cancel each other out and the entry is removed entirely. This
    /// avoids tracking no-op net changes that would result in
    /// meaningless PRs (such as removing a package that was never
    /// present in the system manifest).
    pub fn record(&mut self, change: EphemeralChange) {
        let key = change.dedup_key();

        // Look for an existing change with the same key
        if let Some(existing_idx) = self.changes.iter().position(|c| c.dedup_key() == key) {
            let existing = &self.changes[existing_idx];

            // If the existing and new changes are inverse operations,
            // they cancel out and we remove the entry entirely.
            let cancels_out = (existing.is_add() && change.is_remove())
                || (existing.is_remove() && change.is_add());

            if cancels_out {
                // Remove the existing change and do not add a new one,
                // leaving no tracked change for this key.
                self.changes.remove(existing_idx);
                return;
            } else {
                // Otherwise, replace the existing change with the new one.
                self.changes[existing_idx] = change;
                return;
            }
        }

        // No existing change with this key; record the new change.
        self.changes.push(change);
    }

    /// Remove a change from the manifest by domain and identifier.
    #[allow(dead_code)] // Part of public API, will be used by `bkt local` commands
    pub fn remove(&mut self, domain: ChangeDomain, identifier: &str) -> bool {
        let key = format!("{}:{}", domain, identifier);
        let original_len = self.changes.len();
        self.changes.retain(|c| c.dedup_key() != key);
        self.changes.len() < original_len
    }

    /// Clear all changes from the manifest.
    #[allow(dead_code)] // Part of public API, will be used by `bkt local clear`
    pub fn clear(&mut self) {
        self.changes.clear();
    }

    /// Get all changes for a specific domain.
    #[allow(dead_code)] // Part of public API, will be used by domain filtering
    pub fn changes_for_domain(&self, domain: ChangeDomain) -> Vec<&EphemeralChange> {
        self.changes.iter().filter(|c| c.domain == domain).collect()
    }

    /// Get changes grouped by domain.
    pub fn changes_by_domain(&self) -> HashMap<ChangeDomain, Vec<&EphemeralChange>> {
        let mut grouped: HashMap<ChangeDomain, Vec<&EphemeralChange>> = HashMap::new();
        for change in &self.changes {
            grouped.entry(change.domain).or_default().push(change);
        }
        grouped
    }

    /// Delete the ephemeral manifest file from disk.
    pub fn delete_file() -> Result<()> {
        let path = Self::path();
        if path.exists() {
            fs::remove_file(&path).with_context(|| {
                format!("Failed to delete ephemeral manifest at {}", path.display())
            })?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_change_dedup_key() {
        let change = EphemeralChange::new(
            ChangeDomain::Flatpak,
            ChangeAction::Add,
            "org.gnome.Calculator",
        );
        assert_eq!(change.dedup_key(), "flatpak:org.gnome.Calculator");
    }

    #[test]
    fn test_manifest_record_deduplicates() {
        let mut manifest = EphemeralManifest::default();

        // Add a change
        manifest.record(EphemeralChange::new(
            ChangeDomain::Flatpak,
            ChangeAction::Add,
            "org.gnome.Calculator",
        ));
        assert_eq!(manifest.len(), 1);

        // Add same resource with inverse action - should cancel out (net zero change)
        manifest.record(EphemeralChange::new(
            ChangeDomain::Flatpak,
            ChangeAction::Remove,
            "org.gnome.Calculator",
        ));
        assert_eq!(
            manifest.len(),
            0,
            "Inverse operations (Add + Remove) should cancel out"
        );
    }

    #[test]
    fn test_manifest_record_replaces_same_action() {
        let mut manifest = EphemeralManifest::default();

        // Add a change
        manifest.record(EphemeralChange::new(
            ChangeDomain::Gsetting,
            ChangeAction::Update,
            "org.gnome.desktop.interface.color-scheme",
        ));
        assert_eq!(manifest.len(), 1);

        // Update same resource with same action - should replace (not cancel)
        manifest.record(EphemeralChange::new(
            ChangeDomain::Gsetting,
            ChangeAction::Update,
            "org.gnome.desktop.interface.color-scheme",
        ));
        assert_eq!(manifest.len(), 1, "Same action should replace, not add");
    }

    #[test]
    fn test_manifest_record_different_resources() {
        let mut manifest = EphemeralManifest::default();

        manifest.record(EphemeralChange::new(
            ChangeDomain::Flatpak,
            ChangeAction::Add,
            "org.gnome.Calculator",
        ));
        manifest.record(EphemeralChange::new(
            ChangeDomain::Flatpak,
            ChangeAction::Add,
            "org.gnome.TextEditor",
        ));
        assert_eq!(manifest.len(), 2);
    }

    #[test]
    fn test_manifest_changes_by_domain() {
        let mut manifest = EphemeralManifest::default();

        manifest.record(EphemeralChange::new(
            ChangeDomain::Flatpak,
            ChangeAction::Add,
            "org.gnome.Calculator",
        ));
        manifest.record(EphemeralChange::new(
            ChangeDomain::Dnf,
            ChangeAction::Add,
            "htop",
        ));
        manifest.record(EphemeralChange::new(
            ChangeDomain::Flatpak,
            ChangeAction::Add,
            "org.gnome.TextEditor",
        ));

        let by_domain = manifest.changes_by_domain();
        assert_eq!(by_domain.get(&ChangeDomain::Flatpak).unwrap().len(), 2);
        assert_eq!(by_domain.get(&ChangeDomain::Dnf).unwrap().len(), 1);
    }

    #[test]
    fn test_manifest_remove() {
        let mut manifest = EphemeralManifest::default();

        manifest.record(EphemeralChange::new(
            ChangeDomain::Flatpak,
            ChangeAction::Add,
            "org.gnome.Calculator",
        ));
        assert_eq!(manifest.len(), 1);

        let removed = manifest.remove(ChangeDomain::Flatpak, "org.gnome.Calculator");
        assert!(removed);
        assert!(manifest.is_empty());
    }

    #[test]
    fn test_manifest_serialization_roundtrip() {
        let mut manifest = EphemeralManifest::default();
        manifest.boot_id = "test-boot-id".to_string();
        manifest.record(
            EphemeralChange::new(
                ChangeDomain::Flatpak,
                ChangeAction::Add,
                "org.gnome.Calculator",
            )
            .with_metadata("remote", "flathub"),
        );

        let json = serde_json::to_string_pretty(&manifest).unwrap();
        let parsed: EphemeralManifest = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.boot_id, manifest.boot_id);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed.changes[0].identifier, "org.gnome.Calculator");
        assert_eq!(
            parsed.changes[0].metadata.get("remote"),
            Some(&"flathub".to_string())
        );
    }

    #[test]
    fn test_change_domain_display() {
        assert_eq!(ChangeDomain::Flatpak.to_string(), "flatpak");
        assert_eq!(ChangeDomain::Dnf.to_string(), "dnf");
        assert_eq!(ChangeDomain::Gsetting.to_string(), "gsetting");
    }

    #[test]
    fn test_change_action_display() {
        assert_eq!(ChangeAction::Add.to_string(), "add");
        assert_eq!(ChangeAction::Remove.to_string(), "remove");
        assert_eq!(ChangeAction::Update.to_string(), "update");
    }

    #[test]
    fn test_change_domain_subsystem_mapping_is_registered() {
        for domain in [
            ChangeDomain::Flatpak,
            ChangeDomain::Extension,
            ChangeDomain::Gsetting,
            ChangeDomain::Shim,
            ChangeDomain::Dnf,
            ChangeDomain::AppImage,
        ] {
            assert!(
                domain.is_registered(),
                "ChangeDomain::{:?} maps to unregistered subsystem '{}'",
                domain,
                domain.subsystem_id()
            );
        }
    }
}
