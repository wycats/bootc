use crate::error::FetchError;
use crate::manifest::InstalledBinary;
use crate::runtime::{RuntimePool, RuntimeVersion};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::str::FromStr;

pub mod cargo;
#[path = "github.rs"]
pub mod github;
pub mod npm;

pub use cargo::CargoSource;
pub use github::GithubSource;

pub trait BinarySource: Send + Sync {
    fn source_type(&self) -> &'static str;

    fn resolve(&self, spec: &PackageSpec) -> Result<Vec<ResolvedVersion>, FetchError>;

    fn fetch(
        &self,
        spec: &PackageSpec,
        version: &ResolvedVersion,
        target_dir: &Path,
        runtime: &mut RuntimePool,
    ) -> Result<FetchedBinary, FetchError>;

    fn check_update(
        &self,
        installed: &InstalledBinary,
    ) -> Result<Option<ResolvedVersion>, FetchError>;
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PackageSpec {
    pub name: String,
    pub version_req: Option<String>,
    pub source: SourceConfig,
    pub binary_name: Option<String>,
}

impl PackageSpec {
    /// Normalize version requirement. Bare versions like "2.0" become "^2.0".
    pub fn normalized_version_req(&self) -> Option<String> {
        self.version_req.as_ref().map(|req| {
            let trimmed = req.trim();
            if trimmed.starts_with('^')
                || trimmed.starts_with('~')
                || trimmed.starts_with('>')
                || trimmed.starts_with('<')
                || trimmed.starts_with('=')
                || trimmed == "latest"
            {
                trimmed.to_string()
            } else {
                format!("^{trimmed}")
            }
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum SourceConfig {
    Npm {
        package: String,
    },
    Cargo {
        crate_name: String,
    },
    Github {
        repo: String,
        asset_pattern: Option<String>,
    },
}

impl FromStr for PackageSpec {
    type Err = FetchError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (source_part, version_req) = match value.split_once('@') {
            Some((left, right)) => (left, Some(right.to_string())),
            None => (value, None),
        };

        let (source, name) = source_part
            .split_once(':')
            .ok_or_else(|| FetchError::Parse("missing source prefix".to_string()))?;

        let source = match source {
            "npm" => SourceConfig::Npm {
                package: name.to_string(),
            },
            "cargo" => SourceConfig::Cargo {
                crate_name: name.to_string(),
            },
            "github" => SourceConfig::Github {
                repo: name.to_string(),
                asset_pattern: None,
            },
            other => return Err(FetchError::Parse(format!("unknown source type: {other}"))),
        };

        Ok(Self {
            name: name.to_string(),
            version_req,
            source,
            binary_name: None,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResolvedVersion {
    pub version: String,
    pub download_url: Option<String>,
    pub checksum: Option<String>,
    pub engines: Option<EngineRequirements>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EngineRequirements {
    pub node: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FetchedBinary {
    pub binary_path: PathBuf,
    pub version: String,
    pub sha256: String,
    pub runtime_used: Option<RuntimeVersion>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_npm_spec() {
        let spec = PackageSpec::from_str("npm:turbo@^2.0").expect("parse");
        assert_eq!(spec.name, "turbo");
        assert_eq!(spec.version_req.as_deref(), Some("^2.0"));
        assert_eq!(
            spec.source,
            SourceConfig::Npm {
                package: "turbo".to_string()
            }
        );
    }

    #[test]
    fn normalized_version_req_handles_bare_versions() {
        let spec = PackageSpec {
            name: "tool".to_string(),
            version_req: Some("2.0".to_string()),
            source: SourceConfig::Npm {
                package: "tool".to_string(),
            },
            binary_name: None,
        };

        assert_eq!(spec.normalized_version_req(), Some("^2.0".to_string()));
    }

    #[test]
    fn normalized_version_req_keeps_ranges() {
        let spec = PackageSpec {
            name: "tool".to_string(),
            version_req: Some(">=1.2".to_string()),
            source: SourceConfig::Npm {
                package: "tool".to_string(),
            },
            binary_name: None,
        };

        assert_eq!(spec.normalized_version_req(), Some(">=1.2".to_string()));
    }

    #[test]
    fn normalized_version_req_keeps_latest() {
        let spec = PackageSpec {
            name: "tool".to_string(),
            version_req: Some("latest".to_string()),
            source: SourceConfig::Npm {
                package: "tool".to_string(),
            },
            binary_name: None,
        };

        assert_eq!(spec.normalized_version_req(), Some("latest".to_string()));
    }
}
