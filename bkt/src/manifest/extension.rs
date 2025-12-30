//! GNOME extension manifest types.

use serde::{Deserialize, Serialize};

/// The gnome-extensions.json manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GnomeExtensionsManifest {
    #[serde(rename = "$schema", skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    /// List of extension UUIDs (e.g., "dash-to-dock@micxgx.gmail.com")
    pub extensions: Vec<String>,
}

/// A GNOME Shell extension.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GnomeExtension {
    /// Extension UUID (e.g., "dash-to-dock@micxgx.gmail.com")
    pub uuid: String,
}

impl From<String> for GnomeExtension {
    fn from(uuid: String) -> Self {
        Self { uuid }
    }
}

impl From<&str> for GnomeExtension {
    fn from(uuid: &str) -> Self {
        Self {
            uuid: uuid.to_string(),
        }
    }
}
