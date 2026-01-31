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

/// Execution phase for a subsystem.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ExecutionPhase {
    /// Infrastructure such as remotes, registries, or repositories.
    Infrastructure,
    /// Installable packages (flatpaks, system packages, appimages, homebrew).
    Packages,
    /// Configuration that depends on earlier phases.
    Configuration,
}

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
///     fn sync(
///         &self,
///         ctx: &PlanContext,
///         _config: &SubsystemConfig,
///     ) -> Result<Option<Box<dyn DynPlan>>> {
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

    /// Execution phase for ordering.
    fn phase(&self) -> ExecutionPhase {
        ExecutionPhase::Configuration
    }

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
    fn sync(&self, ctx: &PlanContext, config: &SubsystemConfig)
    -> Result<Option<Box<dyn DynPlan>>>;

    /// Calculate subsystem status (manifest vs system).
    ///
    /// Returns `Ok(None)` if this subsystem doesn't support status.
    fn status(&self, _ctx: &SubsystemContext) -> Result<Option<Box<dyn SubsystemStatus>>> {
        Ok(None)
    }

    /// Calculate drift between manifest and system state.
    ///
    /// Returns `Ok(None)` if this subsystem doesn't support drift detection.
    fn drift(&self, _ctx: &SubsystemContext) -> Result<Option<DriftReport>> {
        Ok(None)
    }

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

/// Status summary for a subsystem.
pub trait SubsystemStatus: std::fmt::Debug + Send + Sync {
    fn total(&self) -> usize;
    fn synced(&self) -> usize;
    fn pending(&self) -> usize;
    fn untracked(&self) -> usize;
}

#[derive(Debug, Default)]
pub struct DriftReport {
    /// In manifest, should exist
    pub expected: Vec<String>,
    /// Actually on system
    pub actual: Vec<String>,
    /// In manifest but not on system
    pub missing: Vec<String>,
    /// On system but not in manifest
    pub extra: Vec<String>,
}

impl DriftReport {
    pub fn has_drift(&self) -> bool {
        !self.missing.is_empty() || !self.extra.is_empty()
    }
}

#[derive(Debug, Clone, Copy)]
struct BasicSubsystemStatus {
    total: usize,
    synced: usize,
    pending: usize,
    untracked: usize,
}

impl SubsystemStatus for BasicSubsystemStatus {
    fn total(&self) -> usize {
        self.total
    }

    fn synced(&self) -> usize {
        self.synced
    }

    fn pending(&self) -> usize {
        self.pending
    }

    fn untracked(&self) -> usize {
        self.untracked
    }
}

fn build_drift_report(mut expected: Vec<String>, mut actual: Vec<String>) -> DriftReport {
    use std::collections::HashSet;

    let expected_set: HashSet<String> = expected.iter().cloned().collect();
    let actual_set: HashSet<String> = actual.iter().cloned().collect();

    let mut missing: Vec<String> = expected_set.difference(&actual_set).cloned().collect();
    let mut extra: Vec<String> = actual_set.difference(&expected_set).cloned().collect();

    expected.sort();
    actual.sort();
    missing.sort();
    extra.sort();

    DriftReport {
        expected,
        actual,
        missing,
        extra,
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
// Subsystem Configuration
// ============================================================================

#[derive(Debug, Default, Clone)]
pub struct SubsystemConfig {
    pub appimage_prune: bool,
    // Future: other per-subsystem config
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
                Box::new(FetchbinSubsystem),
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

    /// Get subsystems ordered by execution phase.
    pub fn by_phase(&self) -> Vec<&dyn Subsystem> {
        self.ordered_by_phase(false)
    }

    fn ordered_by_phase(&self, reverse: bool) -> Vec<&dyn Subsystem> {
        let mut subsystems: Vec<(usize, &dyn Subsystem)> = self
            .subsystems
            .iter()
            .enumerate()
            .map(|(idx, s)| (idx, s.as_ref()))
            .collect();

        subsystems.sort_by_key(|(idx, s)| (Self::phase_sort_key(s.phase(), reverse), *idx));

        subsystems.into_iter().map(|(_, s)| s).collect()
    }

    fn phase_sort_key(phase: ExecutionPhase, reverse: bool) -> u8 {
        let rank = match phase {
            ExecutionPhase::Infrastructure => 0,
            ExecutionPhase::Packages => 1,
            ExecutionPhase::Configuration => 2,
        };

        if reverse { 2 - rank } else { rank }
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
        self.ordered_by_phase(true)
            .into_iter()
            .filter(|s| s.supports_capture())
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
        self.by_phase()
            .into_iter()
            .filter(|s| s.supports_sync())
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
use crate::context::run_command;
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

    fn phase(&self) -> ExecutionPhase {
        ExecutionPhase::Configuration
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

    fn sync(
        &self,
        ctx: &PlanContext,
        _config: &SubsystemConfig,
    ) -> Result<Option<Box<dyn DynPlan>>> {
        let plan = ExtensionSyncCommand.plan(ctx)?;
        if plan.is_empty() {
            Ok(None)
        } else {
            Ok(Some(Box::new(plan)))
        }
    }

    fn status(&self, ctx: &SubsystemContext) -> Result<Option<Box<dyn SubsystemStatus>>> {
        let system =
            GnomeExtensionsManifest::load(&ctx.system_manifest_path("gnome-extensions.json"))?;
        let user = GnomeExtensionsManifest::load(&ctx.user_manifest_path("gnome-extensions.json"))?;
        let merged = GnomeExtensionsManifest::merged(&system, &user);

        let enabled_extensions: std::collections::HashSet<String> =
            get_enabled_extensions().into_iter().collect();
        let manifest_uuids: std::collections::HashSet<_> =
            merged.extensions.iter().map(|s| s.id()).collect();

        let total = merged.extensions.len();
        let synced = merged
            .extensions
            .iter()
            .filter(|u| enabled_extensions.contains(u.id()))
            .count();
        let pending = total.saturating_sub(synced);

        let untracked = enabled_extensions
            .iter()
            .filter(|uuid| !manifest_uuids.contains(uuid.as_str()))
            .count();

        Ok(Some(Box::new(BasicSubsystemStatus {
            total,
            synced,
            pending,
            untracked,
        })))
    }

    fn drift(&self, ctx: &SubsystemContext) -> Result<Option<DriftReport>> {
        let system =
            GnomeExtensionsManifest::load(&ctx.system_manifest_path("gnome-extensions.json"))?;
        let user = GnomeExtensionsManifest::load(&ctx.user_manifest_path("gnome-extensions.json"))?;
        let merged = GnomeExtensionsManifest::merged(&system, &user);

        let expected: Vec<String> = merged
            .extensions
            .iter()
            .map(|s| s.id().to_string())
            .collect();
        let actual = get_enabled_extensions();

        Ok(Some(build_drift_report(expected, actual)))
    }

    fn supports_drift(&self) -> bool {
        true
    }
}

/// Get list of enabled GNOME extension UUIDs.
fn get_enabled_extensions() -> Vec<String> {
    let output = run_command("gnome-extensions", &["list", "--enabled"]);

    match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .lines()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
        _ => Vec::new(),
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

use crate::commands::flatpak::{FlatpakCaptureCommand, FlatpakSyncCommand, get_installed_flatpaks};
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

    fn phase(&self) -> ExecutionPhase {
        ExecutionPhase::Packages
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

    fn sync(
        &self,
        ctx: &PlanContext,
        _config: &SubsystemConfig,
    ) -> Result<Option<Box<dyn DynPlan>>> {
        let plan = FlatpakSyncCommand.plan(ctx)?;
        if plan.is_empty() {
            Ok(None)
        } else {
            Ok(Some(Box::new(plan)))
        }
    }

    fn status(&self, ctx: &SubsystemContext) -> Result<Option<Box<dyn SubsystemStatus>>> {
        let system = FlatpakAppsManifest::load(&ctx.system_manifest_path("flatpak-apps.json"))?;
        let user = FlatpakAppsManifest::load(&ctx.user_manifest_path("flatpak-apps.json"))?;
        let merged = FlatpakAppsManifest::merged(&system, &user);

        let installed_flatpaks: std::collections::HashSet<String> =
            get_installed_flatpaks().into_iter().map(|f| f.id).collect();
        let manifest_ids: std::collections::HashSet<_> =
            merged.apps.iter().map(|a| a.id.as_str()).collect();

        let total = merged.apps.len();
        let synced = merged
            .apps
            .iter()
            .filter(|a| installed_flatpaks.contains(&a.id))
            .count();
        let pending = total.saturating_sub(synced);

        let untracked = installed_flatpaks
            .iter()
            .filter(|id| !manifest_ids.contains(id.as_str()))
            .count();

        Ok(Some(Box::new(BasicSubsystemStatus {
            total,
            synced,
            pending,
            untracked,
        })))
    }

    fn drift(&self, ctx: &SubsystemContext) -> Result<Option<DriftReport>> {
        let system = FlatpakAppsManifest::load(&ctx.system_manifest_path("flatpak-apps.json"))?;
        let user = FlatpakAppsManifest::load(&ctx.user_manifest_path("flatpak-apps.json"))?;
        let merged = FlatpakAppsManifest::merged(&system, &user);

        let expected: Vec<String> = merged.apps.iter().map(|a| a.id.clone()).collect();
        let actual: Vec<String> = get_installed_flatpaks().into_iter().map(|f| f.id).collect();

        Ok(Some(build_drift_report(expected, actual)))
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

    fn phase(&self) -> ExecutionPhase {
        ExecutionPhase::Infrastructure
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

    fn sync(
        &self,
        ctx: &PlanContext,
        _config: &SubsystemConfig,
    ) -> Result<Option<Box<dyn DynPlan>>> {
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

    fn phase(&self) -> ExecutionPhase {
        ExecutionPhase::Configuration
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

    fn sync(
        &self,
        ctx: &PlanContext,
        _config: &SubsystemConfig,
    ) -> Result<Option<Box<dyn DynPlan>>> {
        let plan = GsettingApplyCommand.plan(ctx)?;
        if plan.is_empty() {
            Ok(None)
        } else {
            Ok(Some(Box::new(plan)))
        }
    }

    fn status(&self, ctx: &SubsystemContext) -> Result<Option<Box<dyn SubsystemStatus>>> {
        let system = GSettingsManifest::load(&ctx.system_manifest_path("gsettings.json"))?;
        let user = GSettingsManifest::load(&ctx.user_manifest_path("gsettings.json"))?;
        let merged = GSettingsManifest::merged(&system, &user);

        let total = merged.settings.len();
        let mut synced = 0;
        let mut pending = 0;

        for s in &merged.settings {
            match get_gsetting(&s.schema, &s.key) {
                Some(current) if current == s.value => synced += 1,
                Some(_) => pending += 1,
                None => pending += 1,
            }
        }

        Ok(Some(Box::new(BasicSubsystemStatus {
            total,
            synced,
            pending,
            untracked: 0,
        })))
    }

    fn drift(&self, ctx: &SubsystemContext) -> Result<Option<DriftReport>> {
        let system = GSettingsManifest::load(&ctx.system_manifest_path("gsettings.json"))?;
        let user = GSettingsManifest::load(&ctx.user_manifest_path("gsettings.json"))?;
        let merged = GSettingsManifest::merged(&system, &user);

        let mut report = DriftReport::default();

        for setting in &merged.settings {
            let key = format!("{}.{}", setting.schema, setting.key);
            let expected_entry = format!("{} = {}", key, setting.value);
            report.expected.push(expected_entry);

            match get_gsetting(&setting.schema, &setting.key) {
                Some(current) => {
                    report.actual.push(format!("{} = {}", key, current));
                    if current != setting.value {
                        report.missing.push(format!(
                            "{} (expected {}, actual {})",
                            key, setting.value, current
                        ));
                    }
                }
                None => {
                    report.missing.push(format!(
                        "{} (expected {}, actual <unset>)",
                        key, setting.value
                    ));
                }
            }
        }

        report.expected.sort();
        report.actual.sort();
        report.missing.sort();

        Ok(Some(report))
    }

    fn supports_capture(&self) -> bool {
        // GSettings capture requires schema argument
        false
    }

    fn supports_drift(&self) -> bool {
        true
    }
}

/// Get current value of a gsetting.
fn get_gsetting(schema: &str, key: &str) -> Option<String> {
    run_command("gsettings", &["get", schema, key])
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
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

    fn phase(&self) -> ExecutionPhase {
        ExecutionPhase::Configuration
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

    fn sync(
        &self,
        ctx: &PlanContext,
        _config: &SubsystemConfig,
    ) -> Result<Option<Box<dyn DynPlan>>> {
        let plan = ShimSyncCommand.plan(ctx)?;
        if plan.is_empty() {
            Ok(None)
        } else {
            Ok(Some(Box::new(plan)))
        }
    }

    fn status(&self, ctx: &SubsystemContext) -> Result<Option<Box<dyn SubsystemStatus>>> {
        let system = ShimsManifest::load(&ctx.system_manifest_path("host-shims.json"))?;
        let user = ShimsManifest::load(&ctx.user_manifest_path("host-shims.json"))?;
        let merged = ShimsManifest::merged(&system, &user);

        let shims_dir = shims_dir();
        let total = merged.shims.len();
        let synced = merged
            .shims
            .iter()
            .filter(|s| shims_dir.join(&s.name).exists())
            .count();
        let pending = total.saturating_sub(synced);

        Ok(Some(Box::new(BasicSubsystemStatus {
            total,
            synced,
            pending,
            untracked: 0,
        })))
    }

    fn drift(&self, ctx: &SubsystemContext) -> Result<Option<DriftReport>> {
        let system = ShimsManifest::load(&ctx.system_manifest_path("host-shims.json"))?;
        let user = ShimsManifest::load(&ctx.user_manifest_path("host-shims.json"))?;
        let merged = ShimsManifest::merged(&system, &user);

        let expected: Vec<String> = merged.shims.iter().map(|s| s.name.clone()).collect();
        let actual = get_installed_shims();

        Ok(Some(build_drift_report(expected, actual)))
    }

    fn supports_capture(&self) -> bool {
        false
    }

    fn supports_drift(&self) -> bool {
        true
    }
}

/// Get the shims directory.
fn shims_dir() -> PathBuf {
    ShimsManifest::shims_dir()
}

fn get_installed_shims() -> Vec<String> {
    let dir = shims_dir();
    if !dir.exists() {
        return Vec::new();
    }

    let mut shims = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            if let Ok(file_type) = entry.file_type()
                && file_type.is_file()
                && let Some(name) = entry.file_name().to_str()
            {
                shims.push(name.to_string());
            }
        }
    }

    shims
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

    fn phase(&self) -> ExecutionPhase {
        ExecutionPhase::Packages
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

    fn sync(
        &self,
        ctx: &PlanContext,
        config: &SubsystemConfig,
    ) -> Result<Option<Box<dyn DynPlan>>> {
        let cmd = AppImageSyncCommand {
            keep_unmanaged: !config.appimage_prune,
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
// Fetchbin Subsystem
// ----------------------------------------------------------------------------

use crate::commands::fetchbin::{FetchbinCaptureCommand, FetchbinSyncCommand};
use crate::manifest::HostBinariesManifest;

/// Fetchbin host binaries subsystem.
pub struct FetchbinSubsystem;

impl Subsystem for FetchbinSubsystem {
    fn name(&self) -> &'static str {
        "Fetchbin"
    }

    fn id(&self) -> &'static str {
        "fetchbin"
    }

    fn phase(&self) -> ExecutionPhase {
        ExecutionPhase::Packages
    }

    fn load_manifest(&self, ctx: &SubsystemContext) -> Result<Box<dyn Manifest>> {
        let manifest = HostBinariesManifest::load_from_dir(&ctx.repo_root.join("manifests"))?;
        Ok(Box::new(manifest))
    }

    fn capture(&self, ctx: &PlanContext) -> Result<Option<Box<dyn DynPlan>>> {
        let plan = FetchbinCaptureCommand.plan(ctx)?;
        if plan.is_empty() {
            Ok(None)
        } else {
            Ok(Some(Box::new(plan)))
        }
    }

    fn sync(
        &self,
        ctx: &PlanContext,
        _config: &SubsystemConfig,
    ) -> Result<Option<Box<dyn DynPlan>>> {
        let plan = FetchbinSyncCommand.plan(ctx)?;
        if plan.is_empty() {
            Ok(None)
        } else {
            Ok(Some(Box::new(plan)))
        }
    }
}

impl Manifest for HostBinariesManifest {
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

    fn phase(&self) -> ExecutionPhase {
        ExecutionPhase::Packages
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

    fn sync(
        &self,
        ctx: &PlanContext,
        _config: &SubsystemConfig,
    ) -> Result<Option<Box<dyn DynPlan>>> {
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

    fn phase(&self) -> ExecutionPhase {
        ExecutionPhase::Packages
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

    fn sync(
        &self,
        _ctx: &PlanContext,
        _config: &SubsystemConfig,
    ) -> Result<Option<Box<dyn DynPlan>>> {
        // System packages are synced at image build time, not runtime
        Ok(None)
    }

    fn drift(&self, ctx: &SubsystemContext) -> Result<Option<DriftReport>> {
        let system =
            SystemPackagesManifest::load(&ctx.system_manifest_path("system-packages.json"))?;
        let user = SystemPackagesManifest::load(&ctx.user_manifest_path("system-packages.json"))?;
        let merged = SystemPackagesManifest::merged(&system, &user);

        let expected = merged.packages;
        let actual = get_layered_packages();

        Ok(Some(build_drift_report(expected, actual)))
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

fn get_layered_packages() -> Vec<String> {
    let output = run_command("rpm-ostree", &["status", "--json"]);

    let output = match output {
        Ok(o) if o.status.success() => o,
        _ => return Vec::new(),
    };

    let json: serde_json::Value = match serde_json::from_slice(&output.stdout) {
        Ok(j) => j,
        Err(_) => return Vec::new(),
    };

    let deployments = match json.get("deployments").and_then(|d| d.as_array()) {
        Some(d) => d,
        None => return Vec::new(),
    };

    let booted = match deployments
        .iter()
        .find(|d| d.get("booted").and_then(|b| b.as_bool()).unwrap_or(false))
    {
        Some(b) => b,
        None => return Vec::new(),
    };

    booted
        .get("requested-packages")
        .or_else(|| booted.get("requested_packages"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
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
    fn test_registry_phase_ordering() {
        let registry = SubsystemRegistry::builtin();
        let ids: Vec<_> = registry.by_phase().iter().map(|s| s.id()).collect();

        assert_eq!(
            ids,
            vec![
                "distrobox",
                "flatpak",
                "appimage",
                "homebrew",
                "system",
                "extension",
                "gsetting",
                "shim",
            ]
        );
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

        assert_eq!(
            ids,
            vec![
                "extension",
                "flatpak",
                "appimage",
                "homebrew",
                "system",
                "distrobox"
            ]
        );
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
