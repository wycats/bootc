//! GSettings manifest types.

use serde::{Deserialize, Serialize};

/// A GSettings entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GSetting {
    /// Schema name (e.g., "org.gnome.settings-daemon.plugins.power")
    pub schema: String,
    /// Key name (e.g., "sleep-inactive-ac-type")
    pub key: String,
    /// Value as a GVariant string (e.g., "'nothing'" or "0")
    pub value: String,
    /// Optional comment explaining the setting
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

/// The gsettings.json manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GSettingsManifest {
    #[serde(rename = "$schema", skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    pub settings: Vec<GSetting>,
}
