//! bkt - Bucket: manage your bootc manifests
//!
//! A CLI tool for managing system manifests including Flatpaks, GNOME extensions,
//! GSettings, host shims, skel files, and system profiles.
//!
//! # Command Punning
//!
//! `bkt` implements "command punning": commands that execute immediately AND
//! propagate changes to the distribution via Git PRs. This is the core philosophy
//! of Phase 2.
//!
//! ## Execution Contexts
//!
//! - **Host** (default): Execute on the immutable host system
//! - **Dev** (`bkt dev ...`): Execute in the development toolbox  
//! - **Image** (`--pr-only`): Only update manifests, no local execution
//!
//! ## PR Modes
//!
//! - Default: Execute locally AND create PR
//! - `--local`: Execute locally only, skip PR
//! - `--pr-only`: Create PR only, skip local execution

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

mod commands;
pub mod containerfile;
pub mod context;
mod dbus;
pub mod effects;
mod manifest;
pub mod output;
pub mod pipeline;
pub mod plan;
mod pr;
mod repo;
pub mod validation;

pub use context::{CommandDomain, ExecutionContext, PrMode};

#[derive(Debug, Parser)]
#[command(name = "bkt")]
#[command(about = "Bucket - manage your bootc manifests")]
#[command(version)]
#[command(propagate_version = true)]
pub struct Cli {
    /// Execution context (auto-detected if not specified)
    ///
    /// - host: Execute on the immutable host system
    /// - dev: Execute in the development toolbox
    /// - image: Only update manifests (no local execution)
    #[arg(long, value_enum, global = true)]
    pub context: Option<ExecutionContext>,

    /// Skip local execution, only create PR
    ///
    /// Useful for preparing changes on one machine for another,
    /// or for testing manifest changes in CI before applying.
    #[arg(long, global = true, conflicts_with = "local")]
    pub pr_only: bool,

    /// Skip PR creation, only execute locally
    ///
    /// Useful for temporary installations or testing before committing.
    /// Changes are recorded in the ephemeral manifest for later promotion.
    #[arg(long, global = true, conflicts_with = "pr_only")]
    pub local: bool,

    /// Show what would be done without making changes
    #[arg(long, short = 'n', global = true)]
    pub dry_run: bool,

    /// Skip preflight checks for PR workflow
    #[arg(long, global = true)]
    pub skip_preflight: bool,

    /// Don't auto-delegate to host/toolbox (for debugging)
    #[arg(long, global = true, hide = true)]
    pub no_delegate: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Privileged operations (bootc, systemctl) via polkit
    ///
    /// Provides passwordless access to system administration commands
    /// for wheel group members. Works from both host and toolbox.
    ///
    /// Read-only operations (status) are always passwordless.
    /// Mutating operations require explicit --confirm flag.
    Admin(commands::admin::AdminArgs),

    /// Apply all manifests to the running system
    Apply(commands::apply::ApplyArgs),

    /// Capture system state to manifests
    Capture(commands::capture::CaptureArgs),

    /// Manage system packages in the bootc image
    ///
    /// Add or remove packages from the image recipe (deferred until rebuild).
    /// Updates manifests/system-packages.json and Containerfile, then creates a PR.
    #[command(alias = "sys")]
    System(commands::system::SystemArgs),

    /// Development toolbox commands (shortcut for --context dev)
    Dev(commands::dev::DevArgs),

    /// Manage Flatpak apps in the manifest
    #[command(alias = "fp")]
    Flatpak(commands::flatpak::FlatpakArgs),

    /// Manage Distrobox configuration
    Distrobox(commands::distrobox::DistroboxArgs),

    /// Manage AppImages via GearLever
    #[command(name = "appimage", alias = "ai")]
    AppImage(commands::appimage::AppImageArgs),

    /// Manage host binaries via fetchbin
    Fetchbin(commands::fetchbin::FetchbinArgs),

    /// Manage host shims for toolbox
    Shim(commands::shim::ShimArgs),

    /// Manage GNOME Shell extensions
    #[command(alias = "ext")]
    Extension(commands::extension::ExtensionArgs),

    /// Manage GSettings entries
    #[command(alias = "gs")]
    Gsetting(commands::gsetting::GSettingArgs),

    /// Manage Homebrew/Linuxbrew packages
    #[command(alias = "brew")]
    Homebrew(commands::homebrew::HomebrewArgs),

    /// Manage skeleton (skel) files
    Skel(commands::skel::SkelArgs),

    /// Manage system profile
    Profile(commands::profile::ProfileArgs),

    /// Repository information
    Repo(commands::repo::RepoArgs),

    /// Generate JSON schemas for manifest types
    Schema(commands::schema::SchemaArgs),

    /// Generate shell completions
    Completions(commands::completions::CompletionsArgs),

    /// Check system readiness for bkt workflows
    Doctor(commands::doctor::DoctorArgs),

    /// Show status of all manifest types
    Status(commands::status::StatusArgs),

    /// Manage upstream dependencies (themes, icons, fonts, tools)
    Upstream(commands::upstream::UpstreamArgs),

    /// Manage distribution changelog and version history
    Changelog(commands::changelog::ChangelogArgs),

    /// Check for configuration drift between manifests and system
    Drift(commands::drift::DriftArgs),

    /// Track what the upstream Bazzite image provides (for drift detection)
    Base(commands::base::BaseArgs),

    /// Generate and render build descriptions for container images
    #[command(name = "build-info")]
    BuildInfo(commands::build_info::BuildInfoArgs),

    /// Manage Containerfile managed sections
    Containerfile(commands::containerfile::ContainerfileArgs),

    /// Manage local-only changes (ephemeral manifest)
    ///
    /// View, commit, or clear changes made with --local. These changes
    /// are tracked for later promotion to a PR.
    Local(commands::local::LocalArgs),
}

impl Commands {
    /// Get the natural target for this command.
    ///
    /// This determines where the command wants to run, independent of where
    /// it's currently running. Used by `maybe_delegate()` to decide if we
    /// need to re-exec on a different host/container.
    pub fn target(&self) -> context::CommandTarget {
        use context::CommandTarget;
        match self {
            // Host-only commands (read system manifests or require host daemons)
            Commands::Flatpak(_) => CommandTarget::Host,
            Commands::Extension(_) => CommandTarget::Host,
            Commands::Gsetting(_) => CommandTarget::Host,
            Commands::Shim(_) => CommandTarget::Host,
            Commands::Capture(_) => CommandTarget::Host,
            Commands::Apply(_) => CommandTarget::Host,
            Commands::Status(_) => CommandTarget::Host, // Reads /usr/share/bootc-bootstrap/
            Commands::Doctor(_) => CommandTarget::Host, // Validates host toolchains
            Commands::Profile(_) => CommandTarget::Host, // Reads system manifests, calls rpm
            Commands::Base(_) => CommandTarget::Host,   // Requires host rpm/rpm-ostree
            Commands::System(_) => CommandTarget::Host, // System/image operations
            Commands::Distrobox(_) => CommandTarget::Host, // Distrobox config is host-level
            Commands::AppImage(_) => CommandTarget::Host, // AppImages are host-level
            Commands::Fetchbin(_) => CommandTarget::Host, // Host binaries
            Commands::Homebrew(_) => CommandTarget::Host, // Linuxbrew is host-level
            Commands::Admin(_) => CommandTarget::Host,  // Already handles delegation internally

            // Dev-only commands (toolbox operations)
            Commands::Dev(_) => CommandTarget::Dev,

            // Either: pure utilities or work on repo/user files only
            Commands::Drift(_) => CommandTarget::Either,
            Commands::Repo(_) => CommandTarget::Either,
            Commands::Schema(_) => CommandTarget::Either,
            Commands::Completions(_) => CommandTarget::Either,
            Commands::Upstream(_) => CommandTarget::Either,
            Commands::Changelog(_) => CommandTarget::Either,
            Commands::Skel(_) => CommandTarget::Either,
            Commands::BuildInfo(_) => CommandTarget::Either,
            Commands::Containerfile(_) => CommandTarget::Either,
            Commands::Local(_) => CommandTarget::Either,
        }
    }
}

/// Delegate to the appropriate context if needed.
///
/// This is called early in main(), after parsing but before command dispatch.
/// If we're in the wrong environment for the command's target, we re-exec
/// via distrobox-host-exec (toolbox→host) or distrobox enter (host→toolbox).
fn maybe_delegate(cli: &Cli) -> Result<()> {
    // Skip if explicitly disabled
    if cli.no_delegate {
        return Ok(());
    }

    // Skip if already delegated (prevent infinite recursion)
    if std::env::var("BKT_DELEGATED").is_ok() {
        return Ok(());
    }

    let runtime = context::detect_environment();
    let target = cli.command.target();

    match (runtime, target) {
        // In toolbox, command wants host → delegate to host
        (context::RuntimeEnvironment::Toolbox, context::CommandTarget::Host) => {
            if cli.dry_run {
                output::Output::dry_run("Would delegate to host: distrobox-host-exec bkt ...");
                return Ok(());
            }
            delegate_to_host()?;
        }

        // On host, command wants dev → delegate to toolbox
        (context::RuntimeEnvironment::Host, context::CommandTarget::Dev) => {
            if cli.dry_run {
                output::Output::dry_run(
                    "Would delegate to toolbox: distrobox enter bootc-dev -- bkt ...",
                );
                return Ok(());
            }
            delegate_to_toolbox()?;
        }

        // Generic container, command wants host → error (no delegation path)
        (context::RuntimeEnvironment::Container, context::CommandTarget::Host) => {
            anyhow::bail!(
                "Cannot run host commands from a generic container\n\n\
                 This command requires the host system, but you're in a container\n\
                 without distrobox-host-exec access.\n\n\
                 Options:\n  \
                 • Exit this container and run on the host\n  \
                 • Use a distrobox instead: distrobox create && distrobox enter"
            );
        }

        // All other cases: run locally
        _ => {}
    }

    Ok(())
}

/// Delegate the current command to the host via distrobox-host-exec.
fn delegate_to_host() -> Result<()> {
    output::Output::info("Delegating to host...");

    let args: Vec<String> = std::env::args().collect();
    let status = std::process::Command::new("distrobox-host-exec")
        .arg("bkt")
        .args(&args[1..]) // Skip argv[0] (the current binary path)
        .env("BKT_DELEGATED", "1") // Prevent recursion
        .status()
        .context("Failed to execute distrobox-host-exec")?;

    // Exit with the same code as the delegated command
    std::process::exit(status.code().unwrap_or(1));
}

/// Delegate the current command to the default toolbox.
fn delegate_to_toolbox() -> Result<()> {
    output::Output::info("Delegating to toolbox...");

    let args: Vec<String> = std::env::args().collect();
    let status = std::process::Command::new("distrobox")
        .arg("enter")
        .arg("bootc-dev")
        .arg("--")
        .arg("bkt")
        .args(&args[1..])
        .env("BKT_DELEGATED", "1")
        .status()
        .context("Failed to execute distrobox enter")?;

    std::process::exit(status.code().unwrap_or(1));
}

fn main() -> Result<()> {
    // Initialize tracing with RUST_LOG env filter
    // e.g., RUST_LOG=bkt=debug
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();

    // Check if we need to delegate to a different context (RFC-0010)
    maybe_delegate(&cli)?;

    // Create execution plan from global options
    let plan = pipeline::ExecutionPlan::from_cli(&cli);

    // Log detected context
    tracing::debug!(
        context = %plan.context,
        pr_mode = ?plan.pr_mode,
        dry_run = plan.dry_run,
        "Execution plan created"
    );

    match cli.command {
        Commands::Admin(args) => commands::admin::run(args, &plan),
        Commands::Apply(args) => commands::apply::run(args, &plan),
        Commands::Capture(args) => commands::capture::run(args, &plan),
        Commands::System(args) => commands::system::run(args, &plan),
        Commands::Dev(args) => commands::dev::run(args, &plan),
        Commands::Flatpak(args) => commands::flatpak::run(args, &plan),
        Commands::Distrobox(args) => commands::distrobox::run(args, &plan),
        Commands::AppImage(args) => commands::appimage::run(args, &plan),
        Commands::Fetchbin(args) => commands::fetchbin::run(args, &plan),
        Commands::Shim(args) => commands::shim::run(args, &plan),
        Commands::Extension(args) => commands::extension::run(args, &plan),
        Commands::Gsetting(args) => commands::gsetting::run(args, &plan),
        Commands::Homebrew(args) => commands::homebrew::run(args, &plan),
        Commands::Skel(args) => commands::skel::run(args, &plan),
        Commands::Profile(args) => commands::profile::run(args),
        Commands::Repo(args) => commands::repo::run(args),
        Commands::Schema(args) => commands::schema::run(args),
        Commands::Completions(args) => commands::completions::run(args),
        Commands::Doctor(args) => commands::doctor::run(args),
        Commands::Status(args) => commands::status::run(args),
        Commands::Upstream(args) => commands::upstream::run(args),
        Commands::Changelog(args) => commands::changelog::run(args),
        Commands::Drift(args) => commands::drift::run(args),
        Commands::Base(args) => commands::base::run(args),
        Commands::BuildInfo(args) => commands::build_info::run(args),
        Commands::Containerfile(args) => commands::containerfile::run(args, &plan),
        Commands::Local(args) => commands::local::run(args, &plan),
    }
}
