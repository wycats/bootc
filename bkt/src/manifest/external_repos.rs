//! External RPM repositories manifest types.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Controls how a package is grouped for deployment layers.
///
/// Build stages are always per-package (for cache efficiency).
/// This field controls deployment layer consolidation to avoid
/// btrfs hardlink limits in ostree.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
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

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct ExternalReposManifest {
    #[serde(rename = "$schema", default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,

    pub repos: Vec<ExternalRepo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ExternalRepo {
    pub name: String,
    pub display_name: String,
    pub baseurl: String,
    pub gpg_key: String,
    pub packages: Vec<String>,
    /// Optional path for /opt relocation (e.g., "microsoft" or "1Password")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opt_path: Option<String>,
    /// Controls deployment layer grouping. Defaults to bundled.
    #[serde(default)]
    pub layer_group: LayerGroup,
}
