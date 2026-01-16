//! Parser for keyd configuration files.
//!
//! keyd uses an INI-like format with sections like `[main]`, `[ids]`,
//! and layer definitions like `[meta_mac:A]`.

use super::{BindingChange, KeydDiff};
use std::collections::BTreeMap;

/// Parsed keyd configuration.
#[derive(Debug, Clone, Default)]
pub struct KeydConfig {
    /// Sections with their key-value bindings.
    /// Key is section name (e.g., "main", "meta_mac:A").
    /// Value is map of key -> action.
    pub sections: BTreeMap<String, BTreeMap<String, String>>,
}

/// Parse keyd config content into structured form.
pub fn parse(content: &str) -> KeydConfig {
    let mut config = KeydConfig::default();
    let mut current_section = String::new();

    for line in content.lines() {
        let line = line.trim();

        // Skip empty lines and comments
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Section header
        if line.starts_with('[') && line.ends_with(']') {
            current_section = line[1..line.len() - 1].to_string();
            config.sections.entry(current_section.clone()).or_default();
            continue;
        }

        // Key = value binding
        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim().to_string();
            let value = value.trim().to_string();
            if !current_section.is_empty() {
                config
                    .sections
                    .entry(current_section.clone())
                    .or_default()
                    .insert(key, value);
            }
        }
    }

    config
}

/// Compute diff between two keyd configs.
pub fn diff(old: &KeydConfig, new: &KeydConfig) -> KeydDiff {
    let mut result = KeydDiff::default();

    // Get all section names from both configs
    let mut all_sections: Vec<_> = old.sections.keys().chain(new.sections.keys()).collect();
    all_sections.sort();
    all_sections.dedup();

    for section in all_sections {
        let old_bindings = old.sections.get(section);
        let new_bindings = new.sections.get(section);

        let changes = diff_bindings(old_bindings, new_bindings);
        if !changes.is_empty() {
            result.sections.insert(section.clone(), changes);
        }
    }

    result
}

fn diff_bindings(
    old: Option<&BTreeMap<String, String>>,
    new: Option<&BTreeMap<String, String>>,
) -> Vec<BindingChange> {
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
                changes.push(BindingChange {
                    key: key.clone(),
                    from: None,
                    to: Some(v.clone()),
                });
            }
            (Some(v), None) => {
                // Removed
                changes.push(BindingChange {
                    key: key.clone(),
                    from: Some(v.clone()),
                    to: None,
                });
            }
            (Some(old_v), Some(new_v)) if old_v != new_v => {
                // Changed
                changes.push(BindingChange {
                    key: key.clone(),
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
    fn test_parse_keyd_config() {
        let content = r#"
[ids]
*

[main]
# Map physical Left Command (Meta) to a custom layer.
leftmeta = layer(meta_mac)
capslock = esc

[meta_mac:A]
c = C-insert
v = S-insert
"#;

        let config = parse(content);

        assert!(config.sections.contains_key("ids"));
        assert!(config.sections.contains_key("main"));
        assert!(config.sections.contains_key("meta_mac:A"));

        let main = config.sections.get("main").unwrap();
        assert_eq!(main.get("leftmeta"), Some(&"layer(meta_mac)".to_string()));
        assert_eq!(main.get("capslock"), Some(&"esc".to_string()));

        let meta_mac = config.sections.get("meta_mac:A").unwrap();
        assert_eq!(meta_mac.get("c"), Some(&"C-insert".to_string()));
        assert_eq!(meta_mac.get("v"), Some(&"S-insert".to_string()));
    }

    #[test]
    fn test_diff_keyd_added_binding() {
        let old = parse("[main]\ncapslock = esc\n");
        let new = parse("[main]\ncapslock = esc\nleftmeta = layer(meta)\n");

        let diff = diff(&old, &new);
        let main_changes = diff.sections.get("main").unwrap();

        assert_eq!(main_changes.len(), 1);
        assert_eq!(main_changes[0].key, "leftmeta");
        assert_eq!(main_changes[0].from, None);
        assert_eq!(main_changes[0].to, Some("layer(meta)".to_string()));
    }

    #[test]
    fn test_diff_keyd_removed_binding() {
        let old = parse("[main]\ncapslock = esc\nleftmeta = layer(meta)\n");
        let new = parse("[main]\ncapslock = esc\n");

        let diff = diff(&old, &new);
        let main_changes = diff.sections.get("main").unwrap();

        assert_eq!(main_changes.len(), 1);
        assert_eq!(main_changes[0].key, "leftmeta");
        assert_eq!(main_changes[0].from, Some("layer(meta)".to_string()));
        assert_eq!(main_changes[0].to, None);
    }

    #[test]
    fn test_diff_keyd_changed_binding() {
        let old = parse("[main]\ncapslock = esc\n");
        let new = parse("[main]\ncapslock = backspace\n");

        let diff = diff(&old, &new);
        let main_changes = diff.sections.get("main").unwrap();

        assert_eq!(main_changes.len(), 1);
        assert_eq!(main_changes[0].key, "capslock");
        assert_eq!(main_changes[0].from, Some("esc".to_string()));
        assert_eq!(main_changes[0].to, Some("backspace".to_string()));
    }
}
