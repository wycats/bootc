use thiserror::Error;

#[derive(Debug, Error)]
pub enum CommonError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("archive error: {0}")]
    Archive(String),
    #[error("checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },
    #[error("http error: {0}")]
    Http(String),
    #[error("manifest error: {0}")]
    Manifest(String),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}
