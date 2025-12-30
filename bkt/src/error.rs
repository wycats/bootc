//! Custom error types for bkt.

use thiserror::Error;

#[allow(dead_code)]
#[derive(Error, Debug)]
pub enum BktError {
    #[error("Manifest not found: {path}")]
    ManifestNotFound { path: String },

    #[error("Invalid manifest format: {message}")]
    InvalidManifest { message: String },

    #[error("Repository configuration not found at {path}")]
    RepoConfigNotFound { path: String },

    #[error("Item not found: {kind} '{name}'")]
    ItemNotFound { kind: String, name: String },

    #[error("Item already exists: {kind} '{name}'")]
    ItemAlreadyExists { kind: String, name: String },

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}
