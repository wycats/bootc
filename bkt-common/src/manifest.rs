use crate::error::CommonError;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Source of an upstream dependency.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum InstallConfig {
    /// Extract an archive
    Archive {
        /// Target directory for extraction
        extract_to: String,
        /// Number of leading path components to strip
        #[serde(default)]
        strip_components: u32,
        /// Output paths produced by the archive extraction
        #[serde(default, skip_serializing_if = "Option::is_none")]
        outputs: Option<Vec<String>>,
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        outputs: Option<Vec<String>>,
        /// Full build script lines (each line is a shell command).
        /// When present, the generator emits these as a heredoc RUN block
        /// instead of using `bkt-build fetch`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        build_script: Option<Vec<String>>,
        /// Optional override for the stage name in the Containerfile.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        stage_name: Option<String>,
    },
}

/// An upstream dependency entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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

/// The upstream manifest (upstream/manifest.json).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UpstreamManifest {
    #[serde(rename = "$schema", skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,

    /// List of upstream dependencies
    #[serde(default)]
    pub upstreams: Vec<Upstream>,
}

/// An external RPM repository entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ExternalRepo {
    pub name: String,
    pub display_name: String,
    pub baseurl: String,
    pub gpg_key: String,
    pub packages: Vec<String>,
}

/// External repositories manifest (manifests/external-repos.json).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ExternalReposManifest {
    #[serde(rename = "$schema", skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,

    pub repos: Vec<ExternalRepo>,
}

impl UpstreamManifest {
    /// Load a manifest from a specific path.
    pub fn load_from(path: &Path) -> Result<Self, CommonError> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = fs::read_to_string(path)?;
        let manifest: Self = serde_json::from_str(&content)?;
        Ok(manifest)
    }

    /// Save the manifest to a specific path.
    pub fn save_to(&self, path: &Path) -> Result<(), CommonError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(self)?;
        fs::write(path, content)?;
        Ok(())
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
}

impl ExternalReposManifest {
    /// Load a manifest from a specific path.
    pub fn load_from(path: &Path) -> Result<Self, CommonError> {
        let content = fs::read_to_string(path)?;
        let manifest: Self = serde_json::from_str(&content)?;
        Ok(manifest)
    }

    /// Find an external repository by name.
    pub fn find(&self, name: &str) -> Option<&ExternalRepo> {
        self.repos.iter().find(|repo| repo.name == name)
    }
}

// ── Vendor artifacts (intent manifest types) ────────────────────────────────

/// Controls how a package is grouped for deployment layers.
///
/// Build stages are always per-package (for cache efficiency).
/// This field controls deployment layer consolidation to avoid
/// btrfs hardlink limits in ostree.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum LayerGroup {
    /// Package gets its own deployment layer.
    /// Use for high-churn, large packages (e.g., vscode, edge).
    Independent,
    /// Package is grouped with other bundled packages.
    /// Use for low-churn, smaller packages.
    #[default]
    Bundled,
}

/// The vendor-artifacts.json manifest.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct VendorArtifactsManifest {
    #[serde(rename = "$schema", default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,

    pub artifacts: Vec<VendorArtifact>,
}

/// A single vendor artifact entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct VendorArtifact {
    /// Unique identifier (e.g., "code")
    pub name: String,

    /// Human-readable name (e.g., "Visual Studio Code")
    pub display_name: String,

    /// Artifact type
    pub kind: ArtifactKind,

    /// How to discover the latest artifact
    pub source: VendorSource,

    /// Controls deployment layer grouping. Defaults to bundled.
    #[serde(default)]
    pub layer_group: LayerGroup,
}

/// The type of artifact being installed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum ArtifactKind {
    /// RPM package
    Rpm,
}

/// Discovery specification for a vendor artifact.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum VendorSource {
    /// Vendor release feed with templated URL
    VendorFeed {
        /// URL template with `{param}` placeholders
        url: String,

        /// Key-value parameters for template substitution
        #[serde(default, skip_serializing_if = "HashMap::is_empty")]
        params: HashMap<String, String>,

        /// Architecture → platform identifier mapping.
        /// The resolved platform value is available as `{platform}` in the URL template.
        #[serde(default, skip_serializing_if = "HashMap::is_empty")]
        platforms: HashMap<String, String>,

        /// Maps resolved artifact field names to vendor response JSON field names.
        /// Required keys: `url`, `version`, `sha256`. Optional: `vendor_revision`.
        response_map: HashMap<String, String>,
    },
}

impl VendorArtifactsManifest {
    pub const PROJECT_PATH: &'static str = "manifests/vendor-artifacts.json";

    /// Load from a specific path.
    pub fn load_from(path: &Path) -> Result<Self, CommonError> {
        let content = fs::read_to_string(path)?;
        let manifest: Self = serde_json::from_str(&content)?;
        Ok(manifest)
    }

    /// Find an artifact by name.
    pub fn find(&self, name: &str) -> Option<&VendorArtifact> {
        self.artifacts.iter().find(|a| a.name == name)
    }
}

// ── Resolved vendor artifacts (build-time resolution output) ────────────────

/// A resolved vendor artifact, produced by the resolver and consumed by the build.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedVendorArtifact {
    /// Artifact name (matches manifest entry)
    pub name: String,
    /// Artifact type
    pub kind: String,
    /// Resolved version string
    pub version: String,
    /// Direct download URL for the artifact
    pub url: String,
    /// SHA256 checksum of the artifact
    pub sha256: String,
    /// Vendor-specific revision identifier (e.g., commit hash)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vendor_revision: Option<String>,
}

/// The resolved vendor artifacts file (`.cache/bkt/vendor-artifacts.resolved.json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedVendorArtifactsManifest {
    /// When resolution was performed
    pub resolved_at: DateTime<Utc>,
    /// Build architecture
    pub arch: String,
    /// Resolved artifacts
    pub artifacts: Vec<ResolvedVendorArtifact>,
}

impl ResolvedVendorArtifactsManifest {
    /// Load from a specific path.
    pub fn load_from(path: &Path) -> Result<Self, CommonError> {
        let content = fs::read_to_string(path)?;
        let manifest: Self = serde_json::from_str(&content)?;
        Ok(manifest)
    }

    /// Find a resolved artifact by name.
    pub fn find(&self, name: &str) -> Option<&ResolvedVendorArtifact> {
        self.artifacts.iter().find(|a| a.name == name)
    }
}
