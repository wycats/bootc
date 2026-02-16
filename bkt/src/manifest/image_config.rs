//! Image configuration manifest types.
//!
//! Describes the system configuration modules applied during the image
//! assembly stage of the Containerfile. Each module maps to a contiguous
//! block of Dockerfile instructions.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// A file to COPY into the image.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileCopy {
    /// Source path (relative to repo root)
    pub src: String,
    /// Destination path in the image
    pub dest: String,
    /// Optional file mode (e.g. "0755")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    /// Optional comment emitted as `# ...` before this COPY line
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

/// A module in the image configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ImageModule {
    /// Copy files into the image
    Files {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        comment: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        pre_run: Vec<String>,
        files: Vec<FileCopy>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        post_run: Vec<String>,
    },
    /// Enable a systemd unit via symlink
    SystemdEnable {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        comment: Option<String>,
        /// "system" or "user"
        scope: String,
        /// Unit name (e.g. "keyd.service")
        unit: String,
        /// Target (e.g. "multi-user.target")
        target: String,
    },
    /// ARG-gated optional feature
    OptionalFeature {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        comment: Option<String>,
        /// Build ARG name (e.g. "ENABLE_NM_DISABLE_WIFI_POWERSAVE")
        arg: String,
        /// Commands to run before staging COPY (e.g. mkdir)
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        staging_pre_run: Vec<String>,
        /// Source file (relative to repo root)
        src: String,
        /// Staging destination (always copied)
        staging: String,
        /// Final destination (only installed when ARG=1)
        dest: String,
        /// Commands to run after install (inside the if block)
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        post_install: Vec<String>,
    },
    /// Raw RUN commands
    Run {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        comment: Option<String>,
        commands: Vec<String>,
    },
    /// Application wrapper (generates a binary that launches via systemd-run)
    Wrapper {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        comment: Option<String>,
        /// Path to the actual binary to wrap
        target: String,
        /// systemd slice to run under (e.g., app-vscode.slice)
        slice: String,
        /// Output path for the wrapper binary
        output: String,
        /// Enable VS Code remote-cli passthrough detection
        #[serde(default)]
        remote_cli: bool,
        /// Description for the systemd scope
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
}

impl ImageModule {
    /// Get the module name.
    pub fn name(&self) -> &str {
        match self {
            ImageModule::Files { name, .. }
            | ImageModule::SystemdEnable { name, .. }
            | ImageModule::OptionalFeature { name, .. }
            | ImageModule::Run { name, .. }
            | ImageModule::Wrapper { name, .. } => name,
        }
    }

    /// Get the module comment.
    pub fn comment(&self) -> Option<&str> {
        match self {
            ImageModule::Files { comment, .. }
            | ImageModule::SystemdEnable { comment, .. }
            | ImageModule::OptionalFeature { comment, .. }
            | ImageModule::Run { comment, .. }
            | ImageModule::Wrapper { comment, .. } => comment.as_deref(),
        }
    }
}

/// The image-config.json manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageConfigManifest {
    #[serde(rename = "$schema", skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,

    /// Ordered list of modules to apply during image assembly.
    pub modules: Vec<ImageModule>,
}

impl ImageConfigManifest {
    /// Resolve the path to the image-config.json file in the repo.
    pub fn path() -> Result<PathBuf> {
        let repo_path = crate::repo::find_repo_path()?;
        Ok(repo_path.join("manifests").join("image-config.json"))
    }

    /// Load the manifest from the repository.
    pub fn load() -> Result<Self> {
        let path = Self::path()?;
        Self::load_from_path(&path)
    }

    /// Load a manifest from a specific path.
    pub fn load_from_path(path: &PathBuf) -> Result<Self> {
        let content = fs::read_to_string(path).with_context(|| {
            format!(
                "Failed to read image config manifest from {}",
                path.display()
            )
        })?;
        let manifest: Self = serde_json::from_str(&content).with_context(|| {
            format!(
                "Failed to parse image config manifest from {}",
                path.display()
            )
        })?;
        Ok(manifest)
    }

    /// Load the manifest from a repo root path.
    pub fn load_from_repo(repo_root: &std::path::Path) -> Result<Self> {
        let path = repo_root.join("manifests").join("image-config.json");
        Self::load_from_path(&path)
    }

    /// Extract wrapper configurations from the manifest.
    pub fn wrappers(&self) -> Vec<crate::commands::wrap::WrapperConfig> {
        self.modules
            .iter()
            .filter_map(|m| match m {
                ImageModule::Wrapper {
                    name,
                    target,
                    slice,
                    output,
                    remote_cli,
                    description,
                    ..
                } => Some(crate::commands::wrap::WrapperConfig {
                    name: name.clone(),
                    target: target.clone(),
                    slice: slice.clone(),
                    output: output.clone(),
                    remote_cli: *remote_cli,
                    description: description.clone(),
                }),
                _ => None,
            })
            .collect()
    }
}
