use thiserror::Error;

#[derive(Debug, Error)]
pub enum FetchError {
    #[error("network error: {0}")]
    Network(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("parse error: {0}")]
    Parse(String),
    #[error("no asset found matching pattern '{pattern}'. Available: {}", available.join(", "))]
    AssetNotFound {
        pattern: String,
        available: Vec<String>,
    },
    #[error("no download url for version {version}")]
    NoDownloadUrl { version: String },
    #[error("GitHub API error: {0}")]
    GitHubApi(String),
    #[error("npm registry error: {0}")]
    NpmRegistry(String),
    #[error("crates.io api error: {0}")]
    CratesIoApi(String),
    #[error("binary not found for package {package}. searched: {}", searched.join(", "))]
    BinaryNotFound { package: String, searched: Vec<String> },
    #[error("package {package} requires install scripts")]
    RequiresScripts { package: String },
    #[error("multiple binaries found: {}", binaries.join(", "))]
    MultipleBinaries { binaries: Vec<String> },
    #[error("pnpm install failed: {0}")]
    PnpmInstallFailed(String),
    #[error("cargo-binstall failed: {0}")]
    BinstallFailed(String),
    #[error("unsupported archive format: {0}")]
    UnsupportedArchive(String),
    #[error("unimplemented source")]
    Unimplemented,
}

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("runtime not available: {0}")]
    NotAvailable(String),
    #[error("unsupported platform: {os}-{arch}")]
    UnsupportedPlatform { os: String, arch: String },
    #[error("no compatible node version for requirement: {0}")]
    NoCompatibleNode(String),
    #[error("node download failed for {version}: {details}")]
    NodeDownloadFailed { version: String, details: String },
    #[error("pnpm download failed for {version}: {details}")]
    PnpmDownloadFailed { version: String, details: String },
    #[error("cargo-binstall download failed: {0}")]
    BinstallDownloadFailed(String),
    #[error("cargo-binstall asset not found: {0}")]
    BinstallAssetNotFound(String),
    #[error("cargo-binstall binary not found after extraction")]
    BinstallBinaryNotFound,
    #[error("checksum mismatch for {filename}: expected {expected}, got {actual}")]
    ChecksumMismatch {
        filename: String,
        expected: String,
        actual: String,
    },
    #[error("version index fetch failed: {0}")]
    VersionIndexFetch(String),
    #[error("shasum parse error: {0}")]
    ShasumParse(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("configuration error: {0}")]
    Config(String),
}

#[derive(Debug, Error)]
pub enum ManifestError {
    #[error("manifest io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("manifest parse error: {0}")]
    Parse(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fetch_error_display() {
        let err = FetchError::Network("timeout".to_string());
        assert_eq!(err.to_string(), "network error: timeout");
    }

    #[test]
    fn runtime_error_display() {
        let err = RuntimeError::NotAvailable("node".to_string());
        assert_eq!(err.to_string(), "runtime not available: node");
    }

    #[test]
    fn manifest_error_display() {
        let parse_err = serde_json::from_str::<serde_json::Value>("bad json").unwrap_err();
        let err = ManifestError::Parse(parse_err);
        assert!(err.to_string().starts_with("manifest parse error:"));
    }
}
