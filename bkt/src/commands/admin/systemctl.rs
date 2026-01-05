//! Systemctl subcommand implementation for `bkt admin systemctl`.
//!
//! Provides passwordless access to systemd service control via D-Bus.
//! Uses polkit for authorization - wheel group members get passwordless access.

use anyhow::{Result, bail};
use clap::Subcommand;
use is_terminal::IsTerminal;
use owo_colors::OwoColorize;

use crate::dbus::SystemdManager;
use crate::output::Output;
use crate::pipeline::ExecutionPlan;

/// Systemctl subcommand actions.
#[derive(Debug, Subcommand)]
pub enum SystemctlAction {
    /// Show the status of a unit
    ///
    /// Displays the current state, whether it's enabled, and a description.
    /// This is a read-only operation (no --confirm required).
    Status {
        /// Unit name (e.g., docker, docker.service)
        ///
        /// If no suffix is provided, .service is assumed.
        unit: String,
    },

    /// Start a unit
    ///
    /// Requires --confirm flag for safety.
    Start {
        /// Unit name (e.g., docker, docker.service)
        unit: String,

        /// Confirm this operation
        #[arg(long)]
        confirm: bool,
    },

    /// Stop a unit
    ///
    /// Requires --confirm flag for safety.
    Stop {
        /// Unit name (e.g., docker, docker.service)
        unit: String,

        /// Confirm this operation
        #[arg(long)]
        confirm: bool,
    },

    /// Restart a unit
    ///
    /// Requires --confirm flag for safety.
    Restart {
        /// Unit name (e.g., docker, docker.service)
        unit: String,

        /// Confirm this operation
        #[arg(long)]
        confirm: bool,
    },

    /// Enable a unit to start at boot
    ///
    /// Requires --confirm flag for safety.
    Enable {
        /// Unit name (e.g., docker, docker.service)
        unit: String,

        /// Confirm this operation
        #[arg(long)]
        confirm: bool,
    },

    /// Disable a unit from starting at boot
    ///
    /// Requires --confirm flag for safety.
    Disable {
        /// Unit name (e.g., docker, docker.service)
        unit: String,

        /// Confirm this operation
        #[arg(long)]
        confirm: bool,
    },

    /// Reload systemd daemon configuration
    ///
    /// Equivalent to `systemctl daemon-reload`.
    /// Requires --confirm flag for safety.
    DaemonReload {
        /// Confirm this operation
        #[arg(long)]
        confirm: bool,
    },
}

/// Execute a systemctl subcommand.
pub fn run(action: SystemctlAction, plan: &ExecutionPlan) -> Result<()> {
    match action {
        SystemctlAction::Status { unit } => status(&unit, plan),
        SystemctlAction::Start { unit, confirm } => start(&unit, confirm, plan),
        SystemctlAction::Stop { unit, confirm } => stop(&unit, confirm, plan),
        SystemctlAction::Restart { unit, confirm } => restart(&unit, confirm, plan),
        SystemctlAction::Enable { unit, confirm } => enable(&unit, confirm, plan),
        SystemctlAction::Disable { unit, confirm } => disable(&unit, confirm, plan),
        SystemctlAction::DaemonReload { confirm } => daemon_reload(confirm, plan),
    }
}

/// Show status of a unit.
fn status(unit: &str, plan: &ExecutionPlan) -> Result<()> {
    if plan.dry_run {
        Output::dry_run(format!("Would show status of: {}", unit));
        return Ok(());
    }

    let manager = SystemdManager::new()?;
    let status = manager.status(unit)?;

    // Format output similar to systemctl status
    let use_color = std::io::stdout().is_terminal();

    let active_color = if use_color {
        match status.active_state.as_str() {
            "active" => status.active_state.green().to_string(),
            "inactive" => status.active_state.dimmed().to_string(),
            "failed" => status.active_state.red().to_string(),
            _ => status.active_state.yellow().to_string(),
        }
    } else {
        status.active_state.clone()
    };

    let enabled_color = if use_color {
        match status.unit_file_state.as_str() {
            "enabled" | "static" => status.unit_file_state.green().to_string(),
            "disabled" => status.unit_file_state.dimmed().to_string(),
            "masked" => status.unit_file_state.red().to_string(),
            _ => status.unit_file_state.yellow().to_string(),
        }
    } else {
        status.unit_file_state.clone()
    };

    println!("● {}", status.name.bold());
    println!("     Loaded: {}", status.load_state);
    println!("     Active: {} ({})", active_color, status.sub_state);
    println!("    Enabled: {}", enabled_color);
    if !status.description.is_empty() {
        println!("       Desc: {}", status.description);
    }

    Ok(())
}

/// Start a unit.
fn start(unit: &str, confirm: bool, plan: &ExecutionPlan) -> Result<()> {
    require_confirmation(confirm, "start", Some(unit))?;

    if plan.dry_run {
        Output::dry_run(format!("Would start: {}", unit));
        return Ok(());
    }

    Output::info(format!("Starting {}...", unit.cyan()));
    let manager = SystemdManager::new()?;
    manager.start(unit)?;
    Output::success(format!("Started {}", unit));

    Ok(())
}

/// Stop a unit.
fn stop(unit: &str, confirm: bool, plan: &ExecutionPlan) -> Result<()> {
    require_confirmation(confirm, "stop", Some(unit))?;

    if plan.dry_run {
        Output::dry_run(format!("Would stop: {}", unit));
        return Ok(());
    }

    Output::info(format!("Stopping {}...", unit.cyan()));
    let manager = SystemdManager::new()?;
    manager.stop(unit)?;
    Output::success(format!("Stopped {}", unit));

    Ok(())
}

/// Restart a unit.
fn restart(unit: &str, confirm: bool, plan: &ExecutionPlan) -> Result<()> {
    require_confirmation(confirm, "restart", Some(unit))?;

    if plan.dry_run {
        Output::dry_run(format!("Would restart: {}", unit));
        return Ok(());
    }

    Output::info(format!("Restarting {}...", unit.cyan()));
    let manager = SystemdManager::new()?;
    manager.restart(unit)?;
    Output::success(format!("Restarted {}", unit));

    Ok(())
}

/// Enable a unit to start at boot.
fn enable(unit: &str, confirm: bool, plan: &ExecutionPlan) -> Result<()> {
    require_confirmation(confirm, "enable", Some(unit))?;

    if plan.dry_run {
        Output::dry_run(format!("Would enable: {}", unit));
        return Ok(());
    }

    Output::info(format!("Enabling {}...", unit.cyan()));
    let manager = SystemdManager::new()?;
    let changes_made = manager.enable(unit)?;

    if changes_made {
        Output::success(format!("Enabled {}", unit));
    } else {
        Output::info(format!("{} was already enabled", unit));
    }

    Ok(())
}

/// Disable a unit from starting at boot.
fn disable(unit: &str, confirm: bool, plan: &ExecutionPlan) -> Result<()> {
    require_confirmation(confirm, "disable", Some(unit))?;

    if plan.dry_run {
        Output::dry_run(format!("Would disable: {}", unit));
        return Ok(());
    }

    Output::info(format!("Disabling {}...", unit.cyan()));
    let manager = SystemdManager::new()?;
    manager.disable(unit)?;
    Output::success(format!("Disabled {}", unit));

    Ok(())
}

/// Reload systemd daemon configuration.
fn daemon_reload(confirm: bool, plan: &ExecutionPlan) -> Result<()> {
    require_confirmation(confirm, "daemon-reload", None)?;

    if plan.dry_run {
        Output::dry_run("Would reload systemd daemon configuration");
        return Ok(());
    }

    Output::info("Reloading systemd daemon configuration...");
    let manager = SystemdManager::new()?;
    manager.daemon_reload()?;
    Output::success("Reloaded systemd daemon");

    Ok(())
}

/// Require --confirm flag for mutating operations.
fn require_confirmation(confirm: bool, operation: &str, unit: Option<&str>) -> Result<()> {
    if confirm {
        return Ok(());
    }

    // Interactive mode: prompt for confirmation if we have a TTY
    if std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
        let message = match unit {
            Some(unit) => format!("This will {} {}. Continue?", operation, unit.cyan()),
            None => format!("This will {}. Continue?", operation.cyan()),
        };
        if prompt_continue(&message)? {
            return Ok(());
        }
        bail!("Operation cancelled");
    }

    // Non-interactive: require --confirm
    match unit {
        Some(unit) => bail!(
            "This operation requires confirmation.\n\n\
             Add {} to proceed:\n  \
             bkt admin systemctl {} {} --confirm",
            "--confirm".cyan(),
            operation,
            unit
        ),
        None => bail!(
            "This operation requires confirmation.\n\n\
             Add {} to proceed:\n  \
             bkt admin systemctl {} --confirm",
            "--confirm".cyan(),
            operation
        ),
    }
}

/// Prompt user for confirmation.
fn prompt_continue(message: &str) -> Result<bool> {
    use std::io::{Write, stdin, stdout};

    print!("{} [y/N] ", message);
    stdout().flush()?;

    let mut input = String::new();
    stdin().read_line(&mut input)?;
    let input = input.trim().to_lowercase();

    Ok(input == "y" || input == "yes")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_systemctl_action_variants() {
        // Just verify the enum compiles and has expected variants
        let _ = SystemctlAction::Status {
            unit: "docker".to_string(),
        };
        let _ = SystemctlAction::Start {
            unit: "docker".to_string(),
            confirm: true,
        };
        let _ = SystemctlAction::Stop {
            unit: "docker".to_string(),
            confirm: false,
        };
        let _ = SystemctlAction::Restart {
            unit: "docker".to_string(),
            confirm: true,
        };
        let _ = SystemctlAction::Enable {
            unit: "docker".to_string(),
            confirm: true,
        };
        let _ = SystemctlAction::Disable {
            unit: "docker".to_string(),
            confirm: true,
        };
        let _ = SystemctlAction::DaemonReload { confirm: true };
    }
}
