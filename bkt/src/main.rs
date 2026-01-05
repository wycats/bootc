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

use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

mod commands;
pub mod context;
mod dbus;
mod delegation;
pub mod effects;
mod manifest;
pub mod output;
pub mod pipeline;
pub mod plan;
mod pr;
mod repo;
pub mod validation;

use context::CommandTarget;
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

    /// Don't auto-delegate to host/toolbox (for debugging)
    #[arg(long, global = true, hide = true)]
    pub no_delegate: bool,

    /// Show what would be done without making changes
    #[arg(long, short = 'n', global = true)]
    pub dry_run: bool,

    /// Skip preflight checks for PR workflow
    #[arg(long, global = true)]
    pub skip_preflight: bool,

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

    /// Manage RPM packages (rpm-ostree on host, dnf in toolbox)
    Dnf(commands::dnf::DnfArgs),

    /// Development toolbox commands (shortcut for --context dev)
    Dev(commands::dev::DevArgs),

    /// Manage Flatpak apps in the manifest
    #[command(alias = "fp")]
    Flatpak(commands::flatpak::FlatpakArgs),

    /// Manage host shims for toolbox
    Shim(commands::shim::ShimArgs),

    /// Manage GNOME Shell extensions
    #[command(alias = "ext")]
    Extension(commands::extension::ExtensionArgs),

    /// Manage GSettings entries
    #[command(alias = "gs")]
    Gsetting(commands::gsetting::GSettingArgs),

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
}

impl Commands {
    /// Get the natural execution target for this command.
    ///
    /// Determines whether the command must run on the host, in the dev toolbox,
    /// or can run in either context. Used by the delegation system to decide
    /// if automatic re-execution is needed.
    pub fn target(&self) -> CommandTarget {
        match self {
            // Host-only commands
            Commands::Flatpak(_) => CommandTarget::Host,
            Commands::Extension(_) => CommandTarget::Host,
            Commands::Gsetting(_) => CommandTarget::Host,
            Commands::Shim(_) => CommandTarget::Host,
            Commands::Capture(_) => CommandTarget::Host,
            Commands::Apply(_) => CommandTarget::Host,
            Commands::Admin(_) => CommandTarget::Host,

            // Dev-only commands
            Commands::Dev(_) => CommandTarget::Dev,

            // Either: depends on explicit context or runtime
            Commands::Dnf(_) => CommandTarget::Either,
            Commands::Status(_) => CommandTarget::Either,
            Commands::Doctor(_) => CommandTarget::Either,
            Commands::Profile(_) => CommandTarget::Either,
            Commands::Repo(_) => CommandTarget::Either,
            Commands::Schema(_) => CommandTarget::Either,
            Commands::Completions(_) => CommandTarget::Either,
            Commands::Upstream(_) => CommandTarget::Either,
            Commands::Changelog(_) => CommandTarget::Either,
            Commands::Drift(_) => CommandTarget::Either,
            Commands::Base(_) => CommandTarget::Either,
            Commands::Skel(_) => CommandTarget::Either,
        }
    }
}

fn main() -> Result<()> {
    // Initialize tracing with RUST_LOG env filter
    // e.g., RUST_LOG=bkt=debug
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();

    // Check if we need to delegate to a different context
    // This happens BEFORE creating the execution plan, using only the command target
    let runtime = context::detect_environment();
    let target = match cli.context {
        Some(ExecutionContext::Host) => CommandTarget::Host,
        Some(ExecutionContext::Dev) => CommandTarget::Dev,
        Some(ExecutionContext::Image) => CommandTarget::Either, // Image = no execution
        None => cli.command.target(),
    };
    delegation::maybe_delegate(target, runtime, cli.dry_run, cli.no_delegate)?;

    // If we reach here, we're in the right context
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
        Commands::Dnf(args) => commands::dnf::run(args, &plan),
        Commands::Dev(args) => commands::dev::run(args, &plan),
        Commands::Flatpak(args) => commands::flatpak::run(args, &plan),
        Commands::Shim(args) => commands::shim::run(args, &plan),
        Commands::Extension(args) => commands::extension::run(args, &plan),
        Commands::Gsetting(args) => commands::gsetting::run(args, &plan),
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
    }
}
