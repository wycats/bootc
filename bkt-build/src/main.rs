use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod fetch;
mod repos;
mod vendor_artifacts;

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
    /// Resolve vendor artifacts from their feed URLs.
    /// Runs from the repo checkout (CI), so paths are relative to the repo root.
    ResolveVendorArtifacts {
        /// Path to vendor-artifacts.json manifest
        #[arg(long, default_value = "manifests/vendor-artifacts.json")]
        manifest: PathBuf,
        /// Output path for resolved manifest
        #[arg(long, default_value = "build/vendor-artifacts.resolved.json")]
        output: PathBuf,
    },
    /// Install a resolved vendor artifact by name.
    /// Runs inside the container build, where the resolved manifest has been
    /// COPY'd to /tmp/.
    InstallVendorArtifact {
        /// Artifact name
        name: String,
        /// Path to resolved vendor artifacts manifest
        #[arg(long, default_value = "/tmp/vendor-artifacts.resolved.json")]
        resolved: PathBuf,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Fetch { name, manifest } => fetch::run(&name, &manifest),
        Commands::SetupRepos { manifest } => repos::setup_repos(&manifest),
        Commands::DownloadRpms { repo, manifest } => repos::download_rpms(&repo, &manifest),
        Commands::ResolveVendorArtifacts { manifest, output } => {
            vendor_artifacts::resolve(&manifest, &output)
        }
        Commands::InstallVendorArtifact { name, resolved } => {
            vendor_artifacts::install(&name, &resolved)
        }
    }
}
