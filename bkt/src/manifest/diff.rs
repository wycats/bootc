//! Diffing infrastructure for manifest types.
//!
//! This module provides the `Diffable` trait and `diff_collections` function
//! for computing differences between manifest collections.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Trait for types that can be diffed by a unique key.
pub trait Diffable {
    /// Returns a unique key for matching items across old/new collections.
    fn diff_key(&self) -> String;

    /// Returns true if items with the same key have different content.
    fn content_differs(&self, other: &Self) -> bool;
}

/// Represents a changed item with before and after states.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangedItem<T> {
    pub from: T,
    pub to: T,
}

/// Result of diffing two collections.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DiffResult<T> {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub added: Vec<T>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub removed: Vec<T>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changed: Vec<ChangedItem<T>>,
}

impl<T> DiffResult<T> {
    /// Returns true if there are no differences.
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.changed.is_empty()
    }
}

/// Compute diff between two collections of Diffable items.
pub fn diff_collections<T: Diffable + Clone>(old: &[T], new: &[T]) -> DiffResult<T> {
    let old_by_key: HashMap<String, &T> = old.iter().map(|item| (item.diff_key(), item)).collect();
    let new_by_key: HashMap<String, &T> = new.iter().map(|item| (item.diff_key(), item)).collect();

    let old_keys: HashSet<_> = old_by_key.keys().cloned().collect();
    let new_keys: HashSet<_> = new_by_key.keys().cloned().collect();

    // Added: in new but not in old
    let added: Vec<T> = new_keys
        .difference(&old_keys)
        .filter_map(|k| new_by_key.get(k).map(|v| (*v).clone()))
        .collect();

    // Removed: in old but not in new
    let removed: Vec<T> = old_keys
        .difference(&new_keys)
        .filter_map(|k| old_by_key.get(k).map(|v| (*v).clone()))
        .collect();

    // Changed: in both but with different content
    let changed: Vec<ChangedItem<T>> = old_keys
        .intersection(&new_keys)
        .filter_map(|k| {
            let old_item = old_by_key.get(k)?;
            let new_item = new_by_key.get(k)?;
            if old_item.content_differs(new_item) {
                Some(ChangedItem {
                    from: (*old_item).clone(),
                    to: (*new_item).clone(),
                })
            } else {
                None
            }
        })
        .collect();

    DiffResult {
        added,
        removed,
        changed,
    }
}

/// Compute diff between two sets of strings (e.g., package names).
pub fn diff_string_sets(old: &[String], new: &[String]) -> DiffResult<String> {
    let old_set: HashSet<_> = old.iter().cloned().collect();
    let new_set: HashSet<_> = new.iter().cloned().collect();

    let added: Vec<String> = new_set.difference(&old_set).cloned().collect();
    let removed: Vec<String> = old_set.difference(&new_set).cloned().collect();

    DiffResult {
        added,
        removed,
        changed: vec![], // String sets don't have "changed" - only added/removed
    }
}

// ============================================================================
// Diffable implementations for manifest types
// ============================================================================

use super::{AppImageApp, ExtensionItem, FlatpakApp, FlatpakRemote, GSetting, Shim};

impl Diffable for FlatpakApp {
    fn diff_key(&self) -> String {
        self.id.clone()
    }

    fn content_differs(&self, other: &Self) -> bool {
        self.remote != other.remote
            || self.scope != other.scope
            || self.branch != other.branch
            || self.commit != other.commit
            || self.overrides != other.overrides
    }
}

impl Diffable for FlatpakRemote {
    fn diff_key(&self) -> String {
        self.name.clone()
    }

    fn content_differs(&self, other: &Self) -> bool {
        self.url != other.url || self.scope != other.scope || self.filtered != other.filtered
    }
}

impl Diffable for ExtensionItem {
    fn diff_key(&self) -> String {
        self.id().to_string()
    }

    fn content_differs(&self, other: &Self) -> bool {
        self.enabled() != other.enabled()
    }
}

impl Diffable for GSetting {
    fn diff_key(&self) -> String {
        format!("{}:{}", self.schema, self.key)
    }

    fn content_differs(&self, other: &Self) -> bool {
        self.value != other.value
    }
}

impl Diffable for Shim {
    fn diff_key(&self) -> String {
        self.name.clone()
    }

    fn content_differs(&self, other: &Self) -> bool {
        self.host != other.host
    }
}

impl Diffable for AppImageApp {
    fn diff_key(&self) -> String {
        self.name.clone()
    }

    fn content_differs(&self, other: &Self) -> bool {
        self.repo != other.repo
            || self.asset != other.asset
            || self.prereleases != other.prereleases
            || self.disabled != other.disabled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq)]
    struct TestItem {
        id: String,
        value: i32,
    }

    impl Diffable for TestItem {
        fn diff_key(&self) -> String {
            self.id.clone()
        }

        fn content_differs(&self, other: &Self) -> bool {
            self.value != other.value
        }
    }

    #[test]
    fn test_diff_empty_collections() {
        let result: DiffResult<TestItem> = diff_collections(&[], &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn test_diff_added_items() {
        let old: Vec<TestItem> = vec![];
        let new = vec![TestItem {
            id: "a".into(),
            value: 1,
        }];

        let result = diff_collections(&old, &new);
        assert_eq!(result.added.len(), 1);
        assert_eq!(result.added[0].id, "a");
        assert!(result.removed.is_empty());
        assert!(result.changed.is_empty());
    }

    #[test]
    fn test_diff_removed_items() {
        let old = vec![TestItem {
            id: "a".into(),
            value: 1,
        }];
        let new: Vec<TestItem> = vec![];

        let result = diff_collections(&old, &new);
        assert!(result.added.is_empty());
        assert_eq!(result.removed.len(), 1);
        assert_eq!(result.removed[0].id, "a");
        assert!(result.changed.is_empty());
    }

    #[test]
    fn test_diff_changed_items() {
        let old = vec![TestItem {
            id: "a".into(),
            value: 1,
        }];
        let new = vec![TestItem {
            id: "a".into(),
            value: 2,
        }];

        let result = diff_collections(&old, &new);
        assert!(result.added.is_empty());
        assert!(result.removed.is_empty());
        assert_eq!(result.changed.len(), 1);
        assert_eq!(result.changed[0].from.value, 1);
        assert_eq!(result.changed[0].to.value, 2);
    }

    #[test]
    fn test_diff_string_sets() {
        let old = vec!["a".into(), "b".into(), "c".into()];
        let new = vec!["b".into(), "c".into(), "d".into()];

        let result = diff_string_sets(&old, &new);
        assert_eq!(result.added, vec!["d".to_string()]);
        assert_eq!(result.removed, vec!["a".to_string()]);
        assert!(result.changed.is_empty());
    }
}
