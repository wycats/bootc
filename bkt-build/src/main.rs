use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod fetch;
mod repos;

#[derive(Parser)]
#[command(
    name = "bkt-build",
    about = "Build-time helper for bootc Containerfile"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Download, verify, and install an upstream entry
    Fetch {
        /// Name of the upstream entry
        name: String,
        /// Path to upstream manifest
        #[arg(long, default_value = "/tmp/upstream-manifest.json")]
        manifest: PathBuf,
    },
    /// Import GPG keys and write .repo files from external repos manifest
    SetupRepos {
        /// Path to external repos manifest
        #[arg(long, default_value = "/tmp/external-repos.json")]
        manifest: PathBuf,
    },
    /// Download RPMs for a named external repo from the manifest
    DownloadRpms {
        /// External repo name
        repo: String,
        /// Path to external repos manifest
        #[arg(long, default_value = "/tmp/external-repos.json")]
        manifest: PathBuf,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Fetch { name, manifest } => fetch::run(&name, &manifest),
        Commands::SetupRepos { manifest } => repos::setup_repos(&manifest),
        Commands::DownloadRpms { repo, manifest } => repos::download_rpms(&repo, &manifest),
    }
}
