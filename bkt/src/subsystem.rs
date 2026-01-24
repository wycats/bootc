//! Subsystem trait and registry for unified manifest handling.
//!
//! This module implements RFC-0026: a formal `Subsystem` trait that unifies how
//! all subsystems (extensions, flatpak, gsettings, distrobox, etc.) handle
//! manifest loading, capture, and sync operations.
//!
//! # Key Benefits
//!
//! - **Single source of truth**: Manifest loading (with proper system + user merging)
//!   happens in one place per subsystem
//! - **Unified enumeration**: The registry provides a single list of all subsystems
//! - **Reduced duplication**: Commands like `apply` and `capture` can iterate
//!   over the registry instead of hard-coding subsystem lists
//! - **bootc-bootstrap simplification**: The bash script can delegate to `bkt`
//!   entirely, eliminating duplicated manifest logic
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    SubsystemRegistry                         │
//! │  ┌───────────┐ ┌───────────┐ ┌───────────┐ ┌───────────┐   │
//! │  │ Extension │ │  Flatpak  │ │  Gsetting │ │  Shim     │   │
//! │  └─────┬─────┘ └─────┬─────┘ └─────┬─────┘ └─────┬─────┘   │
//! │        │             │             │             │          │
//! │        ▼             ▼             ▼             ▼          │
//! │  ┌─────────────────────────────────────────────────────┐   │
//! │  │                 Subsystem trait                      │   │
//! │  │  • load_manifest() → Box<dyn Manifest>              │   │
//! │  │  • capture() → Option<Box<dyn Plan>>                │   │
//! │  │  • sync() → Option<Box<dyn Plan>>                   │   │
//! │  └─────────────────────────────────────────────────────┘   │
//! └─────────────────────────────────────────────────────────────┘
//! ```

use std::fmt;
use std::path::PathBuf;

use anyhow::Result;

use crate::plan::{DynPlan, Plan, PlanContext};

// ============================================================================
// Core Traits
// ============================================================================

/// A subsystem manages a category of declarative configuration.
///
/// Each subsystem knows how to:
/// - Load its manifest (with proper system + user merging)
/// - Capture current system state to a manifest
/// - Sync manifest state to the running system
///
/// # Example Implementation
///
/// ```rust,ignore
/// pub struct ExtensionSubsystem;
///
/// impl Subsystem for ExtensionSubsystem {
///     fn name(&self) -> &'static str { "GNOME Extensions" }
///     fn id(&self) -> &'static str { "extension" }
///
///     fn load_manifest(&self, ctx: &SubsystemContext) -> Result<Box<dyn Manifest>> {
///         let system = GnomeExtensionsManifest::load(&ctx.system_manifest_path("gnome-extensions.json"))?;
///         let user = GnomeExtensionsManifest::load(&ctx.user_manifest_path("gnome-extensions.json"))?;
///         Ok(Box::new(GnomeExtensionsManifest::merged(&system, &user)))
///     }
///
///     fn capture(&self, ctx: &PlanContext) -> Result<Option<Box<dyn DynPlan>>> {
///         let plan = ExtensionCaptureCommand.plan(ctx)?;
///         Ok(if plan.is_empty() { None } else { Some(Box::new(plan)) })
///     }
///
///     fn sync(&self, ctx: &PlanContext) -> Result<Option<Box<dyn DynPlan>>> {
///         let plan = ExtensionSyncCommand.plan(ctx)?;
///         Ok(if plan.is_empty() { None } else { Some(Box::new(plan)) })
///     }
/// }
/// ```
pub trait Subsystem: Send + Sync {
    /// Human-readable name for display (e.g., "GNOME Extensions").
    fn name(&self) -> &'static str;

    /// Short identifier for CLI filtering (e.g., "extension").
    ///
    /// This should match the CLI subcommand name.
    fn id(&self) -> &'static str;

    /// Load the merged manifest (system defaults + user overrides).
    ///
    /// This is THE canonical way to get the effective manifest.
    /// The merge semantics are defined once here, not scattered across code.
    fn load_manifest(&self, ctx: &SubsystemContext) -> Result<Box<dyn Manifest>>;

    /// Create a capture plan (system → manifest).
    ///
    /// Returns `Ok(None)` if this subsystem doesn't support capture.
    fn capture(&self, ctx: &PlanContext) -> Result<Option<Box<dyn DynPlan>>>;

    /// Create a sync plan (manifest → system).
    ///
    /// Returns `Ok(None)` if this subsystem doesn't support sync.
    fn sync(&self, ctx: &PlanContext) -> Result<Option<Box<dyn DynPlan>>>;

    /// Returns true if this subsystem supports capture operations.
    fn supports_capture(&self) -> bool {
        true
    }

    /// Returns true if this subsystem supports sync operations.
    fn supports_sync(&self) -> bool {
        true
    }

    /// Returns true if this subsystem supports drift detection.
    fn supports_drift(&self) -> bool {
        false
    }
}

// ============================================================================
// Subsystem Context
// ============================================================================

/// Context for subsystem manifest loading.
///
/// Provides paths to the various locations where manifests can be found.
/// Subsystems use this to locate their system and user manifest files.
#[derive(Debug, Clone)]
pub struct SubsystemContext {
    /// Repository root (where manifests/ lives).
    pub repo_root: PathBuf,
    /// User config directory (~/.config/bootc/).
    pub user_config_dir: PathBuf,
    /// System manifest directory (/usr/share/bootc-bootstrap/).
    pub system_manifest_dir: PathBuf,
}

impl SubsystemContext {
    /// Create a new subsystem context with default paths.
    pub fn new() -> Self {
        let home = std::env::var("HOME")
            .ok()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/"));

        Self {
            repo_root: std::env::current_dir().unwrap_or_default(),
            user_config_dir: home.join(".config").join("bootc"),
            system_manifest_dir: PathBuf::from("/usr/share/bootc-bootstrap"),
        }
    }

    /// Create a context with a custom repo root.
    pub fn with_repo_root(repo_root: PathBuf) -> Self {
        let home = std::env::var("HOME")
            .ok()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/"));

        Self {
            repo_root,
            user_config_dir: home.join(".config").join("bootc"),
            system_manifest_dir: PathBuf::from("/usr/share/bootc-bootstrap"),
        }
    }

    /// Get the path to a system manifest file.
    pub fn system_manifest_path(&self, filename: &str) -> PathBuf {
        self.system_manifest_dir.join(filename)
    }

    /// Get the path to a user manifest file.
    pub fn user_manifest_path(&self, filename: &str) -> PathBuf {
        self.user_config_dir.join(filename)
    }

    /// Get the path to a manifest file in the repo.
    pub fn repo_manifest_path(&self, filename: &str) -> PathBuf {
        self.repo_root.join("manifests").join(filename)
    }
}

impl Default for SubsystemContext {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Manifest Trait
// ============================================================================

/// Marker trait for typed manifest access.
///
/// All manifest types should implement this trait to enable
/// serialization and dynamic dispatch via `Box<dyn Manifest>`.
pub trait Manifest: std::fmt::Debug + Send + Sync {
    /// Serialize to JSON for capture operations.
    fn to_json(&self) -> Result<String>;
}

// ============================================================================
// Subsystem Registry
// ============================================================================

/// Registry of all known subsystems.
///
/// The registry provides the single source of truth for all subsystems,
/// enabling commands to iterate over subsystems without hard-coding lists.
///
/// # Example
///
/// ```rust,ignore
/// let registry = SubsystemRegistry::builtin();
///
/// // Get all subsystems
/// for subsystem in registry.all() {
///     println!("{}: {}", subsystem.id(), subsystem.name());
/// }
///
/// // Filter by ID
/// let selected = registry.filtered(Some(&["extension", "flatpak"]), &[]);
/// ```
pub struct SubsystemRegistry {
    subsystems: Vec<Box<dyn Subsystem>>,
}

impl SubsystemRegistry {
    /// Create registry with all built-in subsystems.
    pub fn builtin() -> Self {
        Self {
            subsystems: vec![
                Box::new(ExtensionSubsystem),
                Box::new(FlatpakSubsystem),
                Box::new(DistroboxSubsystem),
                Box::new(GsettingSubsystem),
                Box::new(ShimSubsystem),
                Box::new(AppImageSubsystem),
                Box::new(HomebrewSubsystem),
                Box::new(SystemSubsystem),
            ],
        }
    }

    /// Create an empty registry (for testing).
    #[allow(dead_code)]
    pub fn empty() -> Self {
        Self {
            subsystems: Vec::new(),
        }
    }

    /// Get all subsystems.
    pub fn all(&self) -> &[Box<dyn Subsystem>] {
        &self.subsystems
    }

    /// Get subsystems by ID filter.
    ///
    /// - `include`: If `Some`, only include subsystems with these IDs
    /// - `exclude`: Always exclude subsystems with these IDs
    pub fn filtered(&self, include: Option<&[&str]>, exclude: &[&str]) -> Vec<&dyn Subsystem> {
        self.subsystems
            .iter()
            .filter(|s| {
                if exclude.contains(&s.id()) {
                    return false;
                }
                match include {
                    Some(ids) => ids.contains(&s.id()),
                    None => true,
                }
            })
            .map(|s| s.as_ref())
            .collect()
    }

    /// Get subsystems that support capture.
    pub fn capturable(&self) -> Vec<&dyn Subsystem> {
        self.subsystems
            .iter()
            .filter(|s| s.supports_capture())
            .map(|s| s.as_ref())
            .collect()
    }

    /// Get IDs of subsystems that support capture.
    pub fn capturable_ids(&self) -> Vec<&'static str> {
        self.subsystems
            .iter()
            .filter(|s| s.supports_capture())
            .map(|s| s.id())
            .collect()
    }

    /// Check if an ID is a valid capturable subsystem.
    pub fn is_valid_capturable(&self, id: &str) -> bool {
        self.subsystems
            .iter()
            .any(|s| s.supports_capture() && s.id() == id)
    }

    /// Get subsystems that support sync.
    pub fn syncable(&self) -> Vec<&dyn Subsystem> {
        self.subsystems
            .iter()
            .filter(|s| s.supports_sync())
            .map(|s| s.as_ref())
            .collect()
    }

    /// Get subsystems that support drift detection.
    pub fn driftable(&self) -> Vec<&dyn Subsystem> {
        self.subsystems
            .iter()
            .filter(|s| s.supports_drift())
            .map(|s| s.as_ref())
            .collect()
    }

    /// Get IDs of subsystems that support sync.
    pub fn syncable_ids(&self) -> Vec<&'static str> {
        self.subsystems
            .iter()
            .filter(|s| s.supports_sync())
            .map(|s| s.id())
            .collect()
    }

    /// Get IDs of subsystems that support drift detection.
    pub fn driftable_ids(&self) -> Vec<&'static str> {
        self.subsystems
            .iter()
            .filter(|s| s.supports_drift())
            .map(|s| s.id())
            .collect()
    }

    /// Check if an ID is a valid syncable subsystem.
    pub fn is_valid_syncable(&self, id: &str) -> bool {
        self.subsystems
            .iter()
            .any(|s| s.supports_sync() && s.id() == id)
    }

    /// Check if an ID is a valid drift-detectable subsystem.
    pub fn is_valid_driftable(&self, id: &str) -> bool {
        self.subsystems
            .iter()
            .any(|s| s.supports_drift() && s.id() == id)
    }

    /// Find a subsystem by ID.
    pub fn find(&self, id: &str) -> Option<&dyn Subsystem> {
        self.subsystems
            .iter()
            .find(|s| s.id() == id)
            .map(|s| s.as_ref())
    }
}

impl fmt::Debug for SubsystemRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SubsystemRegistry")
            .field(
                "subsystems",
                &self.subsystems.iter().map(|s| s.id()).collect::<Vec<_>>(),
            )
            .finish()
    }
}

// ============================================================================
// Subsystem Implementations
// ============================================================================

// ----------------------------------------------------------------------------
// Extension Subsystem
// ----------------------------------------------------------------------------

use crate::commands::extension::{ExtensionCaptureCommand, ExtensionSyncCommand};
use crate::manifest::GnomeExtensionsManifest;
use crate::plan::Plannable;

/// GNOME Shell extensions subsystem.
pub struct ExtensionSubsystem;

impl Subsystem for ExtensionSubsystem {
    fn name(&self) -> &'static str {
        "GNOME Extensions"
    }

    fn id(&self) -> &'static str {
        "extension"
    }

    fn load_manifest(&self, ctx: &SubsystemContext) -> Result<Box<dyn Manifest>> {
        let system =
            GnomeExtensionsManifest::load(&ctx.system_manifest_path("gnome-extensions.json"))?;
        let user = GnomeExtensionsManifest::load(&ctx.user_manifest_path("gnome-extensions.json"))?;
        Ok(Box::new(GnomeExtensionsManifest::merged(&system, &user)))
    }

    fn capture(&self, ctx: &PlanContext) -> Result<Option<Box<dyn DynPlan>>> {
        let plan = ExtensionCaptureCommand.plan(ctx)?;
        if plan.is_empty() {
            Ok(None)
        } else {
            Ok(Some(Box::new(plan)))
        }
    }

    fn sync(&self, ctx: &PlanContext) -> Result<Option<Box<dyn DynPlan>>> {
        let plan = ExtensionSyncCommand.plan(ctx)?;
        if plan.is_empty() {
            Ok(None)
        } else {
            Ok(Some(Box::new(plan)))
        }
    }

    fn supports_drift(&self) -> bool {
        true
    }
}

impl Manifest for GnomeExtensionsManifest {
    fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }
}

// ----------------------------------------------------------------------------
// Flatpak Subsystem
// ----------------------------------------------------------------------------

use crate::commands::flatpak::{FlatpakCaptureCommand, FlatpakSyncCommand};
use crate::manifest::FlatpakAppsManifest;

/// Flatpak applications subsystem.
pub struct FlatpakSubsystem;

impl Subsystem for FlatpakSubsystem {
    fn name(&self) -> &'static str {
        "Flatpak Apps"
    }

    fn id(&self) -> &'static str {
        "flatpak"
    }

    fn load_manifest(&self, ctx: &SubsystemContext) -> Result<Box<dyn Manifest>> {
        let system = FlatpakAppsManifest::load(&ctx.system_manifest_path("flatpak-apps.json"))?;
        let user = FlatpakAppsManifest::load(&ctx.user_manifest_path("flatpak-apps.json"))?;
        Ok(Box::new(FlatpakAppsManifest::merged(&system, &user)))
    }

    fn capture(&self, ctx: &PlanContext) -> Result<Option<Box<dyn DynPlan>>> {
        let plan = FlatpakCaptureCommand.plan(ctx)?;
        if plan.is_empty() {
            Ok(None)
        } else {
            Ok(Some(Box::new(plan)))
        }
    }

    fn sync(&self, ctx: &PlanContext) -> Result<Option<Box<dyn DynPlan>>> {
        let plan = FlatpakSyncCommand.plan(ctx)?;
        if plan.is_empty() {
            Ok(None)
        } else {
            Ok(Some(Box::new(plan)))
        }
    }

    fn supports_drift(&self) -> bool {
        true
    }
}

impl Manifest for FlatpakAppsManifest {
    fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }
}

// ----------------------------------------------------------------------------
// Distrobox Subsystem
// ----------------------------------------------------------------------------

use crate::commands::distrobox::{DistroboxCaptureCommand, DistroboxSyncCommand};
use crate::manifest::DistroboxManifest;

/// Distrobox containers subsystem.
pub struct DistroboxSubsystem;

impl Subsystem for DistroboxSubsystem {
    fn name(&self) -> &'static str {
        "Distrobox"
    }

    fn id(&self) -> &'static str {
        "distrobox"
    }

    fn load_manifest(&self, ctx: &SubsystemContext) -> Result<Box<dyn Manifest>> {
        // Distrobox uses a different path pattern - it loads from manifests/ dir
        let manifest = DistroboxManifest::load_from_dir(&ctx.repo_root.join("manifests"))?;
        Ok(Box::new(manifest))
    }

    fn capture(&self, ctx: &PlanContext) -> Result<Option<Box<dyn DynPlan>>> {
        let plan = DistroboxCaptureCommand.plan(ctx)?;
        if plan.is_empty() {
            Ok(None)
        } else {
            Ok(Some(Box::new(plan)))
        }
    }

    fn sync(&self, ctx: &PlanContext) -> Result<Option<Box<dyn DynPlan>>> {
        let plan = DistroboxSyncCommand.plan(ctx)?;
        if plan.is_empty() {
            Ok(None)
        } else {
            Ok(Some(Box::new(plan)))
        }
    }
}

impl Manifest for DistroboxManifest {
    fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }
}

// ----------------------------------------------------------------------------
// GSettings Subsystem
// ----------------------------------------------------------------------------

use crate::commands::gsetting::GsettingApplyCommand;
use crate::manifest::GSettingsManifest;

/// GSettings (GNOME settings) subsystem.
pub struct GsettingSubsystem;

impl Subsystem for GsettingSubsystem {
    fn name(&self) -> &'static str {
        "GSettings"
    }

    fn id(&self) -> &'static str {
        "gsetting"
    }

    fn load_manifest(&self, ctx: &SubsystemContext) -> Result<Box<dyn Manifest>> {
        let system = GSettingsManifest::load(&ctx.system_manifest_path("gsettings.json"))?;
        let user = GSettingsManifest::load(&ctx.user_manifest_path("gsettings.json"))?;
        Ok(Box::new(GSettingsManifest::merged(&system, &user)))
    }

    fn capture(&self, _ctx: &PlanContext) -> Result<Option<Box<dyn DynPlan>>> {
        // GSettings capture requires a schema argument, so we don't support
        // full capture from the subsystem interface. Users should use
        // `bkt gsetting capture <schema>` directly.
        Ok(None)
    }

    fn sync(&self, ctx: &PlanContext) -> Result<Option<Box<dyn DynPlan>>> {
        let plan = GsettingApplyCommand.plan(ctx)?;
        if plan.is_empty() {
            Ok(None)
        } else {
            Ok(Some(Box::new(plan)))
        }
    }

    fn supports_capture(&self) -> bool {
        // GSettings capture requires schema argument
        false
    }

    fn supports_drift(&self) -> bool {
        true
    }
}

impl Manifest for GSettingsManifest {
    fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }
}

// ----------------------------------------------------------------------------
// Shim Subsystem
// ----------------------------------------------------------------------------

use crate::commands::shim::ShimSyncCommand;
use crate::manifest::ShimsManifest;

/// Host shims subsystem.
pub struct ShimSubsystem;

impl Subsystem for ShimSubsystem {
    fn name(&self) -> &'static str {
        "Host Shims"
    }

    fn id(&self) -> &'static str {
        "shim"
    }

    fn load_manifest(&self, ctx: &SubsystemContext) -> Result<Box<dyn Manifest>> {
        let system = ShimsManifest::load(&ctx.system_manifest_path("host-shims.json"))?;
        let user = ShimsManifest::load(&ctx.user_manifest_path("host-shims.json"))?;
        Ok(Box::new(ShimsManifest::merged(&system, &user)))
    }

    fn capture(&self, _ctx: &PlanContext) -> Result<Option<Box<dyn DynPlan>>> {
        // Shims don't have a capture command - they're defined in manifests only
        Ok(None)
    }

    fn sync(&self, ctx: &PlanContext) -> Result<Option<Box<dyn DynPlan>>> {
        let plan = ShimSyncCommand.plan(ctx)?;
        if plan.is_empty() {
            Ok(None)
        } else {
            Ok(Some(Box::new(plan)))
        }
    }

    fn supports_capture(&self) -> bool {
        false
    }

    fn supports_drift(&self) -> bool {
        true
    }
}

impl Manifest for ShimsManifest {
    fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }
}

// ----------------------------------------------------------------------------
// AppImage Subsystem
// ----------------------------------------------------------------------------

use crate::commands::appimage::{AppImageCaptureCommand, AppImageSyncCommand};
use crate::manifest::AppImageAppsManifest;

/// AppImage (GearLever) subsystem.
pub struct AppImageSubsystem;

impl Subsystem for AppImageSubsystem {
    fn name(&self) -> &'static str {
        "AppImages"
    }

    fn id(&self) -> &'static str {
        "appimage"
    }

    fn load_manifest(&self, ctx: &SubsystemContext) -> Result<Box<dyn Manifest>> {
        // AppImage uses a manifest in the repo's manifests/ directory
        let manifest = AppImageAppsManifest::load_from_dir(&ctx.repo_root.join("manifests"))?;
        Ok(Box::new(manifest))
    }

    fn capture(&self, ctx: &PlanContext) -> Result<Option<Box<dyn DynPlan>>> {
        let plan = AppImageCaptureCommand.plan(ctx)?;
        if plan.is_empty() {
            Ok(None)
        } else {
            Ok(Some(Box::new(plan)))
        }
    }

    fn sync(&self, ctx: &PlanContext) -> Result<Option<Box<dyn DynPlan>>> {
        let cmd = AppImageSyncCommand {
            keep_unmanaged: false,
        };
        let plan = cmd.plan(ctx)?;
        if plan.is_empty() {
            Ok(None)
        } else {
            Ok(Some(Box::new(plan)))
        }
    }
}

impl Manifest for AppImageAppsManifest {
    fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }
}

// ----------------------------------------------------------------------------
// Homebrew Subsystem
// ----------------------------------------------------------------------------

use crate::commands::homebrew::{HomebrewCaptureCommand, HomebrewSyncCommand};
use crate::manifest::homebrew::HomebrewManifest;

/// Homebrew/Linuxbrew subsystem.
pub struct HomebrewSubsystem;

impl Subsystem for HomebrewSubsystem {
    fn name(&self) -> &'static str {
        "Homebrew"
    }

    fn id(&self) -> &'static str {
        "homebrew"
    }

    fn load_manifest(&self, ctx: &SubsystemContext) -> Result<Box<dyn Manifest>> {
        let system = HomebrewManifest::load(&ctx.system_manifest_path("homebrew.json"))?;
        let user = HomebrewManifest::load(&ctx.user_manifest_path("homebrew.json"))?;
        Ok(Box::new(HomebrewManifest::merged(&system, &user)))
    }

    fn capture(&self, ctx: &PlanContext) -> Result<Option<Box<dyn DynPlan>>> {
        let plan = HomebrewCaptureCommand.plan(ctx)?;
        if plan.is_empty() {
            Ok(None)
        } else {
            Ok(Some(Box::new(plan)))
        }
    }

    fn sync(&self, ctx: &PlanContext) -> Result<Option<Box<dyn DynPlan>>> {
        let plan = HomebrewSyncCommand.plan(ctx)?;
        if plan.is_empty() {
            Ok(None)
        } else {
            Ok(Some(Box::new(plan)))
        }
    }
}

impl Manifest for HomebrewManifest {
    fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }
}

// ----------------------------------------------------------------------------
// System Subsystem
// ----------------------------------------------------------------------------

use crate::commands::system::SystemCaptureCommand;
use crate::manifest::SystemPackagesManifest;

/// System packages (rpm-ostree layered) subsystem.
pub struct SystemSubsystem;

impl Subsystem for SystemSubsystem {
    fn name(&self) -> &'static str {
        "System Packages"
    }

    fn id(&self) -> &'static str {
        "system"
    }

    fn load_manifest(&self, ctx: &SubsystemContext) -> Result<Box<dyn Manifest>> {
        let system =
            SystemPackagesManifest::load(&ctx.system_manifest_path("system-packages.json"))?;
        let user = SystemPackagesManifest::load(&ctx.user_manifest_path("system-packages.json"))?;
        Ok(Box::new(SystemPackagesManifest::merged(&system, &user)))
    }

    fn capture(&self, ctx: &PlanContext) -> Result<Option<Box<dyn DynPlan>>> {
        let plan = SystemCaptureCommand.plan(ctx)?;
        if plan.is_empty() {
            Ok(None)
        } else {
            Ok(Some(Box::new(plan)))
        }
    }

    fn sync(&self, _ctx: &PlanContext) -> Result<Option<Box<dyn DynPlan>>> {
        // System packages are synced at image build time, not runtime
        Ok(None)
    }

    fn supports_sync(&self) -> bool {
        // System packages cannot be synced at runtime
        false
    }

    fn supports_drift(&self) -> bool {
        true
    }
}

impl Manifest for SystemPackagesManifest {
    fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_all_subsystems() {
        let registry = SubsystemRegistry::builtin();
        let all = registry.all();

        // Should have all 8 subsystems
        assert_eq!(all.len(), 8);

        // Verify expected IDs
        let ids: Vec<_> = all.iter().map(|s| s.id()).collect();
        assert!(ids.contains(&"extension"));
        assert!(ids.contains(&"flatpak"));
        assert!(ids.contains(&"distrobox"));
        assert!(ids.contains(&"gsetting"));
        assert!(ids.contains(&"shim"));
        assert!(ids.contains(&"appimage"));
        assert!(ids.contains(&"homebrew"));
        assert!(ids.contains(&"system"));
    }

    #[test]
    fn test_registry_filtered() {
        let registry = SubsystemRegistry::builtin();

        // Include only extension and flatpak
        let selected = registry.filtered(Some(&["extension", "flatpak"]), &[]);
        assert_eq!(selected.len(), 2);

        // Exclude gsetting
        let selected = registry.filtered(None, &["gsetting"]);
        assert_eq!(selected.len(), 7);

        // Include extension but exclude it (exclude wins)
        let selected = registry.filtered(Some(&["extension"]), &["extension"]);
        assert_eq!(selected.len(), 0);
    }

    #[test]
    fn test_registry_find() {
        let registry = SubsystemRegistry::builtin();

        assert!(registry.find("extension").is_some());
        assert!(registry.find("nonexistent").is_none());
    }

    #[test]
    fn test_capturable_subsystems() {
        let registry = SubsystemRegistry::builtin();
        let capturable = registry.capturable();

        // gsetting and shim don't support full capture
        let ids: Vec<_> = capturable.iter().map(|s| s.id()).collect();
        assert!(ids.contains(&"extension"));
        assert!(ids.contains(&"flatpak"));
        assert!(!ids.contains(&"gsetting"));
        assert!(!ids.contains(&"shim"));
    }

    #[test]
    fn test_syncable_subsystems() {
        let registry = SubsystemRegistry::builtin();
        let syncable = registry.syncable();

        // system doesn't support sync
        let ids: Vec<_> = syncable.iter().map(|s| s.id()).collect();
        assert!(ids.contains(&"extension"));
        assert!(ids.contains(&"flatpak"));
        assert!(ids.contains(&"gsetting"));
        assert!(!ids.contains(&"system"));
    }

    #[test]
    fn test_subsystem_context_default() {
        let ctx = SubsystemContext::new();

        assert!(
            ctx.system_manifest_dir
                .to_string_lossy()
                .contains("bootc-bootstrap")
        );
        assert!(ctx.user_config_dir.to_string_lossy().contains("bootc"));
    }

    #[test]
    fn test_subsystem_context_paths() {
        let ctx = SubsystemContext::with_repo_root(PathBuf::from("/test/repo"));

        assert_eq!(
            ctx.system_manifest_path("test.json"),
            PathBuf::from("/usr/share/bootc-bootstrap/test.json")
        );
        assert_eq!(
            ctx.repo_manifest_path("test.json"),
            PathBuf::from("/test/repo/manifests/test.json")
        );
    }
}
