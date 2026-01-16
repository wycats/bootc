//! Semantic parsers for system configuration files.
//!
//! This module provides parsers that extract structured data from config files,
//! enabling semantic diffs (e.g., "key X changed from Y to Z") rather than
//! line-based diffs.

pub mod keyd;
pub mod systemd;

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A semantic diff representation for config files.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SemanticDiff {
    /// keyd config: key bindings
    Keyd(KeydDiff),
    /// systemd unit: properties
    Systemd(SystemdDiff),
    /// Generic key-value config (INI, TOML)
    KeyValue(KeyValueDiff),
    /// Fallback: line count summary
    LineSummary(LineSummary),
}

/// Semantic diff for keyd config files.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KeydDiff {
    /// Binding changes by section (e.g., "main", "meta_mac:A")
    pub sections: BTreeMap<String, Vec<BindingChange>>,
}

impl KeydDiff {
    pub fn is_empty(&self) -> bool {
        self.sections.values().all(|v| v.is_empty())
    }
}

/// A single binding change in keyd config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BindingChange {
    pub key: String,
    pub from: Option<String>,
    pub to: Option<String>,
}

/// Semantic diff for systemd unit files.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SystemdDiff {
    /// Property changes by section (e.g., "Unit", "Service", "Install")
    pub sections: BTreeMap<String, Vec<PropertyChange>>,
}

impl SystemdDiff {
    pub fn is_empty(&self) -> bool {
        self.sections.values().all(|v| v.is_empty())
    }
}

/// A single property change in systemd unit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyChange {
    pub property: String,
    pub from: Option<String>,
    pub to: Option<String>,
}

/// Generic key-value diff (for INI, TOML, etc.).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KeyValueDiff {
    /// Changes organized by section (empty string for root level)
    pub sections: BTreeMap<String, Vec<PropertyChange>>,
}

impl KeyValueDiff {
    pub fn is_empty(&self) -> bool {
        self.sections.values().all(|v| v.is_empty())
    }
}

/// Fallback line-based summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineSummary {
    pub added: usize,
    pub removed: usize,
}

/// Classify a file path into a parser type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigFileType {
    Keyd,
    Systemd,
    Ini,
    Other,
}

impl ConfigFileType {
    /// Determine file type from path.
    pub fn from_path(path: &str) -> Self {
        if path.starts_with("system/keyd/") && path.ends_with(".conf") {
            ConfigFileType::Keyd
        } else if path.starts_with("systemd/")
            && (path.ends_with(".service") || path.ends_with(".timer"))
        {
            ConfigFileType::Systemd
        } else if path.ends_with(".conf") || path.ends_with(".ini") {
            ConfigFileType::Ini
        } else {
            ConfigFileType::Other
        }
    }
}

/// Compute a semantic diff between two file contents.
pub fn compute_semantic_diff(
    file_type: ConfigFileType,
    old_content: Option<&str>,
    new_content: Option<&str>,
) -> SemanticDiff {
    match file_type {
        ConfigFileType::Keyd => {
            let old = old_content.map(keyd::parse).unwrap_or_default();
            let new = new_content.map(keyd::parse).unwrap_or_default();
            SemanticDiff::Keyd(keyd::diff(&old, &new))
        }
        ConfigFileType::Systemd => {
            let old = old_content.map(systemd::parse).unwrap_or_default();
            let new = new_content.map(systemd::parse).unwrap_or_default();
            SemanticDiff::Systemd(systemd::diff(&old, &new))
        }
        ConfigFileType::Ini => {
            // For now, treat generic INI as systemd-style (same format)
            let old = old_content.map(systemd::parse).unwrap_or_default();
            let new = new_content.map(systemd::parse).unwrap_or_default();
            let systemd_diff = systemd::diff(&old, &new);
            // Convert to KeyValueDiff
            let kv_diff = KeyValueDiff {
                sections: systemd_diff.sections.into_iter().collect(),
            };
            SemanticDiff::KeyValue(kv_diff)
        }
        ConfigFileType::Other => {
            // Fallback to line count summary
            let old_lines = old_content.map(|c| c.lines().count()).unwrap_or(0);
            let new_lines = new_content.map(|c| c.lines().count()).unwrap_or(0);
            SemanticDiff::LineSummary(LineSummary {
                added: new_lines.saturating_sub(old_lines),
                removed: old_lines.saturating_sub(new_lines),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_file_type_detection() {
        assert_eq!(
            ConfigFileType::from_path("system/keyd/default.conf"),
            ConfigFileType::Keyd
        );
        assert_eq!(
            ConfigFileType::from_path("systemd/user/foo.service"),
            ConfigFileType::Systemd
        );
        assert_eq!(
            ConfigFileType::from_path("systemd/user/bar.timer"),
            ConfigFileType::Systemd
        );
        assert_eq!(
            ConfigFileType::from_path("system/fontconfig/99-emoji-fix.conf"),
            ConfigFileType::Ini
        );
        assert_eq!(
            ConfigFileType::from_path("system/polkit-1/rules.d/foo.rules"),
            ConfigFileType::Other
        );
    }
}
