//! Parser for systemd unit files.
//!
//! systemd units use INI-style format with sections like `[Unit]`,
//! `[Service]`, `[Install]`, `[Timer]`.

use super::{PropertyChange, SystemdDiff};
use std::collections::BTreeMap;

/// Parsed systemd unit file.
#[derive(Debug, Clone, Default)]
pub struct SystemdUnit {
    /// Sections with their key-value properties.
    /// Key is section name (e.g., "Unit", "Service", "Install").
    /// Value is map of property -> value.
    pub sections: BTreeMap<String, BTreeMap<String, String>>,
}

/// Parse systemd unit content into structured form.
pub fn parse(content: &str) -> SystemdUnit {
    let mut unit = SystemdUnit::default();
    let mut current_section = String::new();

    for line in content.lines() {
        let line = line.trim();

        // Skip empty lines and comments
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }

        // Section header
        if line.starts_with('[') && line.ends_with(']') {
            current_section = line[1..line.len() - 1].to_string();
            unit.sections.entry(current_section.clone()).or_default();
            continue;
        }

        // Key = value property
        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim().to_string();
            let value = value.trim().to_string();
            if !current_section.is_empty() {
                unit.sections
                    .entry(current_section.clone())
                    .or_default()
                    .insert(key, value);
            }
        }
    }

    unit
}

/// Compute diff between two systemd units.
pub fn diff(old: &SystemdUnit, new: &SystemdUnit) -> SystemdDiff {
    let mut result = SystemdDiff::default();

    // Get all section names from both units
    let mut all_sections: Vec<_> = old.sections.keys().chain(new.sections.keys()).collect();
    all_sections.sort();
    all_sections.dedup();

    for section in all_sections {
        let old_props = old.sections.get(section);
        let new_props = new.sections.get(section);

        let changes = diff_properties(old_props, new_props);
        if !changes.is_empty() {
            result.sections.insert(section.clone(), changes);
        }
    }

    result
}

fn diff_properties(
    old: Option<&BTreeMap<String, String>>,
    new: Option<&BTreeMap<String, String>>,
) -> Vec<PropertyChange> {
    let empty = BTreeMap::new();
    let old = old.unwrap_or(&empty);
    let new = new.unwrap_or(&empty);

    let mut changes = Vec::new();

    // Get all keys from both
    let mut all_keys: Vec<_> = old.keys().chain(new.keys()).collect();
    all_keys.sort();
    all_keys.dedup();

    for key in all_keys {
        let old_val = old.get(key);
        let new_val = new.get(key);

        match (old_val, new_val) {
            (None, Some(v)) => {
                // Added
                changes.push(PropertyChange {
                    property: key.clone(),
                    from: None,
                    to: Some(v.clone()),
                });
            }
            (Some(v), None) => {
                // Removed
                changes.push(PropertyChange {
                    property: key.clone(),
                    from: Some(v.clone()),
                    to: None,
                });
            }
            (Some(old_v), Some(new_v)) if old_v != new_v => {
                // Changed
                changes.push(PropertyChange {
                    property: key.clone(),
                    from: Some(old_v.clone()),
                    to: Some(new_v.clone()),
                });
            }
            _ => {
                // Same, no change
            }
        }
    }

    changes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_systemd_unit() {
        let content = r#"
[Unit]
Description=My Service
After=network.target

[Service]
Type=oneshot
ExecStart=/usr/bin/my-script

[Install]
WantedBy=default.target
"#;

        let unit = parse(content);

        assert!(unit.sections.contains_key("Unit"));
        assert!(unit.sections.contains_key("Service"));
        assert!(unit.sections.contains_key("Install"));

        let unit_section = unit.sections.get("Unit").unwrap();
        assert_eq!(
            unit_section.get("Description"),
            Some(&"My Service".to_string())
        );
        assert_eq!(
            unit_section.get("After"),
            Some(&"network.target".to_string())
        );

        let service = unit.sections.get("Service").unwrap();
        assert_eq!(service.get("Type"), Some(&"oneshot".to_string()));
        assert_eq!(
            service.get("ExecStart"),
            Some(&"/usr/bin/my-script".to_string())
        );
    }

    #[test]
    fn test_diff_systemd_added_property() {
        let old = parse("[Service]\nType=oneshot\n");
        let new = parse("[Service]\nType=oneshot\nRestart=on-failure\n");

        let diff = diff(&old, &new);
        let service_changes = diff.sections.get("Service").unwrap();

        assert_eq!(service_changes.len(), 1);
        assert_eq!(service_changes[0].property, "Restart");
        assert_eq!(service_changes[0].from, None);
        assert_eq!(service_changes[0].to, Some("on-failure".to_string()));
    }

    #[test]
    fn test_diff_systemd_changed_property() {
        let old = parse("[Service]\nExecStart=/usr/bin/old\n");
        let new = parse("[Service]\nExecStart=/usr/bin/new\n");

        let diff = diff(&old, &new);
        let service_changes = diff.sections.get("Service").unwrap();

        assert_eq!(service_changes.len(), 1);
        assert_eq!(service_changes[0].property, "ExecStart");
        assert_eq!(service_changes[0].from, Some("/usr/bin/old".to_string()));
        assert_eq!(service_changes[0].to, Some("/usr/bin/new".to_string()));
    }

    #[test]
    fn test_diff_systemd_added_section() {
        let old = parse("[Unit]\nDescription=Test\n");
        let new = parse("[Unit]\nDescription=Test\n\n[Install]\nWantedBy=default.target\n");

        let diff = diff(&old, &new);

        assert!(!diff.sections.contains_key("Unit")); // No changes in Unit
        assert!(diff.sections.contains_key("Install")); // New section

        let install_changes = diff.sections.get("Install").unwrap();
        assert_eq!(install_changes.len(), 1);
        assert_eq!(install_changes[0].property, "WantedBy");
    }
}
