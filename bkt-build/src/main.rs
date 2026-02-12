use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod fetch;

#[derive(Parser)]
#[command(name = "bkt-build", about = "Build-time helper for bootc Containerfile")]
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
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Fetch { name, manifest } => fetch::run(&name, &manifest),
    }
}
