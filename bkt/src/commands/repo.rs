//! Repository info command implementation.

use crate::repo::{find_repo_path, RepoConfig};
use anyhow::Result;
use clap::{Args, Subcommand};

#[derive(Debug, Args)]
pub struct RepoArgs {
    #[command(subcommand)]
    pub action: RepoAction,
}

#[derive(Debug, Subcommand)]
pub enum RepoAction {
    /// Show repository information
    Info {
        /// Output format (table, json)
        #[arg(short, long, default_value = "table")]
        format: String,
    },
    /// Show repository path
    Path,
}

pub fn run(args: RepoArgs) -> Result<()> {
    match args.action {
        RepoAction::Info { format } => {
            match RepoConfig::load() {
                Ok(config) => {
                    if format == "json" {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&serde_json::json!({
                                "owner": config.owner,
                                "name": config.name,
                                "url": config.url,
                                "default_branch": config.default_branch,
                            }))?
                        );
                    } else {
                        println!("Repository: {}/{}", config.owner, config.name);
                        println!("URL: {}", config.url);
                        println!("Default branch: {}", config.default_branch);
                    }
                }
                Err(e) => {
                    println!("Repository config not found: {}", e);
                    println!("(This is expected until the image is built with repo.json)");
                }
            }

            match find_repo_path() {
                Ok(path) => println!("Local path: {}", path.display()),
                Err(_) => println!("Local path: (not in repo)"),
            }
        }
        RepoAction::Path => {
            let path = find_repo_path()?;
            println!("{}", path.display());
        }
    }
    Ok(())
}
