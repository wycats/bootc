//! Changelog manifest types for tracking distribution changes.
//!
//! This module provides types for the changelog system as specified in RFC-0005.
//! Changelog entries are stored as YAML files in `.changelog/pending/` and
//! `.changelog/versions/` directories.

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// The type of change being recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ChangeType {
    /// Something was added
    Added,
    /// Something was changed/updated
    Changed,
    /// Something was removed
    Removed,
    /// Something was fixed
    Fixed,
    /// Security-related change
    Security,
    /// Deprecated functionality
    Deprecated,
}

impl fmt::Display for ChangeType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ChangeType::Added => write!(f, "Added"),
            ChangeType::Changed => write!(f, "Changed"),
            ChangeType::Removed => write!(f, "Removed"),
            ChangeType::Fixed => write!(f, "Fixed"),
            ChangeType::Security => write!(f, "Security"),
            ChangeType::Deprecated => write!(f, "Deprecated"),
        }
    }
}

/// The category of change (which manifest type was affected).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ChangeCategory {
    /// Flatpak application change
    Flatpak,
    /// Flatpak remote change
    FlatpakRemote,
    /// System RPM package change
    Package,
    /// Toolbox package change
    ToolboxPackage,
    /// GNOME extension change
    Extension,
    /// GSettings change
    Gsetting,
    /// Host shim change
    Shim,
    /// Upstream dependency change
    Upstream,
    /// COPR repository change
    Copr,
    /// System configuration change
    System,
    /// Other/miscellaneous change
    Other,
}

impl fmt::Display for ChangeCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ChangeCategory::Flatpak => write!(f, "Flatpak"),
            ChangeCategory::FlatpakRemote => write!(f, "Flatpak Remote"),
            ChangeCategory::Package => write!(f, "Package"),
            ChangeCategory::ToolboxPackage => write!(f, "Toolbox Package"),
            ChangeCategory::Extension => write!(f, "Extension"),
            ChangeCategory::Gsetting => write!(f, "GSettings"),
            ChangeCategory::Shim => write!(f, "Shim"),
            ChangeCategory::Upstream => write!(f, "Upstream"),
            ChangeCategory::Copr => write!(f, "COPR"),
            ChangeCategory::System => write!(f, "System"),
            ChangeCategory::Other => write!(f, "Other"),
        }
    }
}

/// A single changelog entry, stored in `.changelog/pending/` or within a version file.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ChangelogEntry {
    /// When this change was made
    pub timestamp: DateTime<Utc>,

    /// The type of change
    #[serde(rename = "type")]
    pub change_type: ChangeType,

    /// The category of change
    pub category: ChangeCategory,

    /// Human-readable description of the change
    pub message: String,

    /// The bkt command that triggered this change (if any)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,

    /// The PR number this change was merged in (set when PR is created/merged)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pr: Option<u32>,

    /// Whether this is a draft entry (drafts cannot be merged)
    #[serde(default)]
    pub draft: bool,
}

impl ChangelogEntry {
    /// Create a new changelog entry with the current timestamp.
    pub fn new(
        change_type: ChangeType,
        category: ChangeCategory,
        message: impl Into<String>,
    ) -> Self {
        Self {
            timestamp: Utc::now(),
            change_type,
            category,
            message: message.into(),
            command: None,
            pr: None,
            draft: false,
        }
    }

    /// Set the command that triggered this change.
    #[allow(dead_code)]
    #[must_use]
    pub fn with_command(mut self, command: impl Into<String>) -> Self {
        self.command = Some(command.into());
        self
    }

    /// Set the PR number.
    #[allow(dead_code)]
    #[must_use]
    pub fn with_pr(mut self, pr: u32) -> Self {
        self.pr = Some(pr);
        self
    }

    /// Mark this entry as a draft.
    #[must_use]
    pub fn into_draft(mut self) -> Self {
        self.draft = true;
        self
    }

    /// Format for display in changelog.
    pub fn format_for_changelog(&self) -> String {
        let pr_ref = self.pr.map(|n| format!(" (PR #{})", n)).unwrap_or_default();
        format!("- {}: {}{}", self.category, self.message, pr_ref)
    }
}

/// Metadata for a released version.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct VersionMetadata {
    /// The version string (YYYY.MM.DD.N format)
    pub version: String,

    /// The date this version was released
    pub date: DateTime<Utc>,

    /// The container image digest for this version
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_digest: Option<String>,

    /// The base image this version was built from
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_image: Option<String>,

    /// All changes included in this version
    pub changes: Vec<ChangelogEntry>,
}

impl VersionMetadata {
    /// Create a new version with the current date.
    pub fn new(version: impl Into<String>) -> Self {
        Self {
            version: version.into(),
            date: Utc::now(),
            image_digest: None,
            base_image: None,
            changes: Vec::new(),
        }
    }

    /// Generate the next version number for today.
    pub fn next_version_for_today() -> String {
        let today = Utc::now().format("%Y.%m.%d");
        // In a real implementation, we'd check existing versions and increment N
        format!("{}.1", today)
    }

    /// Format for CHANGELOG.md output.
    pub fn format_for_changelog(&self) -> String {
        let mut output = String::new();
        output.push_str(&format!(
            "## [{}] - {}\n\n",
            self.version,
            self.date.format("%Y-%m-%d")
        ));

        // Group changes by type
        let mut added: Vec<&ChangelogEntry> = Vec::new();
        let mut changed: Vec<&ChangelogEntry> = Vec::new();
        let mut removed: Vec<&ChangelogEntry> = Vec::new();
        let mut fixed: Vec<&ChangelogEntry> = Vec::new();
        let mut security: Vec<&ChangelogEntry> = Vec::new();
        let mut deprecated: Vec<&ChangelogEntry> = Vec::new();

        for entry in &self.changes {
            match entry.change_type {
                ChangeType::Added => added.push(entry),
                ChangeType::Changed => changed.push(entry),
                ChangeType::Removed => removed.push(entry),
                ChangeType::Fixed => fixed.push(entry),
                ChangeType::Security => security.push(entry),
                ChangeType::Deprecated => deprecated.push(entry),
            }
        }

        // Output each section
        for (title, entries) in [
            ("Added", added),
            ("Changed", changed),
            ("Removed", removed),
            ("Fixed", fixed),
            ("Security", security),
            ("Deprecated", deprecated),
        ] {
            if !entries.is_empty() {
                output.push_str(&format!("### {}\n", title));
                for entry in entries {
                    output.push_str(&entry.format_for_changelog());
                    output.push('\n');
                }
                output.push('\n');
            }
        }

        output
    }
}

/// Manager for changelog operations.
pub struct ChangelogManager {
    /// Root directory of the repository
    root: PathBuf,
}

impl ChangelogManager {
    /// Create a new changelog manager.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Get the pending directory path.
    pub fn pending_dir(&self) -> PathBuf {
        self.root.join(".changelog").join("pending")
    }

    /// Get the versions directory path.
    pub fn versions_dir(&self) -> PathBuf {
        self.root.join(".changelog").join("versions")
    }

    /// Get the CHANGELOG.md path.
    pub fn changelog_file(&self) -> PathBuf {
        self.root.join("CHANGELOG.md")
    }

    /// Ensure the changelog directories exist.
    pub fn ensure_dirs(&self) -> Result<()> {
        fs::create_dir_all(self.pending_dir())
            .context("Failed to create .changelog/pending directory")?;
        fs::create_dir_all(self.versions_dir())
            .context("Failed to create .changelog/versions directory")?;
        Ok(())
    }

    /// Add a pending changelog entry.
    pub fn add_pending(&self, entry: &ChangelogEntry) -> Result<PathBuf> {
        self.ensure_dirs()?;

        // Generate a unique filename
        let timestamp = entry.timestamp.format("%Y%m%d%H%M%S");
        let category = format!("{:?}", entry.category).to_lowercase();
        let filename = format!("{}-{}.yaml", timestamp, category);
        let path = self.pending_dir().join(&filename);

        let content =
            serde_yaml::to_string(entry).context("Failed to serialize changelog entry")?;

        fs::write(&path, content).with_context(|| format!("Failed to write {}", path.display()))?;

        Ok(path)
    }

    /// Load all pending changelog entries.
    pub fn load_pending(&self) -> Result<Vec<ChangelogEntry>> {
        let pending_dir = self.pending_dir();
        if !pending_dir.exists() {
            return Ok(Vec::new());
        }

        let mut entries = Vec::new();
        for entry in fs::read_dir(&pending_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "yaml") {
                let content = fs::read_to_string(&path)
                    .with_context(|| format!("Failed to read {}", path.display()))?;
                let changelog_entry: ChangelogEntry = serde_yaml::from_str(&content)
                    .with_context(|| format!("Failed to parse {}", path.display()))?;
                entries.push(changelog_entry);
            }
        }

        // Sort by timestamp
        entries.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));

        Ok(entries)
    }

    /// Load a specific version's metadata.
    pub fn load_version(&self, version: &str) -> Result<Option<VersionMetadata>> {
        let path = self.versions_dir().join(format!("{}.yaml", version));
        if !path.exists() {
            return Ok(None);
        }

        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let metadata: VersionMetadata = serde_yaml::from_str(&content)
            .with_context(|| format!("Failed to parse {}", path.display()))?;

        Ok(Some(metadata))
    }

    /// List all released versions.
    pub fn list_versions(&self) -> Result<Vec<String>> {
        let versions_dir = self.versions_dir();
        if !versions_dir.exists() {
            return Ok(Vec::new());
        }

        let mut versions = Vec::new();
        for entry in fs::read_dir(&versions_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "yaml")
                && let Some(stem) = path.file_stem()
            {
                versions.push(stem.to_string_lossy().to_string());
            }
        }

        // Sort versions in reverse chronological order
        versions.sort_by(|a, b| b.cmp(a));

        Ok(versions)
    }

    /// Create a new version from pending entries.
    pub fn create_version(&self, version: &str) -> Result<VersionMetadata> {
        let entries = self.load_pending()?;

        let mut metadata = VersionMetadata::new(version);
        metadata.changes = entries;

        // Save version metadata
        let path = self.versions_dir().join(format!("{}.yaml", version));
        let content =
            serde_yaml::to_string(&metadata).context("Failed to serialize version metadata")?;
        fs::write(&path, content).with_context(|| format!("Failed to write {}", path.display()))?;

        // Clear pending entries
        self.clear_pending()?;

        Ok(metadata)
    }

    /// Clear all pending entries.
    pub fn clear_pending(&self) -> Result<()> {
        let pending_dir = self.pending_dir();
        if !pending_dir.exists() {
            return Ok(());
        }

        for entry in fs::read_dir(&pending_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "yaml") {
                fs::remove_file(&path)
                    .with_context(|| format!("Failed to remove {}", path.display()))?;
            }
        }

        Ok(())
    }

    /// Check if any pending entries are drafts.
    pub fn has_draft_entries(&self) -> Result<bool> {
        let entries = self.load_pending()?;
        Ok(entries.iter().any(|e| e.draft))
    }

    /// Get the count of pending entries.
    pub fn pending_count(&self) -> Result<usize> {
        Ok(self.load_pending()?.len())
    }

    /// Update CHANGELOG.md with a new version.
    pub fn update_changelog_file(&self, version: &VersionMetadata) -> Result<()> {
        let changelog_path = self.changelog_file();
        let version_content = version.format_for_changelog();

        let existing = if changelog_path.exists() {
            fs::read_to_string(&changelog_path)
                .with_context(|| format!("Failed to read {}", changelog_path.display()))?
        } else {
            String::from(
                "# Changelog\n\nAll notable changes to this distribution are documented here.\n\n",
            )
        };

        // Insert new version after the header
        let new_content = if let Some(pos) = existing.find("\n## [") {
            format!(
                "{}{}\n{}",
                &existing[..pos + 1],
                version_content,
                &existing[pos + 1..]
            )
        } else {
            format!("{}{}", existing, version_content)
        };

        fs::write(&changelog_path, new_content)
            .with_context(|| format!("Failed to write {}", changelog_path.display()))?;

        Ok(())
    }
}

/// Find the repository root by looking for .git directory.
pub fn find_repo_root(start: &Path) -> Option<PathBuf> {
    let mut current = start.to_path_buf();
    loop {
        if current.join(".git").exists() {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_changelog_entry_creation() {
        let entry = ChangelogEntry::new(
            ChangeType::Added,
            ChangeCategory::Flatpak,
            "org.gnome.Calculator",
        )
        .with_command("bkt flatpak add org.gnome.Calculator");

        assert_eq!(entry.change_type, ChangeType::Added);
        assert_eq!(entry.category, ChangeCategory::Flatpak);
        assert_eq!(entry.message, "org.gnome.Calculator");
        assert!(entry.command.is_some());
        assert!(!entry.draft);
    }

    #[test]
    fn test_changelog_entry_formatting() {
        let entry =
            ChangelogEntry::new(ChangeType::Added, ChangeCategory::Package, "htop").with_pr(42);

        let formatted = entry.format_for_changelog();
        assert!(formatted.contains("Package: htop"));
        assert!(formatted.contains("(PR #42)"));
    }

    #[test]
    fn test_version_formatting() {
        let mut version = VersionMetadata::new("2025.01.02.1");
        version.changes.push(ChangelogEntry::new(
            ChangeType::Added,
            ChangeCategory::Flatpak,
            "org.gnome.Calculator",
        ));
        version.changes.push(ChangelogEntry::new(
            ChangeType::Removed,
            ChangeCategory::Package,
            "nano",
        ));

        let formatted = version.format_for_changelog();
        assert!(formatted.contains("## [2025.01.02.1]"));
        assert!(formatted.contains("### Added"));
        assert!(formatted.contains("### Removed"));
    }

    #[test]
    fn test_changelog_manager_pending() -> Result<()> {
        let temp = TempDir::new()?;
        let manager = ChangelogManager::new(temp.path());

        let entry = ChangelogEntry::new(
            ChangeType::Added,
            ChangeCategory::Flatpak,
            "org.gnome.Calculator",
        );

        manager.add_pending(&entry)?;

        let pending = manager.load_pending()?;
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].message, "org.gnome.Calculator");

        Ok(())
    }

    #[test]
    fn test_changelog_manager_version() -> Result<()> {
        let temp = TempDir::new()?;
        let manager = ChangelogManager::new(temp.path());

        // Add some pending entries
        manager.add_pending(&ChangelogEntry::new(
            ChangeType::Added,
            ChangeCategory::Flatpak,
            "org.gnome.Calculator",
        ))?;

        // Create version
        let version = manager.create_version("2025.01.02.1")?;
        assert_eq!(version.changes.len(), 1);

        // Pending should be empty
        assert_eq!(manager.pending_count()?, 0);

        // Version should be loadable
        let loaded = manager.load_version("2025.01.02.1")?;
        assert!(loaded.is_some());

        Ok(())
    }
}
