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
pub mod effects;
mod manifest;
pub mod pipeline;
mod pr;
mod repo;

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

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Manage RPM packages (rpm-ostree on host, dnf in toolbox)
    Dnf(commands::dnf::DnfArgs),

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
}

fn main() -> Result<()> {
    // Initialize tracing with RUST_LOG env filter
    // e.g., RUST_LOG=bkt=debug
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();

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
        Commands::Dnf(args) => commands::dnf::run(args, &plan),
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
    }
}
