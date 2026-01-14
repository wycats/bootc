//! System Component trait and related types.
//!
//! This module provides the `SystemComponent` trait that unifies how subsystems
//! (Flatpak, Extensions, DNF, Shims, GSettings) interact with `status`, `capture`,
//! and `apply` commands.
//!
//! See RFC 0012 for design rationale.

use anyhow::Result;
use std::collections::HashSet;
use std::fmt::{Debug, Display};
use std::hash::Hash;

/// Represents the identity and state of a single managed resource.
///
/// Resources are things like individual Flatpak apps, GNOME extensions,
/// or package names that can be tracked in manifests and compared against
/// system state.
pub trait Resource: PartialEq + Clone + Debug {
    /// The type used to identify this resource uniquely.
    type Id: Eq + Hash + Display + Clone;

    /// Get the unique identifier for this resource.
    fn id(&self) -> Self::Id;

    /// Merge two resources with the same ID.
    ///
    /// Used when combining system and user manifests. Default behavior
    /// is that `other` (typically user manifest) wins.
    fn merge(&self, other: &Self) -> Self {
        other.clone()
    }
}

/// A unifying trait for all managed subsystems.
///
/// `SystemComponent` provides a standard interface for:
/// - Discovering current system state (`scan_system`)
/// - Loading desired state from manifests (`load_manifest`)
/// - Computing differences (`diff`)
/// - Capturing system state to manifests (`capture`)
///
/// Execution (applying changes) is handled by the `Plannable` trait.
pub trait SystemComponent {
    /// The type of item managed (e.g., FlatpakApp, ExtensionItem).
    type Item: Resource;

    /// The manifest format (e.g., FlatpakAppsManifest).
    type Manifest;

    /// Filter type for capture operations. Use `()` if no filter needed.
    type CaptureFilter;

    /// Human-readable name for display purposes.
    fn name(&self) -> &'static str;

    // ─────────────────────────────────────────────────────
    // Phase 1: Discovery (Read-Only)
    // ─────────────────────────────────────────────────────

    /// Get the current state of the system.
    ///
    /// For Flatpak, this returns all installed apps.
    /// For Extensions, this returns all enabled extensions.
    fn scan_system(&self) -> Result<Vec<Self::Item>>;

    /// Load and merge manifests (system + user).
    fn load_manifest(&self) -> Result<Self::Manifest>;

    /// Extract items from the manifest for diffing.
    fn manifest_items(&self, manifest: &Self::Manifest) -> Vec<Self::Item>;

    // ─────────────────────────────────────────────────────
    // Phase 2: Reconciliation (Pure Logic)
    // ─────────────────────────────────────────────────────

    /// Calculate differences between system state and manifest.
    ///
    /// The default implementation uses standard set-based diffing.
    /// Components with rich state (like Extensions with enabled/disabled)
    /// should override this to populate `to_update`.
    fn diff(&self, system: &[Self::Item], manifest: &Self::Manifest) -> DriftReport<Self::Item> {
        let manifest_items = self.manifest_items(manifest);
        let system_ids: HashSet<_> = system.iter().map(|i| i.id()).collect();
        let manifest_ids: HashSet<_> = manifest_items.iter().map(|i| i.id()).collect();

        DriftReport {
            to_install: manifest_items
                .iter()
                .filter(|i| !system_ids.contains(&i.id()))
                .cloned()
                .collect(),
            untracked: system
                .iter()
                .filter(|i| !manifest_ids.contains(&i.id()))
                .cloned()
                .collect(),
            to_update: Vec::new(),
            synced_count: manifest_items
                .iter()
                .filter(|i| system_ids.contains(&i.id()))
                .count(),
        }
    }

    // ─────────────────────────────────────────────────────
    // Phase 3: Capture (System → Manifest)
    // ─────────────────────────────────────────────────────

    /// Whether this component supports capture (system → manifest).
    ///
    /// Components like Shims return false (derived state only).
    fn supports_capture(&self) -> bool {
        true
    }

    /// Capture system state to manifest, with optional filtering.
    ///
    /// Returns `None` if component doesn't support capture.
    fn capture(
        &self,
        system: &[Self::Item],
        filter: Self::CaptureFilter,
    ) -> Option<Result<Self::Manifest>>;

    // ─────────────────────────────────────────────────────
    // Convenience Methods
    // ─────────────────────────────────────────────────────

    /// Get a complete status report for this component.
    ///
    /// This is a convenience method that combines scan, load, and diff.
    fn status(&self) -> Result<ComponentStatus> {
        let system = self.scan_system()?;
        let manifest = self.load_manifest()?;
        let drift = self.diff(&system, &manifest);

        Ok(ComponentStatus {
            name: self.name().to_string(),
            total: drift.synced_count + drift.to_install.len(),
            synced: drift.synced_count,
            pending: drift.to_install.len(),
            untracked: drift.untracked.len(),
            to_update: drift.to_update.len(),
        })
    }
}

/// Result of diffing system state against manifest.
#[derive(Debug, Clone, Default)]
pub struct DriftReport<T> {
    /// Items in manifest but not on system (pending install).
    pub to_install: Vec<T>,
    /// Items on system but not in manifest (pending capture).
    pub untracked: Vec<T>,
    /// Items that exist in both but with different state (e.g., enabled vs disabled).
    /// Tuple is (current_state, desired_state).
    pub to_update: Vec<(T, T)>,
    /// Count of items already in sync.
    pub synced_count: usize,
}

impl<T> DriftReport<T> {
    /// Returns true if there are no pending changes.
    pub fn is_synced(&self) -> bool {
        self.to_install.is_empty() && self.to_update.is_empty()
    }

    /// Returns true if there are untracked items that could be captured.
    pub fn has_untracked(&self) -> bool {
        !self.untracked.is_empty()
    }

    /// Total number of items that need action (install or update).
    pub fn pending_count(&self) -> usize {
        self.to_install.len() + self.to_update.len()
    }
}

/// Aggregated status for a single component.
#[derive(Debug, Clone)]
pub struct ComponentStatus {
    /// Component name (e.g., "Flatpaks", "Extensions").
    pub name: String,
    /// Total items in manifest.
    pub total: usize,
    /// Items that are in sync.
    pub synced: usize,
    /// Items in manifest but not on system.
    pub pending: usize,
    /// Items on system but not in manifest.
    pub untracked: usize,
    /// Items that exist but with different state.
    pub to_update: usize,
}

impl ComponentStatus {
    /// Whether this component is fully synced.
    pub fn is_synced(&self) -> bool {
        self.pending == 0 && self.to_update == 0
    }

    /// Whether there's drift that needs capture.
    pub fn has_drift(&self) -> bool {
        self.untracked > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq, Default)]
    struct TestItem {
        id: String,
    }

    impl Resource for TestItem {
        type Id = String;

        fn id(&self) -> String {
            self.id.clone()
        }
    }

    #[test]
    fn drift_report_empty_is_synced() {
        let report: DriftReport<TestItem> = DriftReport::default();
        assert!(report.is_synced());
        assert!(!report.has_untracked());
    }

    #[test]
    fn drift_report_with_pending_not_synced() {
        let report: DriftReport<TestItem> = DriftReport {
            to_install: vec![TestItem {
                id: "foo".to_string(),
            }],
            ..Default::default()
        };
        assert!(!report.is_synced());
        assert_eq!(report.pending_count(), 1);
    }

    #[test]
    fn drift_report_with_untracked() {
        let report: DriftReport<TestItem> = DriftReport {
            untracked: vec![TestItem {
                id: "bar".to_string(),
            }],
            ..Default::default()
        };
        assert!(report.is_synced()); // untracked doesn't affect sync status
        assert!(report.has_untracked());
    }
}
