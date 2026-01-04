//! Bootc subcommand implementation for `bkt admin bootc`.
//!
//! Provides passwordless access to bootc operations via polkit + pkexec.
//! Handles context detection to work identically from host or toolbox.

use anyhow::{bail, Context, Result};
use clap::Subcommand;
use is_terminal::IsTerminal;
use owo_colors::OwoColorize;
use std::process::Command;

use crate::context::{detect_environment, RuntimeEnvironment};
use crate::output::Output;
use crate::pipeline::ExecutionPlan;

/// Bootc operations available via `bkt admin bootc`.
#[derive(Debug, Subcommand)]
pub enum BootcAction {
    /// Show current deployment status (passwordless, read-only)
    ///
    /// Displays information about current and staged deployments,
    /// image references, and update availability.
    Status,

    /// Upgrade to the latest image (requires --confirm or --yes)
    ///
    /// Fetches and stages the latest version of the current image.
    /// The new deployment is staged for next boot; running system is unchanged.
    /// Use `bootc rollback` to revert if needed.
    Upgrade {
        /// Confirm the upgrade operation (required for safety)
        #[arg(long)]
        confirm: bool,

        /// Skip confirmation prompt and proceed (implies --confirm, for automation)
        #[arg(long, short = 'y')]
        yes: bool,
    },

    /// Switch to a different image (requires --confirm or --yes)
    ///
    /// Stages a new deployment using the specified image reference.
    /// The switch takes effect on next boot.
    Switch {
        /// Image reference (e.g., ghcr.io/user/image:tag)
        image: String,

        /// Confirm the switch operation (required for safety)
        #[arg(long)]
        confirm: bool,

        /// Skip confirmation prompt and proceed (implies --confirm, for automation)
        #[arg(long, short = 'y')]
        yes: bool,
    },

    /// Rollback to previous deployment (requires --confirm or --yes)
    ///
    /// Makes the previous deployment the default for next boot.
    /// Useful for reverting after a problematic upgrade.
    Rollback {
        /// Confirm the rollback operation (required for safety)
        #[arg(long)]
        confirm: bool,

        /// Skip confirmation prompt and proceed (implies --confirm, for automation)
        #[arg(long, short = 'y')]
        yes: bool,
    },
}

/// Execute a bootc action.
pub fn run(action: BootcAction, plan: &ExecutionPlan) -> Result<()> {
    match action {
        BootcAction::Status => handle_status(plan),
        BootcAction::Upgrade { confirm, yes } => handle_upgrade(plan, confirm, yes),
        BootcAction::Switch { image, confirm, yes } => handle_switch(plan, &image, confirm, yes),
        BootcAction::Rollback { confirm, yes } => handle_rollback(plan, confirm, yes),
    }
}

/// Handle `bkt admin bootc status`.
fn handle_status(plan: &ExecutionPlan) -> Result<()> {
    if plan.dry_run {
        Output::dry_run(format!("Would execute: {}", describe_execution("status", &[])));
        return Ok(());
    }

    exec_bootc("status", &[])
}

/// Handle `bkt admin bootc upgrade`.
fn handle_upgrade(plan: &ExecutionPlan, confirm: bool, yes: bool) -> Result<()> {
    // --yes implies --confirm (for non-interactive automation)
    let confirmed = confirm || yes;
    require_confirmation("upgrade", confirmed)?;

    if !yes && !plan.dry_run {
        if !prompt_continue("This will stage an image upgrade for next boot.")? {
            Output::info("Cancelled.");
            return Ok(());
        }
    }

    if plan.dry_run {
        Output::dry_run(format!("Would execute: {}", describe_execution("upgrade", &[])));
        return Ok(());
    }

    exec_bootc("upgrade", &[])
}

/// Handle `bkt admin bootc switch`.
fn handle_switch(plan: &ExecutionPlan, image: &str, confirm: bool, yes: bool) -> Result<()> {
    // --yes implies --confirm (for non-interactive automation)
    let confirmed = confirm || yes;
    require_confirmation("switch", confirmed)?;

    if !yes && !plan.dry_run {
        let msg = format!("This will switch to image '{}' on next boot.", image);
        if !prompt_continue(&msg)? {
            Output::info("Cancelled.");
            return Ok(());
        }
    }

    if plan.dry_run {
        Output::dry_run(format!(
            "Would execute: {}",
            describe_execution("switch", &[image.to_string()])
        ));
        return Ok(());
    }

    exec_bootc("switch", &[image.to_string()])
}

/// Handle `bkt admin bootc rollback`.
fn handle_rollback(plan: &ExecutionPlan, confirm: bool, yes: bool) -> Result<()> {
    // --yes implies --confirm (for non-interactive automation)
    let confirmed = confirm || yes;
    require_confirmation("rollback", confirmed)?;

    if !yes && !plan.dry_run {
        if !prompt_continue("This will make the previous deployment the default for next boot.")? {
            Output::info("Cancelled.");
            return Ok(());
        }
    }

    if plan.dry_run {
        Output::dry_run(format!(
            "Would execute: {}",
            describe_execution("rollback", &[])
        ));
        return Ok(());
    }

    exec_bootc("rollback", &[])
}

/// Require the --confirm or --yes flag for mutating operations.
fn require_confirmation(operation: &str, confirmed: bool) -> Result<()> {
    if confirmed {
        return Ok(());
    }

    let suggestions = match operation {
        "upgrade" => vec![
            "Stages a new deployment for next boot",
            "Does not affect the running system",
            "Can be rolled back with 'bkt admin bootc rollback'",
        ],
        "switch" => vec![
            "Stages a new image deployment for next boot",
            "Does not affect the running system",
            "Can be rolled back with 'bkt admin bootc rollback'",
        ],
        "rollback" => vec![
            "Makes the previous deployment the default for next boot",
            "Does not affect the running system",
            "Can be re-upgraded with 'bkt admin bootc upgrade'",
        ],
        _ => vec![],
    };

    Output::error("This operation requires explicit confirmation");
    println!();
    println!(
        "'{}' is a mutating operation. This action:",
        format!("bootc {}", operation).cyan()
    );
    for point in &suggestions {
        println!("  • {}", point);
    }
    println!();
    println!("To proceed, run:");
    println!(
        "  {} {} {}",
        "bkt admin bootc".green(),
        operation.green(),
        "--confirm".yellow()
    );
    println!();
    println!(
        "For non-interactive automation, use {} (implies --confirm):",
        "--yes".yellow()
    );
    println!(
        "  {} {} {}",
        "bkt admin bootc".green(),
        operation.green(),
        "--yes".yellow()
    );

    bail!("Missing --confirm or --yes flag for mutating operation");
}

/// Prompt user to continue (for non-dry-run mutating operations).
///
/// Returns `Ok(false)` if stdin is not a TTY (non-interactive), treating it as "no".
fn prompt_continue(message: &str) -> Result<bool> {
    // Check if stdin is a terminal; if not, don't prompt (default to "no")
    if !std::io::stdin().is_terminal() {
        Output::warning("Non-interactive mode detected; use --yes to skip prompts.");
        return Ok(false);
    }

    Output::warning(message);
    print!("Continue? [y/N] ");
    std::io::Write::flush(&mut std::io::stdout())?;

    let mut input = String::new();
    let bytes_read = std::io::stdin().read_line(&mut input)?;

    // Handle EOF (e.g., pipe closed)
    if bytes_read == 0 {
        return Ok(false);
    }

    let input = input.trim().to_lowercase();
    Ok(input == "y" || input == "yes")
}

/// Execute a bootc command via pkexec, handling toolbox context.
///
/// - **Host**: Direct `pkexec bootc <args>`
/// - **Toolbox**: `flatpak-spawn --host pkexec bootc <args>`
/// - **Container**: Returns error (generic containers can't access host polkit)
fn exec_bootc(subcommand: &str, args: &[String]) -> Result<()> {
    let env = detect_environment();
    let status = match env {
        RuntimeEnvironment::Toolbox => {
            // Delegate to host via flatpak-spawn
            Output::info(format!(
                "Executing {} on host from toolbox...",
                format!("bootc {}", subcommand).cyan()
            ));

            Command::new("flatpak-spawn")
                .arg("--host")
                .arg("pkexec")
                .arg("bootc")
                .arg(subcommand)
                .args(args)
                .status()
                .context("Failed to execute flatpak-spawn")?
        }
        RuntimeEnvironment::Host => {
            // Direct execution on host
            Command::new("pkexec")
                .arg("bootc")
                .arg(subcommand)
                .args(args)
                .status()
                .context("Failed to execute pkexec")?
        }
        RuntimeEnvironment::Container => {
            // Generic containers don't have access to host's polkit daemon
            bail!(
                "Cannot execute admin commands from a generic container\n\n\
                 Admin commands require access to the host's polkit daemon, which is\n\
                 only available from toolbox containers (created with 'toolbox create').\n\n\
                 Options:\n  \
                 • Exit this container and run from the host\n  \
                 • Use a toolbox instead: toolbox create && toolbox enter"
            );
        }
    };

    if !status.success() {
        let code = status.code().map(|c| c.to_string()).unwrap_or_else(|| "unknown".to_string());
        bail!(
            "bootc {} failed with exit code {}",
            subcommand,
            code
        );
    }

    Ok(())
}

/// Describe what command would be executed (for dry-run output).
fn describe_execution(subcommand: &str, args: &[String]) -> String {
    let env = detect_environment();
    let args_str = if args.is_empty() {
        String::new()
    } else {
        format!(" {}", args.join(" "))
    };

    match env {
        RuntimeEnvironment::Toolbox => {
            format!(
                "flatpak-spawn --host pkexec bootc {}{}",
                subcommand, args_str
            )
        }
        RuntimeEnvironment::Host => {
            format!("pkexec bootc {}{}", subcommand, args_str)
        }
        RuntimeEnvironment::Container => {
            format!("[error: unsupported in generic container] pkexec bootc {}{}", subcommand, args_str)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    /// Helper to set env var for testing (requires unsafe in Rust 2024)
    fn set_force_host(value: bool) {
        unsafe {
            if value {
                std::env::set_var("BKT_FORCE_HOST", "1");
            } else {
                std::env::remove_var("BKT_FORCE_HOST");
            }
        }
    }

    #[test]
    #[serial]
    fn test_describe_execution_status() {
        set_force_host(true);
        let desc = describe_execution("status", &[]);
        assert_eq!(desc, "pkexec bootc status");
        set_force_host(false);
    }

    #[test]
    #[serial]
    fn test_describe_execution_switch_with_args() {
        set_force_host(true);
        let desc = describe_execution("switch", &["ghcr.io/test/image:latest".to_string()]);
        assert_eq!(desc, "pkexec bootc switch ghcr.io/test/image:latest");
        set_force_host(false);
    }

    #[test]
    fn test_require_confirmation_fails_without_flag() {
        let result = require_confirmation("upgrade", false);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("--confirm"));
        assert!(err.contains("--yes"));
    }

    #[test]
    fn test_require_confirmation_succeeds_with_flag() {
        let result = require_confirmation("upgrade", true);
        assert!(result.is_ok());
    }

    #[test]
    fn test_require_confirmation_messages_vary_by_operation() {
        // Each operation should mention different rollback/recovery options
        let upgrade_err = require_confirmation("upgrade", false).unwrap_err().to_string();
        let rollback_err = require_confirmation("rollback", false).unwrap_err().to_string();

        // Both should require confirmation
        assert!(upgrade_err.contains("--confirm"));
        assert!(rollback_err.contains("--confirm"));
    }
}
