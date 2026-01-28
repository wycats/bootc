use crate::error::FetchError;
use crate::manifest::{InstalledBinary, SourceSpec};
use crate::runtime::RuntimePool;
use crate::source::github::checksum::sha256_hex;
use crate::source::{BinarySource, FetchedBinary, PackageSpec, ResolvedVersion, SourceConfig};
use reqwest::blocking::Client;
use semver::{Version, VersionReq};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct CargoSource {
    client: Client,
    data_dir: PathBuf,
}

impl CargoSource {
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            client: Client::new(),
            data_dir,
        }
    }

    fn fetch_metadata(&self, crate_name: &str) -> Result<CratesIoResponse, FetchError> {
        let url = format!("https://crates.io/api/v1/crates/{crate_name}");
        let response = self
            .client
            .get(url)
            .send()
            .map_err(|err| FetchError::CratesIoApi(err.to_string()))?
            .error_for_status()
            .map_err(|err| FetchError::CratesIoApi(err.to_string()))?;

        response
            .json::<CratesIoResponse>()
            .map_err(|err| FetchError::Parse(err.to_string()))
    }
}

impl Default for CargoSource {
    fn default() -> Self {
        let data_dir = dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("fetchbin");
        Self::new(data_dir)
    }
}

#[derive(Debug, Deserialize)]
struct CratesIoResponse {
    #[serde(rename = "crate")]
    krate: CrateInfo,
    versions: Vec<CrateVersion>,
}

#[derive(Debug, Deserialize)]
struct CrateInfo {
    name: String,
    max_version: String,
}

#[derive(Debug, Deserialize)]
struct CrateVersion {
    num: String,
    #[serde(default)]
    yanked: bool,
}

impl BinarySource for CargoSource {
    fn source_type(&self) -> &'static str {
        "cargo"
    }

    fn resolve(&self, spec: &PackageSpec) -> Result<Vec<ResolvedVersion>, FetchError> {
        let crate_name = match &spec.source {
            SourceConfig::Cargo { crate_name } => crate_name.as_str(),
            _ => {
                return Err(FetchError::Parse(
                    "CargoSource used with non-cargo spec".to_string(),
                ))
            }
        };

        let metadata = self.fetch_metadata(crate_name)?;
        resolve_versions(&metadata, spec.version_req.as_deref())
    }

    fn fetch(
        &self,
        spec: &PackageSpec,
        version: &ResolvedVersion,
        target_dir: &Path,
        runtime: &mut RuntimePool,
    ) -> Result<FetchedBinary, FetchError> {
        let crate_name = match &spec.source {
            SourceConfig::Cargo { crate_name } => crate_name.as_str(),
            _ => {
                return Err(FetchError::Parse(
                    "CargoSource used with non-cargo spec".to_string(),
                ))
            }
        };

        let mut runtime_pool = runtime.clone();
        let binstall = runtime_pool
            .get_binstall()
            .map_err(|err| FetchError::BinstallFailed(err.to_string()))?;

        fs::create_dir_all(target_dir)?;

        let cargo_home = self.data_dir.join("toolchains").join("cargo");
        let output = Command::new(&binstall)
            .arg("--no-confirm")
            .arg("--version")
            .arg(&version.version)
            .arg("--root")
            .arg(target_dir)
            .arg(crate_name)
            .env("CARGO_HOME", cargo_home)
            .output()
            .map_err(|err| {
                FetchError::BinstallFailed(format!("failed to run cargo-binstall: {err}"))
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(FetchError::BinstallFailed(stderr.to_string()));
        }

        let binary_name = spec
            .binary_name
            .clone()
            .unwrap_or_else(|| crate_name.to_string());
        let binary_path = target_dir.join("bin").join(&binary_name);
        if !binary_path.exists() {
            return Err(FetchError::BinaryNotFound {
                package: crate_name.to_string(),
                searched: vec![binary_path.display().to_string()],
            });
        }

        set_executable(&binary_path)?;
        let sha256 = sha256_hex(&fs::read(&binary_path)?);

        Ok(FetchedBinary {
            binary_path,
            version: version.version.clone(),
            sha256,
            runtime_used: None,
        })
    }

    fn check_update(
        &self,
        _installed: &InstalledBinary,
    ) -> Result<Option<ResolvedVersion>, FetchError> {
        let (crate_name, current_version) = match &_installed.source {
            SourceSpec::Cargo {
                crate_name,
                version,
            } => (crate_name, version),
            _ => {
                return Err(FetchError::Parse(
                    "CargoSource used with non-cargo install".to_string(),
                ))
            }
        };

        let spec = PackageSpec {
            name: crate_name.clone(),
            version_req: None,
            source: SourceConfig::Cargo {
                crate_name: crate_name.clone(),
            },
            binary_name: Some(_installed.binary.clone()),
        };

        let latest = self
            .resolve(&spec)?
            .into_iter()
            .next()
            .ok_or_else(|| FetchError::Parse("no versions returned".to_string()))?;

        if versions_match(&latest.version, current_version) {
            Ok(None)
        } else {
            Ok(Some(latest))
        }
    }
}

fn resolve_versions(
    metadata: &CratesIoResponse,
    version_req: Option<&str>,
) -> Result<Vec<ResolvedVersion>, FetchError> {
    let available: Vec<&CrateVersion> = metadata
        .versions
        .iter()
        .filter(|version| !version.yanked)
        .collect();

    if available.is_empty() {
        return Err(FetchError::Parse(
            "no non-yanked versions found".to_string(),
        ));
    }

    let resolved = if let Some(req) = version_req {
        if req == "latest" {
            latest_version(metadata, &available)
        } else if let Some(version) = find_version(&available, req) {
            vec![resolved_from_version(version)]
        } else if let Ok(req) = VersionReq::parse(req) {
            let mut matching = matching_versions(&available, &req);
            if matching.is_empty() {
                return Err(FetchError::Parse(format!("no versions matching {req}")));
            }
            matching
                .drain(..)
                .map(|(_, version)| resolved_from_version(version))
                .collect()
        } else {
            return Err(FetchError::Parse(format!(
                "unsupported cargo version requirement: {req}"
            )));
        }
    } else {
        latest_version(metadata, &available)
    };

    Ok(resolved)
}

fn latest_version(
    metadata: &CratesIoResponse,
    available: &[&CrateVersion],
) -> Vec<ResolvedVersion> {
    let max_version = metadata.krate.max_version.as_str();
    if let Some(version) = available
        .iter()
        .find(|version| versions_match(&version.num, max_version))
    {
        return vec![resolved_from_version(version)];
    }

    let mut parsed: Vec<(Version, &CrateVersion)> = available
        .iter()
        .filter_map(|version| Version::parse(&version.num).ok().map(|v| (v, *version)))
        .collect();
    parsed.sort_by(|(left, _), (right, _)| right.cmp(left));
    parsed
        .first()
        .map(|(_, version)| vec![resolved_from_version(version)])
        .unwrap_or_default()
}

fn resolved_from_version(version: &CrateVersion) -> ResolvedVersion {
    ResolvedVersion {
        version: version.num.clone(),
        download_url: None,
        checksum: None,
        engines: None,
    }
}

fn find_version<'a>(versions: &'a [&CrateVersion], requested: &str) -> Option<&'a CrateVersion> {
    versions
        .iter()
        .find(|version| versions_match(&version.num, requested))
        .copied()
}

fn matching_versions<'a>(
    versions: &'a [&CrateVersion],
    req: &VersionReq,
) -> Vec<(Version, &'a CrateVersion)> {
    let mut matches = Vec::new();
    for version in versions {
        if let Ok(parsed) = Version::parse(&version.num) {
            if req.matches(&parsed) {
                matches.push((parsed, *version));
            }
        }
    }
    matches.sort_by(|(left, _), (right, _)| right.cmp(left));
    matches
}

fn versions_match(left: &str, right: &str) -> bool {
    normalize_version(left) == normalize_version(right)
}

fn normalize_version(value: &str) -> &str {
    value.trim_start_matches('v')
}

fn set_executable(path: &Path) -> Result<(), FetchError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_crates_io_response() {
        let json = r#"{
            "crate": { "name": "ripgrep", "max_version": "14.1.0" },
            "versions": [
                { "num": "14.1.0", "yanked": false },
                { "num": "14.0.0", "yanked": false }
            ]
        }"#;

        let parsed: CratesIoResponse = serde_json::from_str(json).expect("parse");
        assert_eq!(parsed.krate.name, "ripgrep");
        assert_eq!(parsed.krate.max_version, "14.1.0");
        assert_eq!(parsed.versions.len(), 2);
        assert!(!parsed.versions[0].yanked);
    }

    #[test]
    fn test_resolve_crate_versions() {
        let json = r#"{
            "crate": { "name": "bat", "max_version": "1.3.0" },
            "versions": [
                { "num": "1.3.0", "yanked": false },
                { "num": "1.2.0", "yanked": false },
                { "num": "2.0.0", "yanked": true }
            ]
        }"#;

        let parsed: CratesIoResponse = serde_json::from_str(json).expect("parse");
        let resolved = resolve_versions(&parsed, Some("^1.0")).expect("resolve");
        let versions: Vec<&str> = resolved.iter().map(|r| r.version.as_str()).collect();
        assert_eq!(versions, vec!["1.3.0", "1.2.0"]);
    }
}
