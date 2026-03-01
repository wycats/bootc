//! CLI argument definitions for bkt.
//!
//! This module contains the clap-derived `Cli` and `Commands` types.
//! Separated from `main.rs` so that library code (e.g., `pipeline::ExecutionPlan::from_cli`)
//! and shell completion generation can reference these types.

use clap::{Parser, Subcommand};

use crate::commands;
use crate::context;
use crate::context::ExecutionContext;

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
    #[arg(long, global = true)]
    pub pr_only: bool,

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

    /// First-login bootstrap (Flatpak + extensions + gsettings + distrobox)
    ///
    /// Runs automatically via systemd user unit on first login.
    /// Reads manifests from /usr/share/bootc-bootstrap/ (baked into image).
    Bootstrap,

    /// Capture system state to manifests
    Capture(commands::capture::CaptureArgs),

    /// Manage system packages in the bootc image
    ///
    /// Add or remove packages from the image recipe (deferred until rebuild).
    /// Updates manifests/system-packages.json and Containerfile, then creates a PR.
    #[command(alias = "sys")]
    System(commands::system::SystemArgs),

    /// Try transient overlay installs while capturing manifest changes
    Try(commands::try_cmd::TryArgs),

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

    /// Migrate legacy user configuration into the repo
    Migrate(commands::migrate::MigrateArgs),

    /// Generate application wrapper binaries
    ///
    /// Creates Rust binaries that launch applications under systemd
    /// resource controls (slices). Replaces shell wrapper scripts.
    Wrap(commands::wrap::WrapArgs),

    /// Analyze and reclaim system memory (RAM, swap, GPU, caches)
    ///
    /// By default shows what could be reclaimed without taking action.
    /// Use --apply to actually perform reclamation.
    Tune(commands::tune::TuneArgs),
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
            Commands::Try(_) => CommandTarget::Host,    // Try operates on host overlay
            Commands::Distrobox(_) => CommandTarget::Host, // Distrobox config is host-level
            Commands::AppImage(_) => CommandTarget::Host, // AppImages are host-level
            Commands::Fetchbin(_) => CommandTarget::Host, // Host binaries
            Commands::Homebrew(_) => CommandTarget::Host, // Linuxbrew is host-level
            Commands::Admin(_) => CommandTarget::Host,  // Already handles delegation internally
            Commands::Bootstrap => CommandTarget::Host, // First-login setup on host

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
            Commands::Migrate(_) => CommandTarget::Either,
            Commands::Wrap(_) => CommandTarget::Either,
            Commands::Tune(_) => CommandTarget::Host, // Reads /proc, /sys for memory/GPU info
        }
    }
}
