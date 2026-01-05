//! Systemd D-Bus integration.
//!
//! Provides access to systemd's Manager interface for service control.
//!
//! ## D-Bus Interface
//!
//! - **Bus**: System bus (`org.freedesktop.systemd1`)
//! - **Path**: `/org/freedesktop/systemd1`
//! - **Interface**: `org.freedesktop.systemd1.Manager`
//!
//! ## Authorization
//!
//! Polkit handles authorization automatically. Wheel group members get
//! passwordless access to service control operations.

use anyhow::{Context, Result};
use tracing::warn;
use zbus::blocking::Connection;
use zbus::zvariant::OwnedObjectPath;

/// Proxy for the systemd Manager interface.
#[zbus::proxy(
    interface = "org.freedesktop.systemd1.Manager",
    default_service = "org.freedesktop.systemd1",
    default_path = "/org/freedesktop/systemd1"
)]
trait Systemd1Manager {
    /// Start a unit.
    fn start_unit(&self, name: &str, mode: &str) -> zbus::Result<OwnedObjectPath>;

    /// Stop a unit.
    fn stop_unit(&self, name: &str, mode: &str) -> zbus::Result<OwnedObjectPath>;

    /// Restart a unit.
    fn restart_unit(&self, name: &str, mode: &str) -> zbus::Result<OwnedObjectPath>;

    /// Reload a unit's configuration.
    fn reload_unit(&self, name: &str, mode: &str) -> zbus::Result<OwnedObjectPath>;

    /// Enable unit files.
    /// Returns (changes_made, Vec<(type, symlink_name, destination)>)
    fn enable_unit_files(
        &self,
        files: &[&str],
        runtime: bool,
        force: bool,
    ) -> zbus::Result<(bool, Vec<(String, String, String)>)>;

    /// Disable unit files.
    /// Returns Vec<(type, symlink_name, destination)>
    fn disable_unit_files(
        &self,
        files: &[&str],
        runtime: bool,
    ) -> zbus::Result<Vec<(String, String, String)>>;

    /// Reload systemd daemon configuration.
    fn reload(&self) -> zbus::Result<()>;

    /// Get a unit by name.
    fn get_unit(&self, name: &str) -> zbus::Result<OwnedObjectPath>;

    /// Load a unit (creates it if not loaded).
    fn load_unit(&self, name: &str) -> zbus::Result<OwnedObjectPath>;
}

/// Proxy for individual systemd Unit properties.
#[zbus::proxy(
    interface = "org.freedesktop.systemd1.Unit",
    default_service = "org.freedesktop.systemd1"
)]
trait Systemd1Unit {
    /// The current active state (active, inactive, activating, deactivating, failed).
    #[zbus(property)]
    fn active_state(&self) -> zbus::Result<String>;

    /// The sub-state (running, exited, dead, etc.).
    #[zbus(property)]
    fn sub_state(&self) -> zbus::Result<String>;

    /// The load state (loaded, not-found, error, masked).
    #[zbus(property)]
    fn load_state(&self) -> zbus::Result<String>;

    /// The unit description.
    #[zbus(property)]
    fn description(&self) -> zbus::Result<String>;

    /// Whether the unit is enabled (enabled, disabled, static, masked).
    #[zbus(property)]
    fn unit_file_state(&self) -> zbus::Result<String>;
}

/// High-level wrapper for systemd operations.
pub struct SystemdManager {
    connection: Connection,
}

/// Status information for a systemd unit.
#[derive(Debug, Clone)]
pub struct UnitStatus {
    /// Unit name (e.g., "docker.service")
    pub name: String,
    /// Human-readable description
    pub description: String,
    /// Load state: loaded, not-found, error, masked
    pub load_state: String,
    /// Active state: active, inactive, activating, deactivating, failed
    pub active_state: String,
    /// Sub-state: running, exited, dead, etc.
    pub sub_state: String,
    /// Unit file state: enabled, disabled, static, masked
    pub unit_file_state: String,
}

#[allow(dead_code)]
impl UnitStatus {
    /// Check if the unit is currently running.
    pub fn is_active(&self) -> bool {
        self.active_state == "active"
    }

    /// Check if the unit is enabled to start at boot.
    pub fn is_enabled(&self) -> bool {
        matches!(self.unit_file_state.as_str(), "enabled" | "static")
    }

    /// Format as a single-line status string.
    pub fn one_line(&self) -> String {
        format!(
            "{}: {} ({}) - {}",
            self.name, self.active_state, self.sub_state, self.description
        )
    }
}

impl SystemdManager {
    /// Connect to the system bus.
    pub fn new() -> Result<Self> {
        let connection = Connection::system().context("Failed to connect to system D-Bus")?;
        Ok(Self { connection })
    }

    /// Get the Manager proxy.
    fn manager(&self) -> Result<Systemd1ManagerProxyBlocking<'_>> {
        Systemd1ManagerProxyBlocking::new(&self.connection)
            .context("Failed to create systemd Manager proxy")
    }

    /// Get a Unit proxy for a specific unit path.
    fn unit_proxy(&self, path: &OwnedObjectPath) -> Result<Systemd1UnitProxyBlocking<'_>> {
        Systemd1UnitProxyBlocking::builder(&self.connection)
            .path(path.clone())
            .context("Invalid unit path")?
            .build()
            .context("Failed to create Unit proxy")
    }

    /// Normalize a unit name by appending .service if no suffix present.
    fn normalize_unit_name(name: &str) -> String {
        if name.contains('.') {
            name.to_string()
        } else {
            format!("{}.service", name)
        }
    }

    /// Get the status of a unit.
    pub fn status(&self, unit: &str) -> Result<UnitStatus> {
        let name = Self::normalize_unit_name(unit);
        let manager = self.manager()?;

        // Try to load the unit (doesn't start it, just makes it available)
        let path = manager
            .load_unit(&name)
            .context(format!("Failed to load unit: {}", name))?;

        let unit_proxy = self.unit_proxy(&path)?;

        Ok(UnitStatus {
            name: name.clone(),
            description: unit_proxy.description().unwrap_or_else(|e| {
                warn!(
                    "Failed to read systemd unit description for '{}': {}",
                    name, e
                );
                String::new()
            }),
            load_state: unit_proxy.load_state().unwrap_or_else(|e| {
                warn!(
                    "Failed to read systemd unit load_state for '{}': {}",
                    name, e
                );
                "unknown".to_string()
            }),
            active_state: unit_proxy.active_state().unwrap_or_else(|e| {
                warn!(
                    "Failed to read systemd unit active_state for '{}': {}",
                    name, e
                );
                "unknown".to_string()
            }),
            sub_state: unit_proxy.sub_state().unwrap_or_else(|e| {
                warn!(
                    "Failed to read systemd unit sub_state for '{}': {}",
                    name, e
                );
                "unknown".to_string()
            }),
            unit_file_state: unit_proxy.unit_file_state().unwrap_or_else(|e| {
                warn!(
                    "Failed to read systemd unit unit_file_state for '{}': {}",
                    name, e
                );
                "unknown".to_string()
            }),
        })
    }

    /// Start a unit.
    pub fn start(&self, unit: &str) -> Result<()> {
        let name = Self::normalize_unit_name(unit);
        let manager = self.manager()?;
        manager
            .start_unit(&name, "replace")
            .context(format!("Failed to start unit: {}", name))?;
        Ok(())
    }

    /// Stop a unit.
    pub fn stop(&self, unit: &str) -> Result<()> {
        let name = Self::normalize_unit_name(unit);
        let manager = self.manager()?;
        manager
            .stop_unit(&name, "replace")
            .context(format!("Failed to stop unit: {}", name))?;
        Ok(())
    }

    /// Restart a unit.
    pub fn restart(&self, unit: &str) -> Result<()> {
        let name = Self::normalize_unit_name(unit);
        let manager = self.manager()?;
        manager
            .restart_unit(&name, "replace")
            .context(format!("Failed to restart unit: {}", name))?;
        Ok(())
    }

    /// Reload a unit's configuration (not the daemon).
    #[allow(dead_code)]
    pub fn reload_unit(&self, unit: &str) -> Result<()> {
        let name = Self::normalize_unit_name(unit);
        let manager = self.manager()?;
        manager
            .reload_unit(&name, "replace")
            .context(format!("Failed to reload unit: {}", name))?;
        Ok(())
    }

    /// Enable a unit to start at boot.
    pub fn enable(&self, unit: &str) -> Result<bool> {
        let name = Self::normalize_unit_name(unit);
        let manager = self.manager()?;
        let (changes_made, _symlinks) = manager
            .enable_unit_files(&[name.as_str()], false, false)
            .context(format!("Failed to enable unit: {}", name))?;

        // Reload daemon to pick up changes
        manager
            .reload()
            .context("Failed to reload systemd daemon")?;

        Ok(changes_made)
    }

    /// Disable a unit from starting at boot.
    pub fn disable(&self, unit: &str) -> Result<()> {
        let name = Self::normalize_unit_name(unit);
        let manager = self.manager()?;
        manager
            .disable_unit_files(&[name.as_str()], false)
            .context(format!("Failed to disable unit: {}", name))?;

        // Reload daemon to pick up changes
        manager
            .reload()
            .context("Failed to reload systemd daemon")?;

        Ok(())
    }

    /// Reload the systemd daemon (daemon-reload).
    pub fn daemon_reload(&self) -> Result<()> {
        let manager = self.manager()?;
        manager
            .reload()
            .context("Failed to reload systemd daemon")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_unit_name_with_suffix() {
        assert_eq!(
            SystemdManager::normalize_unit_name("docker.service"),
            "docker.service"
        );
        assert_eq!(
            SystemdManager::normalize_unit_name("cups.socket"),
            "cups.socket"
        );
        assert_eq!(
            SystemdManager::normalize_unit_name("multi-user.target"),
            "multi-user.target"
        );
    }

    #[test]
    fn test_normalize_unit_name_without_suffix() {
        assert_eq!(
            SystemdManager::normalize_unit_name("docker"),
            "docker.service"
        );
        assert_eq!(SystemdManager::normalize_unit_name("sshd"), "sshd.service");
    }

    #[test]
    fn test_unit_status_is_active() {
        let status = UnitStatus {
            name: "test.service".to_string(),
            description: "Test".to_string(),
            load_state: "loaded".to_string(),
            active_state: "active".to_string(),
            sub_state: "running".to_string(),
            unit_file_state: "enabled".to_string(),
        };
        assert!(status.is_active());
        assert!(status.is_enabled());
    }

    #[test]
    fn test_unit_status_is_inactive() {
        let status = UnitStatus {
            name: "test.service".to_string(),
            description: "Test".to_string(),
            load_state: "loaded".to_string(),
            active_state: "inactive".to_string(),
            sub_state: "dead".to_string(),
            unit_file_state: "disabled".to_string(),
        };
        assert!(!status.is_active());
        assert!(!status.is_enabled());
    }
}
