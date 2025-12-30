//! Flatpak manifest types.

use serde::{Deserialize, Serialize};

/// Scope for Flatpak apps and remotes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FlatpakScope {
    System,
    User,
}

/// A Flatpak application entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlatpakApp {
    /// Application ID (e.g., "org.gnome.Calculator")
    pub id: String,
    /// Remote name (e.g., "flathub")
    pub remote: String,
    /// Installation scope
    pub scope: FlatpakScope,
}

/// The flatpak-apps.json manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlatpakAppsManifest {
    #[serde(rename = "$schema", skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    pub apps: Vec<FlatpakApp>,
}

/// A Flatpak remote entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlatpakRemote {
    /// Remote name
    pub name: String,
    /// Remote URL
    pub url: String,
    /// Installation scope
    pub scope: FlatpakScope,
    /// Whether the remote is filtered (Flathub verified only)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filtered: Option<bool>,
}

/// The flatpak-remotes.json manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlatpakRemotesManifest {
    #[serde(rename = "$schema", skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    pub remotes: Vec<FlatpakRemote>,
}
