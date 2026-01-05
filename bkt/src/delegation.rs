//! Transparent command delegation between host and toolbox.
//!
//! Provides automatic re-execution of commands in the appropriate context:
//! - Commands that need host (flatpak, etc.) run via `flatpak-spawn --host`
//! - Commands that need toolbox (dev) run via `toolbox run`
//!
//! This makes all bkt commands work identically from both environments.

use anyhow::{Context, Result, bail};

use crate::context::{CommandTarget, RuntimeEnvironment};
use crate::output::Output;

/// Delegate the current command to a different context if needed.
///
/// Called early in main() after parsing CLI arguments. Checks if the command
/// target matches the current runtime, and if not, re-executes via the
/// appropriate delegation mechanism.
///
/// Returns Ok(()) if no delegation needed or if already delegated.
/// Never returns if delegation occurs (process is replaced).
pub fn maybe_delegate(
    command_target: CommandTarget,
    runtime: RuntimeEnvironment,
    dry_run: bool,
    no_delegate: bool,
) -> Result<()> {
    // Skip if delegation is disabled
    if no_delegate {
        tracing::debug!("Delegation disabled via --no-delegate");
        return Ok(());
    }

    // Skip if already delegated (prevent infinite recursion)
    if std::env::var("BKT_DELEGATED").is_ok() {
        tracing::debug!("Already delegated (BKT_DELEGATED set)");
        return Ok(());
    }

    match (runtime, command_target) {
        // In toolbox, command wants host → delegate to host
        (RuntimeEnvironment::Toolbox, CommandTarget::Host) => {
            if dry_run {
                Output::dry_run("Would delegate to host: flatpak-spawn --host bkt ...");
                return Ok(());
            }
            tracing::info!("Delegating to host via flatpak-spawn");
            delegate_to_host()?;
        }

        // On host, command wants dev → delegate to toolbox
        (RuntimeEnvironment::Host, CommandTarget::Dev) => {
            if dry_run {
                Output::dry_run("Would delegate to toolbox: toolbox run bkt ...");
                return Ok(());
            }
            tracing::info!("Delegating to toolbox");
            delegate_to_toolbox()?;
        }

        // Generic container, command wants host → error (no delegation path)
        (RuntimeEnvironment::Container, CommandTarget::Host) => {
            bail!(
                "Cannot run host commands from a generic container\n\n\
                 This command requires the host system, but you're in a container\n\
                 without flatpak-spawn access (not a toolbox).\n\n\
                 Options:\n  \
                 • Exit this container and run on the host\n  \
                 • Use a toolbox instead: toolbox create && toolbox enter"
            );
        }

        // All other cases: run locally
        _ => {
            tracing::debug!(
                "No delegation needed: runtime={:?}, target={:?}",
                runtime,
                command_target
            );
        }
    }

    Ok(())
}

/// Delegate the current command to the host via flatpak-spawn.
///
/// Re-executes the entire bkt invocation on the host, passing through all
/// arguments unchanged. Sets BKT_DELEGATED=1 to prevent recursion.
///
/// This function never returns on success (the process is replaced).
fn delegate_to_host() -> Result<()> {
    Output::info("→ Delegating to host via flatpak-spawn...");

    let args: Vec<String> = std::env::args().collect();
    let status = std::process::Command::new("flatpak-spawn")
        .arg("--host")
        .arg("bkt")
        .args(&args[1..]) // Skip argv[0] (the current binary path)
        .env("BKT_DELEGATED", "1") // Prevent recursion
        .status()
        .context("Failed to execute flatpak-spawn")?;

    // Exit with the same code as the delegated command
    std::process::exit(status.code().unwrap_or(1));
}

/// Delegate the current command to the default toolbox.
///
/// Re-executes the entire bkt invocation inside the toolbox, passing through
/// all arguments unchanged. Sets BKT_DELEGATED=1 to prevent recursion.
///
/// This function never returns on success (the process is replaced).
fn delegate_to_toolbox() -> Result<()> {
    Output::info("→ Delegating to toolbox...");

    let args: Vec<String> = std::env::args().collect();
    let status = std::process::Command::new("toolbox")
        .arg("run")
        .arg("bkt")
        .args(&args[1..])
        .env("BKT_DELEGATED", "1")
        .status()
        .context("Failed to execute toolbox run")?;

    std::process::exit(status.code().unwrap_or(1));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_delegation_when_disabled() {
        // Should not panic or delegate
        let result = maybe_delegate(
            CommandTarget::Host,
            RuntimeEnvironment::Toolbox,
            false,
            true, // no_delegate=true
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_no_delegation_when_targets_match() {
        let result = maybe_delegate(CommandTarget::Host, RuntimeEnvironment::Host, false, false);
        assert!(result.is_ok());
    }

    #[test]
    fn test_error_from_generic_container() {
        let result = maybe_delegate(
            CommandTarget::Host,
            RuntimeEnvironment::Container,
            false,
            false,
        );
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Cannot run host commands from a generic container"));
    }
}
