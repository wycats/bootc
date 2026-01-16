//! Build info types for container build descriptions.
//!
//! This module implements the schema defined in RFC-0013 for tracking
//! what changed between container builds.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::diff::{ChangedItem, DiffResult};
use super::parsers::SemanticDiff;
use super::{AppImageApp, ExtensionItem, FlatpakApp, FlatpakRemote, GSetting, Shim};

/// Schema version for forward compatibility.
pub const SCHEMA_VERSION: &str = "1.0.0";

/// Complete build info document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildInfo {
    /// Schema version for forward compatibility
    pub schema_version: String,
    /// Build metadata
    pub build: BuildMetadata,
    /// Manifest diffs
    pub manifests: ManifestDiffs,
    /// System config diffs (Phase 3)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_config: Option<SystemConfigDiffs>,
    /// Upstream changes (Phase 2)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream: Option<UpstreamChanges>,
    /// Provenance entries (Phase 4)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub provenance: Vec<ProvenanceEntry>,
}

impl BuildInfo {
    /// Create a new BuildInfo with the current schema version.
    pub fn new(build: BuildMetadata, manifests: ManifestDiffs) -> Self {
        Self {
            schema_version: SCHEMA_VERSION.to_string(),
            build,
            manifests,
            system_config: None,
            upstream: None,
            provenance: vec![],
        }
    }

    /// Returns true if there are no changes.
    pub fn is_empty(&self) -> bool {
        self.manifests.is_empty()
            && self.system_config.as_ref().is_none_or(|s| s.is_empty())
            && self.upstream.is_none()
            && self.provenance.is_empty()
    }
}

/// Build metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildMetadata {
    /// Current commit hash
    pub commit: String,
    /// Build timestamp
    pub timestamp: DateTime<Utc>,
    /// Previous commit hash (if available)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_commit: Option<String>,
}

/// Container for all manifest diffs.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ManifestDiffs {
    /// Flatpak apps diff
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flatpak_apps: Option<DiffResult<FlatpakAppDiff>>,
    /// Flatpak remotes diff
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flatpak_remotes: Option<DiffResult<FlatpakRemoteDiff>>,
    /// System packages diff
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_packages: Option<DiffResult<String>>,
    /// Toolbox packages diff
    #[serde(skip_serializing_if = "Option::is_none")]
    pub toolbox_packages: Option<DiffResult<String>>,
    /// GNOME extensions diff
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gnome_extensions: Option<DiffResult<ExtensionDiff>>,
    /// GSettings diff
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gsettings: Option<DiffResult<GSettingDiff>>,
    /// Host shims diff
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host_shims: Option<DiffResult<ShimDiff>>,
    /// AppImage apps diff
    #[serde(skip_serializing_if = "Option::is_none")]
    pub appimage_apps: Option<DiffResult<AppImageDiff>>,
}

impl ManifestDiffs {
    /// Returns true if all diffs are empty or None.
    pub fn is_empty(&self) -> bool {
        self.flatpak_apps.as_ref().is_none_or(|d| d.is_empty())
            && self.flatpak_remotes.as_ref().is_none_or(|d| d.is_empty())
            && self.system_packages.as_ref().is_none_or(|d| d.is_empty())
            && self.toolbox_packages.as_ref().is_none_or(|d| d.is_empty())
            && self.gnome_extensions.as_ref().is_none_or(|d| d.is_empty())
            && self.gsettings.as_ref().is_none_or(|d| d.is_empty())
            && self.host_shims.as_ref().is_none_or(|d| d.is_empty())
            && self.appimage_apps.as_ref().is_none_or(|d| d.is_empty())
    }
}

// ============================================================================
// Diff entry types for each manifest type
// ============================================================================

/// Flatpak app diff entry.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FlatpakAppDiff {
    pub id: String,
    pub remote: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

impl From<&FlatpakApp> for FlatpakAppDiff {
    fn from(app: &FlatpakApp) -> Self {
        Self {
            id: app.id.clone(),
            remote: app.remote.clone(),
            scope: Some(app.scope.to_string()),
        }
    }
}

impl From<FlatpakApp> for FlatpakAppDiff {
    fn from(app: FlatpakApp) -> Self {
        Self::from(&app)
    }
}

/// Flatpak remote diff entry.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FlatpakRemoteDiff {
    pub name: String,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

impl From<&FlatpakRemote> for FlatpakRemoteDiff {
    fn from(remote: &FlatpakRemote) -> Self {
        Self {
            name: remote.name.clone(),
            url: remote.url.clone(),
            scope: Some(remote.scope.to_string()),
        }
    }
}

impl From<FlatpakRemote> for FlatpakRemoteDiff {
    fn from(remote: FlatpakRemote) -> Self {
        Self::from(&remote)
    }
}

/// GNOME extension diff entry.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExtensionDiff {
    pub id: String,
    pub enabled: bool,
}

impl From<&ExtensionItem> for ExtensionDiff {
    fn from(ext: &ExtensionItem) -> Self {
        Self {
            id: ext.id().to_string(),
            enabled: ext.enabled(),
        }
    }
}

impl From<ExtensionItem> for ExtensionDiff {
    fn from(ext: ExtensionItem) -> Self {
        Self::from(&ext)
    }
}

/// GSettings diff entry.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GSettingDiff {
    pub schema: String,
    pub key: String,
    pub value: String,
}

impl From<&GSetting> for GSettingDiff {
    fn from(setting: &GSetting) -> Self {
        Self {
            schema: setting.schema.clone(),
            key: setting.key.clone(),
            value: setting.value.clone(),
        }
    }
}

impl From<GSetting> for GSettingDiff {
    fn from(setting: GSetting) -> Self {
        Self::from(&setting)
    }
}

/// Host shim diff entry.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ShimDiff {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
}

impl From<&Shim> for ShimDiff {
    fn from(shim: &Shim) -> Self {
        Self {
            name: shim.name.clone(),
            host: shim.host.clone(),
        }
    }
}

impl From<Shim> for ShimDiff {
    fn from(shim: Shim) -> Self {
        Self::from(&shim)
    }
}

/// AppImage app diff entry.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AppImageDiff {
    pub name: String,
    pub repo: String,
}

impl From<&AppImageApp> for AppImageDiff {
    fn from(app: &AppImageApp) -> Self {
        Self {
            name: app.name.clone(),
            repo: app.repo.clone(),
        }
    }
}

impl From<AppImageApp> for AppImageDiff {
    fn from(app: AppImageApp) -> Self {
        Self::from(&app)
    }
}

// ============================================================================
// Helper to convert DiffResult<T> to DiffResult<D> where T -> D
// ============================================================================

/// Convert a DiffResult of one type to another via From trait.
pub fn convert_diff_result<T, D>(result: DiffResult<T>) -> DiffResult<D>
where
    D: From<T>,
{
    DiffResult {
        added: result.added.into_iter().map(D::from).collect(),
        removed: result.removed.into_iter().map(D::from).collect(),
        changed: result
            .changed
            .into_iter()
            .map(|c| ChangedItem {
                from: D::from(c.from),
                to: D::from(c.to),
            })
            .collect(),
    }
}

// ============================================================================
// Placeholder types for future phases
// ============================================================================

/// System configuration diffs (Phase 3).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SystemConfigDiffs {
    /// Added files
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub added: Vec<SystemConfigEntry>,
    /// Removed files
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub removed: Vec<SystemConfigEntry>,
    /// Modified files
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modified: Vec<SystemConfigModified>,
}

impl SystemConfigDiffs {
    /// Returns true if there are no changes.
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.modified.is_empty()
    }
}

/// A system config file entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemConfigEntry {
    pub path: String,
}

/// A modified system config file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemConfigModified {
    pub path: String,
    /// Semantic diff if parseable
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic_diff: Option<SemanticDiff>,
    /// Legacy raw diff (deprecated, use semantic_diff)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff: Option<String>,
}

/// Upstream changes (Phase 2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamChanges {
    /// Base image changes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_image: Option<BaseImageChange>,
    /// Upstream tool changes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<ToolChanges>,
}

/// Base image change information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaseImageChange {
    pub name: String,
    pub previous_digest: String,
    pub current_digest: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub packages: Option<PackageChanges>,
}

/// Package changes in base image.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageChanges {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub added: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub removed: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub updated: Vec<PackageUpdate>,
}

/// A package update entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageUpdate {
    pub name: String,
    pub from: String,
    pub to: String,
}

/// Upstream tool changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolChanges {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub added: Vec<ToolEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub removed: Vec<ToolEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub updated: Vec<ToolUpdate>,
}

/// A tool entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolEntry {
    pub name: String,
    pub version: String,
}

/// A tool update entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolUpdate {
    pub name: String,
    pub from: String,
    pub to: String,
}

/// Provenance entry (Phase 4).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvenanceEntry {
    pub component: String,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verification: Option<String>,
}
