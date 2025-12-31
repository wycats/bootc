//! bkt - Bucket: manage your bootc manifests
//!
//! A CLI tool for managing system manifests including Flatpaks, GNOME extensions,
//! GSettings, host shims, skel files, and system profiles.

use anyhow::Result;
use clap::{Parser, Subcommand};

mod commands;
mod manifest;
mod pr;
mod repo;

#[derive(Debug, Parser)]
#[command(name = "bkt")]
#[command(about = "Bucket - manage your bootc manifests")]
#[command(version)]
#[command(propagate_version = true)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
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
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Flatpak(args) => commands::flatpak::run(args),
        Commands::Shim(args) => commands::shim::run(args),
        Commands::Extension(args) => commands::extension::run(args),
        Commands::Gsetting(args) => commands::gsetting::run(args),
        Commands::Skel(args) => commands::skel::run(args),
        Commands::Profile(args) => commands::profile::run(args),
        Commands::Repo(args) => commands::repo::run(args),
        Commands::Schema(args) => commands::schema::run(args),
        Commands::Completions(args) => commands::completions::run(args),
        Commands::Doctor(args) => commands::doctor::run(args),
    }
}
