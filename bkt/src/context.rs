//! Execution context detection and management.
//!
//! Determines where commands should execute (host, toolbox, or image-only)
//! and validates that command/context combinations are valid.
//!
//! ## Environment Abstraction
//!
//! This module provides the [`Environment`] trait for abstracting access to
//! environment variables, filesystem paths, and user directories. This enables
//! testing of environment-sensitive code without using `std::env::set_var`,
//! which is unsafe in Rust 2024 edition.
//!
//! See RFC-0019 for design details.

use anyhow::{Context, Result, bail};
use clap::ValueEnum;
use std::collections::{HashMap, HashSet};
use std::env::VarError;
use std::ffi::OsString;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Output;

use crate::command_runner::{CommandOptions, CommandRunner, RealCommandRunner};

// ─────────────────────────────────────────────────────────────────────────────
// Environment Trait (RFC-0019)
// ─────────────────────────────────────────────────────────────────────────────

/// Abstraction over process environment and filesystem for testability.
///
/// This trait enables testing of environment-sensitive code without using
/// `std::env::set_var`, which is unsafe in Rust 2024 edition.
///
/// # Example
///
/// ```ignore
/// // Production code accepts any Environment
/// fn detect_container(env: &dyn Environment) -> bool {
///     env.var("CONTAINER_ID").is_ok() || env.exists(Path::new("/run/.containerenv"))
/// }
///
/// // Tests use MockEnvironment
/// #[test]
/// fn test_detects_container() {
///     let env = MockEnvironment::new()
///         .with_var("CONTAINER_ID", "abc123");
///     assert!(detect_container(&env));
/// }
/// ```
pub trait Environment: Send + Sync {
    // ─────────────────────────────────────────────────────────────────────
    // Environment Variables
    // ─────────────────────────────────────────────────────────────────────

    /// Get an environment variable. Equivalent to `std::env::var`.
    fn var(&self, key: &str) -> Result<String, VarError>;

    /// Get an environment variable as OsString. Equivalent to `std::env::var_os`.
    fn var_os(&self, key: &str) -> Option<OsString>;

    // ─────────────────────────────────────────────────────────────────────
    // User Directories
    // ─────────────────────────────────────────────────────────────────────

    /// Get the user's home directory.
    ///
    /// Prefers `$HOME` environment variable, falls back to `dirs::home_dir()`.
    fn home_dir(&self) -> Option<PathBuf>;

    /// Get the user's config directory. Equivalent to `dirs::config_dir()`.
    fn config_dir(&self) -> Option<PathBuf>;

    /// Get the user's data directory. Equivalent to `dirs::data_dir()`.
    fn data_dir(&self) -> Option<PathBuf>;

    /// Get the user's local data directory. Equivalent to `dirs::data_local_dir()`.
    fn data_local_dir(&self) -> Option<PathBuf>;

    // ─────────────────────────────────────────────────────────────────────
    // Working Directory
    // ─────────────────────────────────────────────────────────────────────

    /// Get the current working directory. Equivalent to `std::env::current_dir()`.
    fn current_dir(&self) -> io::Result<PathBuf>;

    // ─────────────────────────────────────────────────────────────────────
    // Filesystem Queries (Read-Only)
    // ─────────────────────────────────────────────────────────────────────

    /// Check if a path exists. Equivalent to `Path::exists()`.
    fn exists(&self, path: &Path) -> bool;

    /// Check if a path is a file. Equivalent to `Path::is_file()`.
    fn is_file(&self, path: &Path) -> bool;

    /// Check if a path is a directory. Equivalent to `Path::is_dir()`.
    fn is_dir(&self, path: &Path) -> bool;

    /// Read a file to string. Equivalent to `std::fs::read_to_string()`.
    fn read_to_string(&self, path: &Path) -> io::Result<String>;

    // ─────────────────────────────────────────────────────────────────────
    // Convenience Methods (Default Implementations)
    // ─────────────────────────────────────────────────────────────────────

    /// Expand `~/` prefix to the user's home directory.
    ///
    /// If the value starts with `~/`, replaces it with the home directory.
    /// Returns the original path unchanged if it doesn't start with `~/` or if
    /// home directory is not available.
    fn expand_home(&self, value: &str) -> PathBuf {
        if let Some(rest) = value.strip_prefix("~/")
            && let Some(home) = self.home_dir()
        {
            return home.join(rest);
        }
        PathBuf::from(value)
    }

    /// Collapse the user's home directory to `~/` prefix.
    ///
    /// If the path starts with the home directory, replaces that prefix with `~/`.
    /// Returns the original path as a string if it doesn't start with home
    /// or if home is not available.
    fn collapse_home(&self, path: &Path) -> String {
        if let Some(home) = self.home_dir()
            && let Ok(suffix) = path.strip_prefix(&home)
        {
            if suffix.as_os_str().is_empty() {
                return "~".to_string();
            }
            return format!("~/{}", suffix.display());
        }
        path.display().to_string()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// RealEnvironment: Production Implementation
// ─────────────────────────────────────────────────────────────────────────────

/// Production environment that delegates to std and directories crates.
///
/// This is the default implementation used in production code.
#[derive(Debug, Clone, Copy, Default)]
pub struct RealEnvironment;

/// Global instance of the real environment for convenience.
pub static REAL_ENV: RealEnvironment = RealEnvironment;

impl Environment for RealEnvironment {
    fn var(&self, key: &str) -> Result<String, VarError> {
        std::env::var(key)
    }

    fn var_os(&self, key: &str) -> Option<OsString> {
        std::env::var_os(key)
    }

    fn home_dir(&self) -> Option<PathBuf> {
        // Prefer $HOME env var for consistency with shell behavior
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .or_else(|| directories::BaseDirs::new().map(|d| d.home_dir().to_path_buf()))
    }

    fn config_dir(&self) -> Option<PathBuf> {
        directories::BaseDirs::new().map(|d| d.config_dir().to_path_buf())
    }

    fn data_dir(&self) -> Option<PathBuf> {
        directories::BaseDirs::new().map(|d| d.data_dir().to_path_buf())
    }

    fn data_local_dir(&self) -> Option<PathBuf> {
        directories::BaseDirs::new().map(|d| d.data_local_dir().to_path_buf())
    }

    fn current_dir(&self) -> io::Result<PathBuf> {
        std::env::current_dir()
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn is_file(&self, path: &Path) -> bool {
        path.is_file()
    }

    fn is_dir(&self, path: &Path) -> bool {
        path.is_dir()
    }

    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        std::fs::read_to_string(path)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// MockEnvironment: Test Implementation
// ─────────────────────────────────────────────────────────────────────────────

/// Mock environment for testing with configurable state.
///
/// Uses a builder pattern for easy test fixture setup.
///
/// # Example
///
/// ```ignore
/// let env = MockEnvironment::new()
///     .with_var("HOME", "/home/testuser")
///     .with_home("/home/testuser")
///     .with_file("/run/.containerenv", "")
///     .with_cwd("/home/testuser/project");
///
/// assert!(detect_container_with_env(&env));
/// ```
#[derive(Debug, Clone, Default)]
pub struct MockEnvironment {
    vars: HashMap<String, String>,
    home: Option<PathBuf>,
    config_dir: Option<PathBuf>,
    data_dir: Option<PathBuf>,
    data_local_dir: Option<PathBuf>,
    cwd: Option<PathBuf>,
    files: HashMap<PathBuf, String>,
    dirs: HashSet<PathBuf>,
}

impl MockEnvironment {
    /// Create a new empty mock environment.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set an environment variable.
    pub fn with_var(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.vars.insert(key.into(), value.into());
        self
    }

    /// Set the home directory.
    pub fn with_home(mut self, path: impl Into<PathBuf>) -> Self {
        self.home = Some(path.into());
        self
    }

    /// Set the config directory.
    pub fn with_config_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.config_dir = Some(path.into());
        self
    }

    /// Set the data directory.
    pub fn with_data_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.data_dir = Some(path.into());
        self
    }

    /// Set the local data directory.
    pub fn with_data_local_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.data_local_dir = Some(path.into());
        self
    }

    /// Set the current working directory.
    pub fn with_cwd(mut self, path: impl Into<PathBuf>) -> Self {
        self.cwd = Some(path.into());
        self
    }

    /// Add a file with content.
    ///
    /// Parent directories are automatically added.
    pub fn with_file(mut self, path: impl Into<PathBuf>, content: impl Into<String>) -> Self {
        let path = path.into();
        self.files.insert(path.clone(), content.into());
        // Ensure parent directories exist
        let mut current = path.as_path();
        while let Some(parent) = current.parent() {
            if !parent.as_os_str().is_empty() {
                self.dirs.insert(parent.to_path_buf());
            }
            current = parent;
        }
        self
    }

    /// Add an empty directory.
    pub fn with_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.dirs.insert(path.into());
        self
    }
}

impl Environment for MockEnvironment {
    fn var(&self, key: &str) -> Result<String, VarError> {
        self.vars.get(key).cloned().ok_or(VarError::NotPresent)
    }

    fn var_os(&self, key: &str) -> Option<OsString> {
        self.vars.get(key).map(OsString::from)
    }

    fn home_dir(&self) -> Option<PathBuf> {
        self.home.clone()
    }

    fn config_dir(&self) -> Option<PathBuf> {
        self.config_dir.clone()
    }

    fn data_dir(&self) -> Option<PathBuf> {
        self.data_dir.clone()
    }

    fn data_local_dir(&self) -> Option<PathBuf> {
        self.data_local_dir.clone()
    }

    fn current_dir(&self) -> io::Result<PathBuf> {
        self.cwd.clone().ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "cwd not set in MockEnvironment")
        })
    }

    fn exists(&self, path: &Path) -> bool {
        self.files.contains_key(path) || self.dirs.contains(path)
    }

    fn is_file(&self, path: &Path) -> bool {
        self.files.contains_key(path)
    }

    fn is_dir(&self, path: &Path) -> bool {
        self.dirs.contains(path)
    }

    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        self.files.get(path).cloned().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("file not found in MockEnvironment: {}", path.display()),
            )
        })
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Execution Context and Domain Types
// ─────────────────────────────────────────────────────────────────────────────

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

/// Command domain categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandDomain {
    /// Flatpak apps (host-only)
    Flatpak,
    /// Distrobox configuration (host-only)
    Distrobox,
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
    /// System packages (image-level, deferred until image rebuild)
    System,
    /// Homebrew/Linuxbrew packages (host-only)
    Homebrew,
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

// ─────────────────────────────────────────────────────────────────────────────
// Command Target (RFC-0010)
// ─────────────────────────────────────────────────────────────────────────────

/// Where a command naturally wants to execute.
///
/// This is distinct from `ExecutionContext` (user intent) and `RuntimeEnvironment`
/// (where we're actually running). `CommandTarget` represents the command's
/// intrinsic requirement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandTarget {
    /// Must run on the host (flatpak, extension, gsetting, shim, capture, apply, etc.)
    Host,
    /// Must run in the dev toolbox (bkt dev commands)
    Dev,
    /// Can run either place, depends on context (schema, completions, repo, etc.)
    Either,
}

impl CommandDomain {
    /// Check if this domain is valid for the given execution context.
    pub fn valid_for_context(&self, context: ExecutionContext) -> bool {
        match (self, context) {
            // Host-only domains
            (CommandDomain::Flatpak, ExecutionContext::Dev) => false,
            (CommandDomain::Distrobox, ExecutionContext::Dev) => false,
            (CommandDomain::Extension, ExecutionContext::Dev) => false,
            (CommandDomain::Shim, ExecutionContext::Dev) => false,
            (CommandDomain::Homebrew, ExecutionContext::Dev) => false,

            // DNF is valid in both host (rpm-ostree) and dev (dnf) contexts
            (CommandDomain::Dnf, _) => true,

            // System is host-only (modifies image manifest, not toolbox)
            (CommandDomain::System, ExecutionContext::Dev) => false,
            (CommandDomain::System, _) => true,

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
                 Fix: Remove the --context flag or use --context host"
                    .to_string()
            }
            (CommandDomain::Distrobox, ExecutionContext::Dev) => {
                "Distrobox configuration is host-level.\n\n\
                 This command requires host context. If you're seeing this error,\n\
                 you may have explicitly specified --context dev.\n\n\
                 Fix: Remove the --context flag or use --context host"
                    .to_string()
            }
            (CommandDomain::Extension, ExecutionContext::Dev) => {
                "GNOME extensions are host-level.\n\n\
                 This command requires host context. If you're seeing this error,\n\
                 you may have explicitly specified --context dev.\n\n\
                 Fix: Remove the --context flag or use --context host"
                    .to_string()
            }
            (CommandDomain::Shim, ExecutionContext::Dev) => {
                "Shims are host-level (they expose toolbox commands to the host).\n\n\
                 This command requires host context. If you're seeing this error,\n\
                 you may have explicitly specified --context dev.\n\n\
                 Fix: Remove the --context flag or use --context host"
                    .to_string()
            }
            (CommandDomain::System, ExecutionContext::Dev) => {
                "System packages are baked into the bootc image.\n\n\
                 This command modifies manifests/system-packages.json and the Containerfile.\n\
                 It doesn't make sense in a dev/toolbox context.\n\n\
                 For toolbox packages, use: bkt dev install <package>"
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
    detect_environment_with_env(&REAL_ENV)
}

/// Detect the current runtime environment using the provided Environment.
///
/// This is the testable version that accepts an Environment trait object.
pub fn detect_environment_with_env(env: &dyn Environment) -> RuntimeEnvironment {
    // Allow override for testing
    if env.var("BKT_FORCE_HOST").is_ok() {
        return RuntimeEnvironment::Host;
    }

    // Check for toolbox first (most specific)
    if env.exists(Path::new("/run/.toolboxenv")) {
        return RuntimeEnvironment::Toolbox;
    }

    // Check for generic container
    if env.exists(Path::new("/run/.containerenv")) {
        return RuntimeEnvironment::Container;
    }

    // Check environment variable
    if env.var("CONTAINER_ID").is_ok() {
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
    is_in_toolbox_with_env(&REAL_ENV)
}

/// Check if we're in a toolbox using the provided Environment.
///
/// This is the testable version that accepts an Environment trait object.
pub fn is_in_toolbox_with_env(env: &dyn Environment) -> bool {
    // Allow override for testing
    if env.var("BKT_FORCE_HOST").is_ok() {
        return false;
    }

    env.var("TOOLBOX_PATH").is_ok()
        || env.exists(Path::new("/run/.toolboxenv"))
        || env.exists(Path::new("/run/.containerenv"))
}

/// Run a command and return its output.
///
/// This is a simple wrapper around Command::new().output() with error context.
/// Since bkt always runs on the host (via host-only shim), no delegation is needed.
///
/// # Arguments
/// * `program` - The program to run (e.g., "flatpak", "gnome-extensions")
/// * `args` - Arguments to pass to the program
///
/// # Example
/// ```ignore
/// let output = run_command("flatpak", &["list", "--app"])?;
/// ```
pub fn run_command(program: &str, args: &[&str]) -> Result<Output> {
    run_command_with(&RealCommandRunner, program, args)
}

/// Run a command and return its output using a provided command runner.
pub fn run_command_with(
    runner: &dyn CommandRunner,
    program: &str,
    args: &[&str],
) -> Result<Output> {
    runner
        .run_output(program, args, &CommandOptions::default())
        .with_context(|| format!("Failed to run '{}'", program))
}

/// Determine the effective execution context.
///
/// If explicitly specified, use that. Otherwise, auto-detect based on environment.
pub fn resolve_context(explicit: Option<ExecutionContext>) -> ExecutionContext {
    resolve_context_with_env(explicit, &REAL_ENV)
}

/// Determine the effective execution context using the provided Environment.
///
/// This is the testable version that accepts an Environment trait object.
pub fn resolve_context_with_env(
    explicit: Option<ExecutionContext>,
    env: &dyn Environment,
) -> ExecutionContext {
    match explicit {
        Some(ctx) => ctx,
        None => {
            // Auto-detect: if in toolbox, default to Dev; otherwise Host
            match detect_environment_with_env(env) {
                RuntimeEnvironment::Toolbox => ExecutionContext::Dev,
                _ => ExecutionContext::Host,
            }
        }
    }
}

/// Expand `~/` prefix to the user's home directory.
///
/// If the value starts with `~/`, replaces it with the value of `$HOME`.
/// Returns the original string unchanged if it doesn't start with `~/` or if
/// `$HOME` is not set.
///
/// For testable code, use [`Environment::expand_home`] instead.
pub fn expand_home(value: &str) -> String {
    REAL_ENV.expand_home(value).display().to_string()
}

/// Collapse the user's home directory to `~/` prefix.
///
/// If the value starts with `$HOME/`, replaces that prefix with `~/`.
/// Returns the original string unchanged if it doesn't start with the home
/// directory or if `$HOME` is not set.
///
/// For testable code, use [`Environment::collapse_home`] instead.
pub fn collapse_home(value: &str) -> String {
    REAL_ENV.collapse_home(Path::new(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─────────────────────────────────────────────────────────────────────
    // PrMode Tests
    // ─────────────────────────────────────────────────────────────────────

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

    // ─────────────────────────────────────────────────────────────────────
    // CommandDomain Tests
    // ─────────────────────────────────────────────────────────────────────

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

    // ─────────────────────────────────────────────────────────────────────
    // Environment Trait Tests (using MockEnvironment)
    // ─────────────────────────────────────────────────────────────────────

    #[test]
    fn test_mock_environment_var() {
        let env = MockEnvironment::new()
            .with_var("MY_VAR", "my_value")
            .with_var("OTHER_VAR", "other_value");

        assert_eq!(env.var("MY_VAR").unwrap(), "my_value");
        assert_eq!(env.var("OTHER_VAR").unwrap(), "other_value");
        assert!(env.var("MISSING").is_err());
    }

    #[test]
    fn test_mock_environment_home_dir() {
        let env = MockEnvironment::new().with_home("/home/testuser");

        assert_eq!(env.home_dir(), Some(PathBuf::from("/home/testuser")));
    }

    #[test]
    fn test_mock_environment_filesystem() {
        let env = MockEnvironment::new()
            .with_file("/etc/config.toml", "key = 'value'")
            .with_dir("/var/log");

        assert!(env.exists(Path::new("/etc/config.toml")));
        assert!(env.is_file(Path::new("/etc/config.toml")));
        assert!(!env.is_dir(Path::new("/etc/config.toml")));

        assert!(env.exists(Path::new("/var/log")));
        assert!(env.is_dir(Path::new("/var/log")));
        assert!(!env.is_file(Path::new("/var/log")));

        assert!(!env.exists(Path::new("/nonexistent")));
    }

    #[test]
    fn test_mock_environment_read_file() {
        let env = MockEnvironment::new().with_file("/etc/config.toml", "key = 'value'");

        assert_eq!(
            env.read_to_string(Path::new("/etc/config.toml")).unwrap(),
            "key = 'value'"
        );
        assert!(env.read_to_string(Path::new("/nonexistent")).is_err());
    }

    // ─────────────────────────────────────────────────────────────────────
    // Environment Detection Tests (using MockEnvironment)
    // ─────────────────────────────────────────────────────────────────────

    #[test]
    fn test_detect_host_environment() {
        let env = MockEnvironment::new();
        assert_eq!(detect_environment_with_env(&env), RuntimeEnvironment::Host);
    }

    #[test]
    fn test_detect_toolbox_via_file() {
        let env = MockEnvironment::new().with_file("/run/.toolboxenv", "");
        assert_eq!(
            detect_environment_with_env(&env),
            RuntimeEnvironment::Toolbox
        );
    }

    #[test]
    fn test_detect_container_via_file() {
        let env = MockEnvironment::new().with_file("/run/.containerenv", "");
        assert_eq!(
            detect_environment_with_env(&env),
            RuntimeEnvironment::Container
        );
    }

    #[test]
    fn test_detect_container_via_env_var() {
        let env = MockEnvironment::new().with_var("CONTAINER_ID", "abc123");
        assert_eq!(
            detect_environment_with_env(&env),
            RuntimeEnvironment::Container
        );
    }

    #[test]
    fn test_force_host_overrides_container() {
        let env = MockEnvironment::new()
            .with_var("BKT_FORCE_HOST", "1")
            .with_file("/run/.containerenv", "");

        assert_eq!(detect_environment_with_env(&env), RuntimeEnvironment::Host);
    }

    #[test]
    fn test_toolbox_takes_precedence_over_container() {
        // If both toolbox and container markers exist, toolbox wins
        let env = MockEnvironment::new()
            .with_file("/run/.toolboxenv", "")
            .with_file("/run/.containerenv", "");

        assert_eq!(
            detect_environment_with_env(&env),
            RuntimeEnvironment::Toolbox
        );
    }

    // ─────────────────────────────────────────────────────────────────────
    // is_in_toolbox Tests (using MockEnvironment)
    // ─────────────────────────────────────────────────────────────────────

    #[test]
    fn test_is_in_toolbox_via_env_var() {
        let env = MockEnvironment::new().with_var("TOOLBOX_PATH", "/usr/bin/toolbox");
        assert!(is_in_toolbox_with_env(&env));
    }

    #[test]
    fn test_is_in_toolbox_via_toolboxenv() {
        let env = MockEnvironment::new().with_file("/run/.toolboxenv", "");
        assert!(is_in_toolbox_with_env(&env));
    }

    #[test]
    fn test_is_in_toolbox_via_containerenv() {
        let env = MockEnvironment::new().with_file("/run/.containerenv", "");
        assert!(is_in_toolbox_with_env(&env));
    }

    #[test]
    fn test_is_not_in_toolbox_on_host() {
        let env = MockEnvironment::new();
        assert!(!is_in_toolbox_with_env(&env));
    }

    #[test]
    fn test_force_host_disables_toolbox() {
        let env = MockEnvironment::new()
            .with_var("BKT_FORCE_HOST", "1")
            .with_var("TOOLBOX_PATH", "/usr/bin/toolbox");

        assert!(!is_in_toolbox_with_env(&env));
    }

    // ─────────────────────────────────────────────────────────────────────
    // resolve_context Tests (using MockEnvironment)
    // ─────────────────────────────────────────────────────────────────────

    #[test]
    fn test_resolve_context_explicit_wins() {
        let env = MockEnvironment::new().with_file("/run/.toolboxenv", "");

        // Explicit context should override detection
        assert_eq!(
            resolve_context_with_env(Some(ExecutionContext::Host), &env),
            ExecutionContext::Host
        );
        assert_eq!(
            resolve_context_with_env(Some(ExecutionContext::Image), &env),
            ExecutionContext::Image
        );
    }

    #[test]
    fn test_resolve_context_auto_detects_toolbox() {
        let env = MockEnvironment::new().with_file("/run/.toolboxenv", "");
        assert_eq!(resolve_context_with_env(None, &env), ExecutionContext::Dev);
    }

    #[test]
    fn test_resolve_context_auto_detects_host() {
        let env = MockEnvironment::new();
        assert_eq!(resolve_context_with_env(None, &env), ExecutionContext::Host);
    }

    // ─────────────────────────────────────────────────────────────────────
    // expand_home / collapse_home Tests (using MockEnvironment)
    // ─────────────────────────────────────────────────────────────────────

    #[test]
    fn test_env_expand_home_with_tilde() {
        let env = MockEnvironment::new().with_home("/home/testuser");

        assert_eq!(
            env.expand_home("~/bin"),
            PathBuf::from("/home/testuser/bin")
        );
        assert_eq!(
            env.expand_home("~/.config/app"),
            PathBuf::from("/home/testuser/.config/app")
        );
    }

    #[test]
    fn test_env_expand_home_without_tilde() {
        let env = MockEnvironment::new().with_home("/home/testuser");

        assert_eq!(env.expand_home("/usr/bin"), PathBuf::from("/usr/bin"));
        assert_eq!(
            env.expand_home("relative/path"),
            PathBuf::from("relative/path")
        );
        assert_eq!(env.expand_home("~notahome"), PathBuf::from("~notahome"));
    }

    #[test]
    fn test_env_expand_home_no_home_set() {
        let env = MockEnvironment::new(); // No home set

        // Should return path unchanged when home is not available
        assert_eq!(env.expand_home("~/bin"), PathBuf::from("~/bin"));
    }

    #[test]
    fn test_env_collapse_home_with_home_prefix() {
        let env = MockEnvironment::new().with_home("/home/testuser");

        assert_eq!(env.collapse_home(Path::new("/home/testuser/bin")), "~/bin");
        assert_eq!(
            env.collapse_home(Path::new("/home/testuser/.config/app")),
            "~/.config/app"
        );
        assert_eq!(env.collapse_home(Path::new("/home/testuser")), "~");
    }

    #[test]
    fn test_env_collapse_home_without_home_prefix() {
        let env = MockEnvironment::new().with_home("/home/testuser");

        assert_eq!(env.collapse_home(Path::new("/usr/bin")), "/usr/bin");
        assert_eq!(env.collapse_home(Path::new("/var/log")), "/var/log");
    }

    #[test]
    fn test_env_expand_collapse_roundtrip() {
        let env = MockEnvironment::new().with_home("/home/testuser");

        let original = "~/some/path";
        let expanded = env.expand_home(original);
        let collapsed = env.collapse_home(&expanded);
        assert_eq!(collapsed, original);
    }

    // ─────────────────────────────────────────────────────────────────────
    // Legacy Tests (using real environment - kept for backward compatibility)
    // ─────────────────────────────────────────────────────────────────────

    #[test]
    fn test_expand_home_with_tilde_prefix() {
        // Use actual HOME from environment
        if let Ok(home) = std::env::var("HOME") {
            assert_eq!(expand_home("~/bin"), format!("{}/bin", home));
            assert_eq!(
                expand_home("~/.config/app"),
                format!("{}/.config/app", home)
            );
        }
    }

    #[test]
    fn test_expand_home_without_tilde_prefix() {
        assert_eq!(expand_home("/usr/bin"), "/usr/bin");
        assert_eq!(expand_home("relative/path"), "relative/path");
        assert_eq!(expand_home("~notahome"), "~notahome"); // ~ not followed by /
    }

    #[test]
    fn test_collapse_home_with_home_prefix() {
        // Use actual HOME from environment
        if let Ok(home) = std::env::var("HOME") {
            assert_eq!(collapse_home(&format!("{}/bin", home)), "~/bin");
            assert_eq!(
                collapse_home(&format!("{}/.config/app", home)),
                "~/.config/app"
            );
            assert_eq!(collapse_home(&home), "~");
        }
    }

    #[test]
    fn test_collapse_home_without_home_prefix() {
        assert_eq!(collapse_home("/usr/bin"), "/usr/bin");
        // Paths not starting with HOME should be unchanged
        assert_eq!(collapse_home("/var/log"), "/var/log");
    }

    #[test]
    fn test_expand_collapse_roundtrip() {
        // Use actual HOME from environment
        if let Ok(_home) = std::env::var("HOME") {
            let original = "~/some/path";
            let expanded = expand_home(original);
            let collapsed = collapse_home(&expanded);
            assert_eq!(collapsed, original);
        }
    }
}
