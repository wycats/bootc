//! Execution context detection and management.
//!
//! Determines where commands should execute (host, toolbox, or image-only)
//! and validates that command/context combinations are valid.

use anyhow::{Context, Result, bail};
use clap::ValueEnum;
use std::fmt;
use std::path::Path;
use std::process::{Command, Output};
use tracing::warn;

/// Execution context for bkt commands.
///
/// Determines where the command will have its immediate effect:
/// - `Host`: Execute on the immutable host system (default)
/// - `Dev`: Execute in the development toolbox
/// - `Image`: Only update manifests for next image build (no immediate effect)
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub enum ExecutionContext {
    /// Execute on the host system (rpm-ostree, flatpak, gsettings)
    #[default]
    Host,
    /// Execute in the development toolbox (dnf, cargo, npm)
    Dev,
    /// Only update manifests, no local execution (for remote preparation)
    Image,
}

impl fmt::Display for ExecutionContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExecutionContext::Host => write!(f, "host"),
            ExecutionContext::Dev => write!(f, "dev"),
            ExecutionContext::Image => write!(f, "image"),
        }
    }
}

/// PR behavior mode for commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PrMode {
    /// Default: execute locally AND create PR
    #[default]
    Both,
    /// Only create PR, skip local execution (--pr-only)
    PrOnly,
    /// Only execute locally, skip PR (--local)
    LocalOnly,
}

impl PrMode {
    /// Should we execute the local action?
    pub fn should_execute_locally(&self) -> bool {
        matches!(self, PrMode::Both | PrMode::LocalOnly)
    }

    /// Should we create a PR?
    pub fn should_create_pr(&self) -> bool {
        matches!(self, PrMode::Both | PrMode::PrOnly)
    }
}

/// Where a command naturally wants to execute.
///
/// This determines whether a command should be delegated to a different
/// runtime environment (host vs. toolbox) for transparent execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandTarget {
    /// Must run on the host (flatpak, extension, gsetting, shim, capture, apply)
    Host,
    /// Must run in the dev toolbox (bkt dev commands)
    Dev,
    /// Can run either place, meaning depends on context
    Either,
}

/// Command domain categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandDomain {
    /// Flatpak apps (host-only)
    Flatpak,
    /// GNOME extensions (host-only)
    Extension,
    /// GSettings (host-only, but can be applied in toolbox too)
    Gsetting,
    /// Host shims for toolbox access
    Shim,
    /// Skel/dotfiles management
    Skel,
    /// DNF/RPM packages (context-dependent: host uses rpm-ostree, dev uses dnf)
    Dnf,
    /// Profile/status commands (read-only, any context)
    Profile,
    /// Repository info (read-only)
    Repo,
    /// Schema generation (read-only)
    Schema,
    /// Doctor/health checks (read-only)
    Doctor,
    /// Status overview (read-only)
    Status,
    /// Completions (read-only)
    Completions,
}

impl CommandDomain {
    /// Check if this domain is valid for the given execution context.
    pub fn valid_for_context(&self, context: ExecutionContext) -> bool {
        match (self, context) {
            // Host-only domains
            (CommandDomain::Flatpak, ExecutionContext::Dev) => false,
            (CommandDomain::Extension, ExecutionContext::Dev) => false,
            (CommandDomain::Shim, ExecutionContext::Dev) => false,

            // DNF is valid in both host (rpm-ostree) and dev (dnf) contexts
            (CommandDomain::Dnf, _) => true,

            // Read-only domains work everywhere
            (CommandDomain::Profile, _) => true,
            (CommandDomain::Repo, _) => true,
            (CommandDomain::Schema, _) => true,
            (CommandDomain::Doctor, _) => true,
            (CommandDomain::Status, _) => true,
            (CommandDomain::Completions, _) => true,

            // Everything else is valid
            _ => true,
        }
    }

    /// Get the error message for an invalid context/domain combination.
    pub fn context_error_message(&self, context: ExecutionContext) -> String {
        match (self, context) {
            (CommandDomain::Flatpak, ExecutionContext::Dev) => {
                "Flatpaks are host-level applications.\n\n\
                 This command requires host context. If you're seeing this error,\n\
                 you may have explicitly specified --context dev.\n\n\
                 Fix: Remove the --context flag (delegation is automatic)"
                    .to_string()
            }
            (CommandDomain::Extension, ExecutionContext::Dev) => {
                "GNOME extensions are host-level.\n\n\
                 This command requires host context. If you're seeing this error,\n\
                 you may have explicitly specified --context dev.\n\n\
                 Fix: Remove the --context flag (delegation is automatic)"
                    .to_string()
            }
            (CommandDomain::Shim, ExecutionContext::Dev) => {
                "Shims are host-level (they expose host commands to the toolbox).\n\n\
                 This command requires host context. If you're seeing this error,\n\
                 you may have explicitly specified --context dev.\n\n\
                 Fix: Remove the --context flag (delegation is automatic)"
                    .to_string()
            }
            _ => format!("{:?} is not valid in {} context", self, context),
        }
    }
}

/// Validate that a domain is valid for the given context.
pub fn validate_context_for_domain(domain: CommandDomain, context: ExecutionContext) -> Result<()> {
    if domain.valid_for_context(context) {
        Ok(())
    } else {
        bail!(
            "Invalid context for domain\n\n{}",
            domain.context_error_message(context)
        )
    }
}

/// Runtime environment detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeEnvironment {
    /// Running directly on the host system
    Host,
    /// Running inside a toolbox container
    Toolbox,
    /// Running inside a generic container (not toolbox)
    Container,
}

/// Detect the current runtime environment.
///
/// Checks for toolbox/container indicators:
/// - /run/.toolboxenv (toolbox-specific)
/// - /run/.containerenv (generic podman container)
/// - CONTAINER_ID environment variable
///
/// Note: Can be overridden with BKT_FORCE_HOST=1 for testing.
pub fn detect_environment() -> RuntimeEnvironment {
    // Allow override for testing
    if std::env::var("BKT_FORCE_HOST").is_ok() {
        return RuntimeEnvironment::Host;
    }

    // Check for toolbox first (most specific)
    if Path::new("/run/.toolboxenv").exists() {
        return RuntimeEnvironment::Toolbox;
    }

    // Check for generic container
    if Path::new("/run/.containerenv").exists() {
        return RuntimeEnvironment::Container;
    }

    // Check environment variable
    if std::env::var("CONTAINER_ID").is_ok() {
        return RuntimeEnvironment::Container;
    }

    RuntimeEnvironment::Host
}

/// Check if we're running inside a toolbox or container environment.
///
/// This is the canonical function for toolbox detection. Use this instead of
/// duplicating the detection logic in individual modules.
///
/// Checks for:
/// - TOOLBOX_PATH environment variable (set by toolbox)
/// - /run/.toolboxenv (toolbox-specific marker)
/// - /run/.containerenv (generic podman container marker)
pub fn is_in_toolbox() -> bool {
    // Allow override for testing
    if std::env::var("BKT_FORCE_HOST").is_ok() {
        return false;
    }

    std::env::var("TOOLBOX_PATH").is_ok()
        || Path::new("/run/.toolboxenv").exists()
        || Path::new("/run/.containerenv").exists()
}

/// Run a command on the host, delegating via flatpak-spawn if in a toolbox/container.
///
/// Returns a Result with the command output. If the command fails to execute
/// (e.g., flatpak-spawn not available), logs a warning and returns the error.
///
/// # Arguments
/// * `program` - The program to run (e.g., "flatpak", "gnome-extensions")
/// * `args` - Arguments to pass to the program
///
/// # Example
/// ```ignore
/// let output = run_host_command("flatpak", &["list", "--app"])?;
/// ```
pub fn run_host_command(program: &str, args: &[&str]) -> Result<Output> {
    if is_in_toolbox() {
        let mut cmd = Command::new("flatpak-spawn");
        cmd.arg("--host").arg(program).args(args);
        let output = cmd.output().with_context(|| {
            format!(
                "Failed to run '{}' via flatpak-spawn --host. \
                 This is required when running inside a toolbox/container.",
                program
            )
        });
        if output.is_err() {
            warn!(
                "flatpak-spawn failed for command: {} {:?}. Is flatpak-spawn available?",
                program, args
            );
        }
        output
    } else {
        Command::new(program)
            .args(args)
            .output()
            .with_context(|| format!("Failed to run '{}'", program))
    }
}

/// Determine the effective execution context.
///
/// If explicitly specified, use that. Otherwise, auto-detect based on environment.
pub fn resolve_context(explicit: Option<ExecutionContext>) -> ExecutionContext {
    match explicit {
        Some(ctx) => ctx,
        None => {
            // Auto-detect: if in toolbox, default to Dev; otherwise Host
            match detect_environment() {
                RuntimeEnvironment::Toolbox => ExecutionContext::Dev,
                _ => ExecutionContext::Host,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pr_mode_should_execute_locally() {
        assert!(PrMode::Both.should_execute_locally());
        assert!(!PrMode::PrOnly.should_execute_locally());
        assert!(PrMode::LocalOnly.should_execute_locally());
    }

    #[test]
    fn test_pr_mode_should_create_pr() {
        assert!(PrMode::Both.should_create_pr());
        assert!(PrMode::PrOnly.should_create_pr());
        assert!(!PrMode::LocalOnly.should_create_pr());
    }

    #[test]
    fn test_flatpak_invalid_in_dev_context() {
        assert!(!CommandDomain::Flatpak.valid_for_context(ExecutionContext::Dev));
        assert!(CommandDomain::Flatpak.valid_for_context(ExecutionContext::Host));
        assert!(CommandDomain::Flatpak.valid_for_context(ExecutionContext::Image));
    }

    #[test]
    fn test_dnf_valid_in_all_contexts() {
        assert!(CommandDomain::Dnf.valid_for_context(ExecutionContext::Host));
        assert!(CommandDomain::Dnf.valid_for_context(ExecutionContext::Dev));
        assert!(CommandDomain::Dnf.valid_for_context(ExecutionContext::Image));
    }
}
