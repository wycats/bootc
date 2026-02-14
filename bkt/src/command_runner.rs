//! Abstraction over external command execution for testability.
//!
//! This module provides the [`CommandRunner`] trait, which abstracts all external
//! command invocations (git, flatpak, gsettings, etc.) behind a trait object.
//! This enables in-process testing without spawning subprocesses.
//!
//! # Production Usage
//!
//! [`RealCommandRunner`] delegates to [`std::process::Command`] and is the default
//! implementation stored in [`ExecutionPlan`](crate::pipeline::ExecutionPlan).
//!
//! # Testing Usage
//!
//! [`MockCommandRunner`] records all calls and returns canned responses, enabling
//! fast, deterministic unit tests without external dependencies.

use anyhow::{Context, Result};
use std::path::PathBuf;
use std::process::{Command, ExitStatus, Output};

/// Trait for abstracting external command execution.
///
/// Follows the same pattern as [`PrBackend`](crate::pr::PrBackend): stored as
/// `Arc<dyn CommandRunner>` in [`ExecutionPlan`](crate::pipeline::ExecutionPlan).
///
/// # Call Sites
///
/// There are ~83 call sites across 18 files that use this trait. The two methods
/// cover all usage patterns:
/// - [`run_output`](CommandRunner::run_output): captures stdout + stderr + exit status
/// - [`run_status`](CommandRunner::run_status): inherits stdio, returns only exit status
pub trait CommandRunner: Send + Sync {
    /// Run a command and capture its full output (stdout + stderr + exit status).
    ///
    /// Used by most call sites for output parsing and success checks.
    fn run_output(&self, program: &str, args: &[&str], options: &CommandOptions) -> Result<Output>;

    /// Run a command and return only its exit status (inherits stdio).
    ///
    /// Used by install/uninstall commands, git operations, dnf, brew, etc.
    fn run_status(
        &self,
        program: &str,
        args: &[&str],
        options: &CommandOptions,
    ) -> Result<ExitStatus>;
}

/// Options for command execution.
///
/// Most call sites use `CommandOptions::default()` (no cwd, no env).
/// The `cwd` field is used by ~16 call sites (git/gh operations in pr.rs,
/// local.rs, and build_info.rs).
#[derive(Debug, Default, Clone)]
pub struct CommandOptions {
    /// Working directory for the command.
    pub cwd: Option<PathBuf>,
    /// Additional environment variables.
    pub env: Vec<(String, String)>,
}

impl CommandOptions {
    /// Create options with a working directory.
    pub fn with_cwd(cwd: impl Into<PathBuf>) -> Self {
        Self {
            cwd: Some(cwd.into()),
            ..Default::default()
        }
    }
}

/// Production implementation that delegates to [`std::process::Command`].
pub struct RealCommandRunner;

impl CommandRunner for RealCommandRunner {
    fn run_output(&self, program: &str, args: &[&str], options: &CommandOptions) -> Result<Output> {
        let mut cmd = Command::new(program);
        cmd.args(args);
        if let Some(cwd) = &options.cwd {
            cmd.current_dir(cwd);
        }
        for (k, v) in &options.env {
            cmd.env(k, v);
        }
        cmd.output()
            .with_context(|| format!("Failed to run '{program}'"))
    }

    fn run_status(
        &self,
        program: &str,
        args: &[&str],
        options: &CommandOptions,
    ) -> Result<ExitStatus> {
        let mut cmd = Command::new(program);
        cmd.args(args);
        if let Some(cwd) = &options.cwd {
            cmd.current_dir(cwd);
        }
        for (k, v) in &options.env {
            cmd.env(k, v);
        }
        cmd.status()
            .with_context(|| format!("Failed to run '{program}'"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_options_default() {
        let opts = CommandOptions::default();
        assert!(opts.cwd.is_none());
        assert!(opts.env.is_empty());
    }

    #[test]
    fn test_command_options_with_cwd() {
        let opts = CommandOptions::with_cwd("/tmp");
        assert_eq!(opts.cwd.as_ref().unwrap().to_str().unwrap(), "/tmp");
        assert!(opts.env.is_empty());
    }

    #[test]
    fn test_real_runner_output() {
        let runner = RealCommandRunner;
        let output = runner
            .run_output("echo", &["hello"], &CommandOptions::default())
            .unwrap();
        assert!(output.status.success());
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "hello");
    }

    #[test]
    fn test_real_runner_status() {
        let runner = RealCommandRunner;
        let status = runner
            .run_status("true", &[], &CommandOptions::default())
            .unwrap();
        assert!(status.success());
    }

    #[test]
    fn test_real_runner_with_cwd() {
        let runner = RealCommandRunner;
        let output = runner
            .run_output("pwd", &[], &CommandOptions::with_cwd("/tmp"))
            .unwrap();
        assert!(output.status.success());
        // /tmp might be a symlink, so just check it resolves
        assert!(!String::from_utf8_lossy(&output.stdout).trim().is_empty());
    }
}
