//! Schema generation command implementation.

use crate::manifest::{
    FlatpakApp, FlatpakAppsManifest, FlatpakRemote, FlatpakRemotesManifest, GSetting,
    GSettingsManifest, GnomeExtensionsManifest, Shim, ShimsManifest,
};
use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use schemars::schema_for;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Args)]
pub struct SchemaArgs {
    #[command(subcommand)]
    pub action: SchemaAction,
}

#[derive(Debug, Subcommand)]
pub enum SchemaAction {
    /// Generate JSON schemas for all manifest types
    Generate {
        /// Output directory (if not specified, prints to stdout)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// List available schema types
    List,
}

/// Schema type information
struct SchemaInfo {
    name: &'static str,
    filename: &'static str,
    description: &'static str,
}

const SCHEMAS: &[SchemaInfo] = &[
    SchemaInfo {
        name: "FlatpakApp",
        filename: "flatpak-app.schema.json",
        description: "A single Flatpak application entry",
    },
    SchemaInfo {
        name: "FlatpakAppsManifest",
        filename: "flatpak-apps.schema.json",
        description: "The flatpak-apps.json manifest (list of Flatpak apps)",
    },
    SchemaInfo {
        name: "FlatpakRemote",
        filename: "flatpak-remote.schema.json",
        description: "A single Flatpak remote entry",
    },
    SchemaInfo {
        name: "FlatpakRemotesManifest",
        filename: "flatpak-remotes.schema.json",
        description: "The flatpak-remotes.json manifest (list of Flatpak remotes)",
    },
    SchemaInfo {
        name: "GnomeExtensionsManifest",
        filename: "gnome-extensions.schema.json",
        description: "The gnome-extensions.json manifest",
    },
    SchemaInfo {
        name: "GSetting",
        filename: "gsetting.schema.json",
        description: "A single GSettings entry",
    },
    SchemaInfo {
        name: "GSettingsManifest",
        filename: "gsettings.schema.json",
        description: "The gsettings.json manifest",
    },
    SchemaInfo {
        name: "Shim",
        filename: "shim.schema.json",
        description: "A single host shim entry",
    },
    SchemaInfo {
        name: "ShimsManifest",
        filename: "host-shims.schema.json",
        description: "The host-shims.json manifest",
    },
];

/// Generate all schemas and return them as (filename, json) pairs.
fn generate_all_schemas() -> Vec<(&'static str, String)> {
    vec![
        (
            "flatpak-app.schema.json",
            serde_json::to_string_pretty(&schema_for!(FlatpakApp)).unwrap(),
        ),
        (
            "flatpak-apps.schema.json",
            serde_json::to_string_pretty(&schema_for!(FlatpakAppsManifest)).unwrap(),
        ),
        (
            "flatpak-remote.schema.json",
            serde_json::to_string_pretty(&schema_for!(FlatpakRemote)).unwrap(),
        ),
        (
            "flatpak-remotes.schema.json",
            serde_json::to_string_pretty(&schema_for!(FlatpakRemotesManifest)).unwrap(),
        ),
        (
            "gnome-extensions.schema.json",
            serde_json::to_string_pretty(&schema_for!(GnomeExtensionsManifest)).unwrap(),
        ),
        (
            "gsetting.schema.json",
            serde_json::to_string_pretty(&schema_for!(GSetting)).unwrap(),
        ),
        (
            "gsettings.schema.json",
            serde_json::to_string_pretty(&schema_for!(GSettingsManifest)).unwrap(),
        ),
        (
            "shim.schema.json",
            serde_json::to_string_pretty(&schema_for!(Shim)).unwrap(),
        ),
        (
            "host-shims.schema.json",
            serde_json::to_string_pretty(&schema_for!(ShimsManifest)).unwrap(),
        ),
    ]
}

pub fn run(args: SchemaArgs) -> Result<()> {
    match args.action {
        SchemaAction::Generate { output } => {
            let schemas = generate_all_schemas();

            match output {
                Some(dir) => {
                    // Write schemas to files
                    fs::create_dir_all(&dir)
                        .with_context(|| format!("Failed to create directory {}", dir.display()))?;

                    for (filename, json) in schemas {
                        let path = dir.join(filename);
                        fs::write(&path, &json)
                            .with_context(|| format!("Failed to write {}", path.display()))?;
                        println!("Wrote {}", path.display());
                    }
                }
                None => {
                    // Print all schemas to stdout as a combined object
                    let mut combined: serde_json::Map<String, serde_json::Value> =
                        serde_json::Map::new();

                    for (filename, json) in schemas {
                        let value: serde_json::Value = serde_json::from_str(&json)?;
                        let name = filename.strip_suffix(".schema.json").unwrap_or(filename);
                        combined.insert(name.to_string(), value);
                    }

                    println!("{}", serde_json::to_string_pretty(&combined)?);
                }
            }
        }
        SchemaAction::List => {
            println!("Available schema types:\n");
            for info in SCHEMAS {
                println!("  {} ({})", info.name, info.filename);
                println!("    {}\n", info.description);
            }
        }
    }

    Ok(())
}
