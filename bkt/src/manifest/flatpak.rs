//! Flatpak manifest types.

use anyhow::{Context, Result};
use directories::BaseDirs;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

// ============================================================================
// Flatpak Override Parsing
// ============================================================================

/// Parsed representation of a flatpak override file.
///
/// Override files are INI-style and typically located at:
/// - User scope: `~/.local/share/flatpak/overrides/{app_id}`
/// - System scope: `/var/lib/flatpak/overrides/{app_id}`
#[derive(Debug, Clone, Default)]
pub struct FlatpakOverrides {
    /// Filesystem permissions (e.g., "~/Documents:rw", "!~/Private")
    pub filesystems: Vec<String>,
    /// Device permissions (e.g., "all", "dri")
    pub devices: Vec<String>,
    /// Shared namespaces (e.g., "network", "ipc")
    pub shared: Vec<String>,
    /// Socket permissions (e.g., "wayland", "x11", "pulseaudio")
    pub sockets: Vec<String>,
    /// Persistent directories
    pub persistent: Vec<String>,
    /// Environment variables
    pub environment: HashMap<String, String>,
    /// Session bus policies (bus name -> policy like "talk", "own", "see")
    pub session_bus_policy: HashMap<String, String>,
    /// System bus policies (bus name -> policy like "talk", "own", "see")
    pub system_bus_policy: HashMap<String, String>,
}

impl FlatpakOverrides {
    /// Check if overrides are empty (no permissions set).
    pub fn is_empty(&self) -> bool {
        self.filesystems.is_empty()
            && self.devices.is_empty()
            && self.shared.is_empty()
            && self.sockets.is_empty()
            && self.persistent.is_empty()
            && self.environment.is_empty()
            && self.session_bus_policy.is_empty()
            && self.system_bus_policy.is_empty()
    }

    /// Parse from INI file content.
    ///
    /// The format is:
    /// ```ini
    /// [Context]
    /// filesystems=~/Documents:rw;!~/Private
    /// devices=all
    ///
    /// [Environment]
    /// GTK_THEME=Adwaita-dark
    ///
    /// [Session Bus Policy]
    /// org.freedesktop.secrets=talk
    /// ```
    pub fn from_ini(content: &str) -> Self {
        let mut overrides = Self::default();
        let mut current_section = String::new();

        for line in content.lines() {
            let line = line.trim();

            // Skip empty lines and comments
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }

            // Section header
            if line.starts_with('[') && line.ends_with(']') {
                current_section = line[1..line.len() - 1].to_lowercase();
                continue;
            }

            // Key=value pair
            if let Some((key, value)) = line.split_once('=') {
                let key = key.trim().to_lowercase();
                let value = value.trim();

                match current_section.as_str() {
                    "context" => match key.as_str() {
                        "filesystems" => {
                            overrides
                                .filesystems
                                .extend(Self::parse_semicolon_list(value));
                        }
                        "devices" => {
                            overrides.devices.extend(Self::parse_semicolon_list(value));
                        }
                        "shared" => {
                            overrides.shared.extend(Self::parse_semicolon_list(value));
                        }
                        "sockets" => {
                            overrides.sockets.extend(Self::parse_semicolon_list(value));
                        }
                        "persistent" => {
                            overrides
                                .persistent
                                .extend(Self::parse_semicolon_list(value));
                        }
                        _ => {}
                    },
                    "environment" => {
                        // In Environment section, the key IS the env var name
                        // and value is the env var value
                        overrides
                            .environment
                            .insert(key.to_uppercase(), value.to_string());
                    }
                    "session bus policy" => {
                        // Key is bus name (preserve case), value is policy
                        let bus_name = line.split_once('=').map(|(k, _)| k.trim()).unwrap_or(&key);
                        overrides
                            .session_bus_policy
                            .insert(bus_name.to_string(), value.to_string());
                    }
                    "system bus policy" => {
                        let bus_name = line.split_once('=').map(|(k, _)| k.trim()).unwrap_or(&key);
                        overrides
                            .system_bus_policy
                            .insert(bus_name.to_string(), value.to_string());
                    }
                    _ => {}
                }
            }
        }

        overrides
    }

    /// Parse semicolon-separated list, filtering empty values.
    fn parse_semicolon_list(value: &str) -> Vec<String> {
        value
            .split(';')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    }

    /// Convert to CLI flags for manifest storage.
    ///
    /// Each override becomes a flag like:
    /// - `--filesystem=~/Documents:rw`
    /// - `--device=all`
    /// - `--env=GTK_THEME=Adwaita-dark`
    /// - `--talk-name=org.freedesktop.secrets`
    pub fn to_cli_flags(&self) -> Vec<String> {
        let mut flags = Vec::new();

        // Filesystems - handle negation
        for fs in &self.filesystems {
            if let Some(path) = fs.strip_prefix('!') {
                flags.push(format!("--nofilesystem={}", path));
            } else {
                flags.push(format!("--filesystem={}", fs));
            }
        }

        // Devices - handle negation
        for dev in &self.devices {
            if let Some(name) = dev.strip_prefix('!') {
                flags.push(format!("--nodevice={}", name));
            } else {
                flags.push(format!("--device={}", dev));
            }
        }

        // Shared - handle negation
        for s in &self.shared {
            if let Some(name) = s.strip_prefix('!') {
                flags.push(format!("--unshare={}", name));
            } else {
                flags.push(format!("--share={}", s));
            }
        }

        // Sockets - handle negation
        for sock in &self.sockets {
            if let Some(name) = sock.strip_prefix('!') {
                flags.push(format!("--nosocket={}", name));
            } else {
                flags.push(format!("--socket={}", sock));
            }
        }

        // Persistent directories
        for p in &self.persistent {
            flags.push(format!("--persist={}", p));
        }

        // Environment variables
        for (key, value) in &self.environment {
            flags.push(format!("--env={}={}", key, value));
        }

        // Session bus policy
        for (name, policy) in &self.session_bus_policy {
            match policy.as_str() {
                "talk" => flags.push(format!("--talk-name={}", name)),
                "own" => flags.push(format!("--own-name={}", name)),
                "see" => flags.push(format!("--see-name={}", name)),
                "none" => flags.push(format!("--no-talk-name={}", name)),
                _ => flags.push(format!("--talk-name={}", name)), // default to talk
            }
        }

        // System bus policy
        for (name, policy) in &self.system_bus_policy {
            match policy.as_str() {
                "talk" => flags.push(format!("--system-talk-name={}", name)),
                "own" => flags.push(format!("--system-own-name={}", name)),
                "see" => flags.push(format!("--system-see-name={}", name)),
                "none" => flags.push(format!("--system-no-talk-name={}", name)),
                _ => flags.push(format!("--system-talk-name={}", name)),
            }
        }

        // Sort for consistent output
        flags.sort();
        flags
    }

    /// Load override file for an app at the given scope.
    ///
    /// Returns `None` if no override file exists.
    pub fn load_for_app(app_id: &str, scope: FlatpakScope) -> Option<Self> {
        let path = Self::override_file_path(app_id, scope);
        Self::load_from_path(&path)
    }

    /// Load overrides from a file path.
    pub fn load_from_path(path: &Path) -> Option<Self> {
        let content = fs::read_to_string(path).ok()?;
        let overrides = Self::from_ini(&content);
        if overrides.is_empty() {
            None
        } else {
            Some(overrides)
        }
    }

    /// Get the path to the override file for an app.
    pub fn override_file_path(app_id: &str, scope: FlatpakScope) -> PathBuf {
        match scope {
            FlatpakScope::User => {
                // Prefer $HOME for test isolation
                let data_dir = std::env::var("HOME")
                    .ok()
                    .map(|h| PathBuf::from(h).join(".local/share"))
                    .or_else(|| BaseDirs::new().map(|d| d.data_local_dir().to_path_buf()))
                    .unwrap_or_else(|| PathBuf::from(".local/share"));
                data_dir.join("flatpak/overrides").join(app_id)
            }
            FlatpakScope::System => PathBuf::from("/var/lib/flatpak/overrides").join(app_id),
        }
    }
}

/// Scope for Flatpak apps and remotes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum FlatpakScope {
    #[default]
    System,
    User,
}

impl std::fmt::Display for FlatpakScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FlatpakScope::System => write!(f, "system"),
            FlatpakScope::User => write!(f, "user"),
        }
    }
}

impl std::str::FromStr for FlatpakScope {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "system" => Ok(FlatpakScope::System),
            "user" => Ok(FlatpakScope::User),
            _ => anyhow::bail!("Invalid scope: {}. Use 'system' or 'user'", s),
        }
    }
}

/// A Flatpak application entry.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FlatpakApp {
    /// Application ID (e.g., "org.gnome.Calculator")
    pub id: String,
    /// Remote name (e.g., "flathub")
    pub remote: String,
    /// Installation scope
    pub scope: FlatpakScope,
    /// Branch (e.g., "stable", "1.2")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// Commit hash
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    /// Overrides (e.g. "--filesystem=home")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overrides: Option<Vec<String>>,
}

/// The flatpak-apps.json manifest.
#[derive(Debug, Clone, Serialize, Deserialize, Default, JsonSchema)]
pub struct FlatpakAppsManifest {
    #[serde(rename = "$schema", skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    pub apps: Vec<FlatpakApp>,
}

impl FlatpakAppsManifest {
    /// Project manifest path (relative to workspace root).
    pub const PROJECT_PATH: &'static str = "manifests/flatpak-apps.json";

    /// Load a manifest from a path.
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read flatpak manifest from {}", path.display()))?;
        let manifest: Self = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse flatpak manifest from {}", path.display()))?;
        Ok(manifest)
    }

    /// Save a manifest to a path.
    pub fn save(&self, path: &PathBuf) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory {}", parent.display()))?;
        }
        let content =
            serde_json::to_string_pretty(self).context("Failed to serialize flatpak manifest")?;
        fs::write(path, content)
            .with_context(|| format!("Failed to write flatpak manifest to {}", path.display()))?;
        Ok(())
    }

    /// Load from the repository's manifests directory.
    pub fn load_repo() -> Result<Self> {
        let repo = crate::repo::find_repo_path()?;
        Self::load(&repo.join(Self::PROJECT_PATH))
    }

    /// Save to the repository's manifests directory.
    pub fn save_repo(&self) -> Result<()> {
        let repo = crate::repo::find_repo_path()?;
        self.save(&repo.join(Self::PROJECT_PATH))
    }

    /// Find an app by id.
    pub fn find(&self, id: &str) -> Option<&FlatpakApp> {
        self.apps.iter().find(|a| a.id == id)
    }

    /// Add or update an app.
    pub fn upsert(&mut self, app: FlatpakApp) {
        if let Some(existing) = self.apps.iter_mut().find(|a| a.id == app.id) {
            *existing = app;
        } else {
            self.apps.push(app);
        }
        self.apps.sort_by(|a, b| a.id.cmp(&b.id));
    }

    /// Remove an app by id. Returns true if removed.
    pub fn remove(&mut self, id: &str) -> bool {
        let len_before = self.apps.len();
        self.apps.retain(|a| a.id != id);
        self.apps.len() < len_before
    }
}

/// A Flatpak remote entry.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, Serialize, Deserialize, Default, JsonSchema)]
pub struct FlatpakRemotesManifest {
    #[serde(rename = "$schema", skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    pub remotes: Vec<FlatpakRemote>,
}

impl FlatpakRemotesManifest {
    /// Load a manifest from a path.
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = fs::read_to_string(path).with_context(|| {
            format!(
                "Failed to read flatpak remotes manifest from {}",
                path.display()
            )
        })?;
        let manifest: Self = serde_json::from_str(&content).with_context(|| {
            format!(
                "Failed to parse flatpak remotes manifest from {}",
                path.display()
            )
        })?;
        Ok(manifest)
    }

    /// Load from current working directory (for manifest repos).
    pub fn load_cwd() -> Result<Self> {
        Self::load(&PathBuf::from("manifests/flatpak-remotes.json"))
    }

    /// Check if a remote name is managed by this manifest.
    pub fn has_remote(&self, name: &str) -> bool {
        self.remotes.iter().any(|r| r.name == name)
    }

    /// Get all remote names.
    #[allow(dead_code)]
    pub fn remote_names(&self) -> Vec<&str> {
        self.remotes.iter().map(|r| r.name.as_str()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_app(id: &str) -> FlatpakApp {
        FlatpakApp {
            id: id.to_string(),
            remote: "flathub".to_string(),
            scope: FlatpakScope::System,
            branch: None,
            commit: None,
            overrides: None,
        }
    }

    fn sample_app_user(id: &str) -> FlatpakApp {
        FlatpakApp {
            id: id.to_string(),
            remote: "flathub".to_string(),
            scope: FlatpakScope::User,
            branch: None,
            commit: None,
            overrides: None,
        }
    }

    // FlatpakScope tests
    #[test]
    fn scope_display() {
        assert_eq!(FlatpakScope::System.to_string(), "system");
        assert_eq!(FlatpakScope::User.to_string(), "user");
    }

    #[test]
    fn scope_from_str() {
        assert_eq!(
            "system".parse::<FlatpakScope>().unwrap(),
            FlatpakScope::System
        );
        assert_eq!("user".parse::<FlatpakScope>().unwrap(), FlatpakScope::User);
        assert_eq!("USER".parse::<FlatpakScope>().unwrap(), FlatpakScope::User);
        assert!("invalid".parse::<FlatpakScope>().is_err());
    }

    // FlatpakAppsManifest tests
    #[test]
    fn manifest_default_is_empty() {
        let manifest = FlatpakAppsManifest::default();
        assert!(manifest.apps.is_empty());
        assert!(manifest.schema.is_none());
    }

    #[test]
    fn manifest_find_returns_matching_app() {
        let mut manifest = FlatpakAppsManifest::default();
        manifest.apps.push(sample_app("org.gnome.Calculator"));

        let found = manifest.find("org.gnome.Calculator");
        assert!(found.is_some());
        assert_eq!(found.unwrap().id, "org.gnome.Calculator");
    }

    #[test]
    fn manifest_find_returns_none_for_missing() {
        let manifest = FlatpakAppsManifest::default();
        assert!(manifest.find("nonexistent").is_none());
    }

    #[test]
    fn manifest_upsert_adds_new_app() {
        let mut manifest = FlatpakAppsManifest::default();
        manifest.upsert(sample_app("org.gnome.Calculator"));

        assert_eq!(manifest.apps.len(), 1);
        assert_eq!(manifest.apps[0].id, "org.gnome.Calculator");
    }

    #[test]
    fn manifest_upsert_updates_existing_app() {
        let mut manifest = FlatpakAppsManifest::default();
        manifest.upsert(sample_app("org.gnome.Calculator"));
        manifest.upsert(sample_app_user("org.gnome.Calculator"));

        assert_eq!(manifest.apps.len(), 1);
        assert_eq!(manifest.apps[0].scope, FlatpakScope::User);
    }

    #[test]
    fn manifest_upsert_maintains_sorted_order() {
        let mut manifest = FlatpakAppsManifest::default();
        manifest.upsert(sample_app("org.z.App"));
        manifest.upsert(sample_app("org.a.App"));
        manifest.upsert(sample_app("org.m.App"));

        let ids: Vec<_> = manifest.apps.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, vec!["org.a.App", "org.m.App", "org.z.App"]);
    }

    #[test]
    fn manifest_remove_returns_true_when_found() {
        let mut manifest = FlatpakAppsManifest::default();
        manifest.apps.push(sample_app("org.gnome.Calculator"));

        assert!(manifest.remove("org.gnome.Calculator"));
        assert!(manifest.apps.is_empty());
    }

    #[test]
    fn manifest_remove_returns_false_when_not_found() {
        let mut manifest = FlatpakAppsManifest::default();
        assert!(!manifest.remove("nonexistent"));
    }

    #[test]
    fn manifest_serialization_roundtrip() {
        let mut manifest = FlatpakAppsManifest::default();
        manifest.apps.push(sample_app("org.gnome.Calculator"));
        manifest.apps.push(sample_app_user("org.custom.App"));

        let json = serde_json::to_string(&manifest).unwrap();
        let parsed: FlatpakAppsManifest = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.apps.len(), 2);
    }

    #[test]
    fn manifest_load_save_roundtrip() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let path = tmp_dir.path().join("test-flatpak.json");

        let mut manifest = FlatpakAppsManifest::default();
        manifest.apps.push(sample_app("org.gnome.Calculator"));

        manifest.save(&path).unwrap();
        let loaded = FlatpakAppsManifest::load(&path).unwrap();

        assert_eq!(loaded.apps.len(), 1);
        assert!(loaded.find("org.gnome.Calculator").is_some());
    }

    #[test]
    fn manifest_load_nonexistent_returns_default() {
        let path = PathBuf::from("/nonexistent/path/flatpak.json");
        let manifest = FlatpakAppsManifest::load(&path).unwrap();
        assert!(manifest.apps.is_empty());
    }

    // ========================================================================
    // FlatpakOverrides tests
    // ========================================================================

    #[test]
    fn overrides_from_ini_parses_context_section() {
        let ini = r#"
[Context]
filesystems=~/Documents:rw;~/Downloads:ro
devices=all;dri
shared=network;ipc
sockets=wayland;x11
"#;
        let overrides = FlatpakOverrides::from_ini(ini);

        assert_eq!(
            overrides.filesystems,
            vec!["~/Documents:rw", "~/Downloads:ro"]
        );
        assert_eq!(overrides.devices, vec!["all", "dri"]);
        assert_eq!(overrides.shared, vec!["network", "ipc"]);
        assert_eq!(overrides.sockets, vec!["wayland", "x11"]);
    }

    #[test]
    fn overrides_from_ini_parses_environment_section() {
        let ini = r#"
[Environment]
GTK_THEME=Adwaita-dark
MOZ_ENABLE_WAYLAND=1
"#;
        let overrides = FlatpakOverrides::from_ini(ini);

        assert_eq!(
            overrides.environment.get("GTK_THEME"),
            Some(&"Adwaita-dark".to_string())
        );
        assert_eq!(
            overrides.environment.get("MOZ_ENABLE_WAYLAND"),
            Some(&"1".to_string())
        );
    }

    #[test]
    fn overrides_from_ini_parses_bus_policies() {
        let ini = r#"
[Session Bus Policy]
org.freedesktop.secrets=talk
org.gnome.Shell.Screenshot=own

[System Bus Policy]
org.freedesktop.UPower=talk
"#;
        let overrides = FlatpakOverrides::from_ini(ini);

        assert_eq!(
            overrides.session_bus_policy.get("org.freedesktop.secrets"),
            Some(&"talk".to_string())
        );
        assert_eq!(
            overrides
                .session_bus_policy
                .get("org.gnome.Shell.Screenshot"),
            Some(&"own".to_string())
        );
        assert_eq!(
            overrides.system_bus_policy.get("org.freedesktop.UPower"),
            Some(&"talk".to_string())
        );
    }

    #[test]
    fn overrides_to_cli_flags_generates_correct_flags() {
        let mut overrides = FlatpakOverrides::default();
        overrides.filesystems.push("~/Documents:rw".to_string());
        overrides.filesystems.push("!~/Private".to_string()); // negation
        overrides.devices.push("all".to_string());
        overrides.sockets.push("wayland".to_string());
        overrides
            .environment
            .insert("GTK_THEME".to_string(), "Adwaita-dark".to_string());
        overrides
            .session_bus_policy
            .insert("org.freedesktop.secrets".to_string(), "talk".to_string());

        let flags = overrides.to_cli_flags();

        assert!(flags.contains(&"--filesystem=~/Documents:rw".to_string()));
        assert!(flags.contains(&"--nofilesystem=~/Private".to_string()));
        assert!(flags.contains(&"--device=all".to_string()));
        assert!(flags.contains(&"--socket=wayland".to_string()));
        assert!(flags.contains(&"--env=GTK_THEME=Adwaita-dark".to_string()));
        assert!(flags.contains(&"--talk-name=org.freedesktop.secrets".to_string()));
    }

    #[test]
    fn overrides_to_cli_flags_handles_negation() {
        let mut overrides = FlatpakOverrides::default();
        overrides.devices.push("!dri".to_string());
        overrides.shared.push("!network".to_string());
        overrides.sockets.push("!x11".to_string());

        let flags = overrides.to_cli_flags();

        assert!(flags.contains(&"--nodevice=dri".to_string()));
        assert!(flags.contains(&"--unshare=network".to_string()));
        assert!(flags.contains(&"--nosocket=x11".to_string()));
    }

    #[test]
    fn overrides_to_cli_flags_handles_bus_policies() {
        let mut overrides = FlatpakOverrides::default();
        overrides
            .session_bus_policy
            .insert("org.test.talk".to_string(), "talk".to_string());
        overrides
            .session_bus_policy
            .insert("org.test.own".to_string(), "own".to_string());
        overrides
            .session_bus_policy
            .insert("org.test.see".to_string(), "see".to_string());
        overrides
            .system_bus_policy
            .insert("org.system.talk".to_string(), "talk".to_string());

        let flags = overrides.to_cli_flags();

        assert!(flags.contains(&"--talk-name=org.test.talk".to_string()));
        assert!(flags.contains(&"--own-name=org.test.own".to_string()));
        assert!(flags.contains(&"--see-name=org.test.see".to_string()));
        assert!(flags.contains(&"--system-talk-name=org.system.talk".to_string()));
    }

    #[test]
    fn overrides_is_empty_returns_true_for_default() {
        let overrides = FlatpakOverrides::default();
        assert!(overrides.is_empty());
    }

    #[test]
    fn overrides_is_empty_returns_false_when_has_data() {
        let mut overrides = FlatpakOverrides::default();
        overrides.filesystems.push("~/Documents".to_string());
        assert!(!overrides.is_empty());
    }

    #[test]
    fn overrides_roundtrip_ini_to_cli() {
        let ini = r#"
[Context]
filesystems=~/Documents:rw;~/Downloads:ro
devices=all

[Environment]
GTK_THEME=Adwaita-dark

[Session Bus Policy]
org.freedesktop.secrets=talk
"#;
        let overrides = FlatpakOverrides::from_ini(ini);
        let flags = overrides.to_cli_flags();

        // Verify key flags are present
        assert!(flags.contains(&"--filesystem=~/Documents:rw".to_string()));
        assert!(flags.contains(&"--filesystem=~/Downloads:ro".to_string()));
        assert!(flags.contains(&"--device=all".to_string()));
        assert!(flags.contains(&"--env=GTK_THEME=Adwaita-dark".to_string()));
        assert!(flags.contains(&"--talk-name=org.freedesktop.secrets".to_string()));
    }

    #[test]
    fn overrides_from_ini_ignores_comments_and_empty_lines() {
        let ini = r#"
# This is a comment
[Context]
; Another comment
filesystems=~/Documents

"#;
        let overrides = FlatpakOverrides::from_ini(ini);
        assert_eq!(overrides.filesystems, vec!["~/Documents"]);
    }

    #[test]
    fn overrides_override_file_path_user_scope() {
        // Test that user scope uses the right path pattern
        let path = FlatpakOverrides::override_file_path("com.example.App", FlatpakScope::User);
        assert!(
            path.to_string_lossy()
                .contains("flatpak/overrides/com.example.App")
        );
    }

    #[test]
    fn overrides_override_file_path_system_scope() {
        let path = FlatpakOverrides::override_file_path("com.example.App", FlatpakScope::System);
        assert_eq!(
            path,
            PathBuf::from("/var/lib/flatpak/overrides/com.example.App")
        );
    }

    #[test]
    fn overrides_load_from_path_nonexistent() {
        let path = PathBuf::from("/nonexistent/path/overrides");
        let overrides = FlatpakOverrides::load_from_path(&path);
        assert!(overrides.is_none());
    }
}
