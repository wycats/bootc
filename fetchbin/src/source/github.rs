#[path = "github/api.rs"]
mod api;
#[path = "github/archive.rs"]
mod archive;
#[path = "github/checksum.rs"]
pub(crate) mod checksum;

use crate::error::FetchError;
use crate::manifest::{InstalledBinary, SourceSpec};
use crate::platform::Platform;
use crate::runtime::RuntimePool;
use crate::source::{BinarySource, FetchedBinary, PackageSpec, ResolvedVersion, SourceConfig};
use api::{Asset, Release};
use archive::{detect_archive_type, extract_tar_gz, extract_zip, write_raw, ArchiveType};
use checksum::{find_checksum_asset, parse_checksum_file, sha256_hex};
use glob::Pattern;
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, USER_AGENT};
use std::env;
use std::fs;
use std::path::Path;

pub struct GithubSource {
    client: Client,
}

impl GithubSource {
    pub fn new() -> Self {
        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static("fetchbin"));

        if let Ok(token) = env::var("GITHUB_TOKEN") {
            if let Ok(value) = HeaderValue::from_str(&format!("token {token}")) {
                headers.insert(AUTHORIZATION, value);
            }
        }

        let client = Client::builder()
            .default_headers(headers)
            .build()
            .unwrap_or_else(|_| Client::new());

        Self { client }
    }

    fn fetch_releases(&self, repo: &str) -> Result<Vec<Release>, FetchError> {
        let url = format!("https://api.github.com/repos/{repo}/releases");
        let response = self
            .client
            .get(url)
            .send()
            .map_err(|err| FetchError::Network(err.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().unwrap_or_default();
            return Err(FetchError::GitHubApi(format!("status {status}: {body}")));
        }

        response
            .json::<Vec<Release>>()
            .map_err(|err| FetchError::Parse(err.to_string()))
    }

    fn find_asset<'a>(
        &self,
        release: &'a Release,
        pattern: Option<&str>,
    ) -> Result<&'a Asset, FetchError> {
        let available: Vec<String> = release
            .assets
            .iter()
            .map(|asset| asset.name.clone())
            .collect();

        let mut candidates: Vec<&Asset> = if let Some(pattern) = pattern {
            let pattern_lower = pattern.to_lowercase();
            let use_glob = pattern_lower.contains('*')
                || pattern_lower.contains('?')
                || pattern_lower.contains('[');

            if use_glob {
                let pattern = Pattern::new(&pattern_lower)
                    .map_err(|err| FetchError::Parse(err.to_string()))?;
                release
                    .assets
                    .iter()
                    .filter(|asset| pattern.matches(&asset.name.to_lowercase()))
                    .collect()
            } else {
                release
                    .assets
                    .iter()
                    .filter(|asset| asset.name.to_lowercase().contains(&pattern_lower))
                    .collect()
            }
        } else {
            let platform = Platform::current();
            release
                .assets
                .iter()
                .filter(|asset| platform.matches_asset(&asset.name))
                .collect()
        };

        if candidates.is_empty() {
            let pattern_label = if let Some(pattern) = pattern {
                pattern.to_string()
            } else {
                let platform = Platform::current();
                platform.asset_patterns().join(",")
            };

            return Err(FetchError::AssetNotFound {
                pattern: pattern_label,
                available,
            });
        }

        candidates.sort_by_key(|asset| asset_rank(&asset.name));
        Ok(candidates[0])
    }

    fn download_asset(&self, asset: &Asset) -> Result<Vec<u8>, FetchError> {
        if asset.browser_download_url.trim().is_empty() {
            return Err(FetchError::NoDownloadUrl {
                version: asset.name.clone(),
            });
        }

        let response = self
            .client
            .get(&asset.browser_download_url)
            .send()
            .map_err(|err| FetchError::Network(err.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().unwrap_or_default();
            return Err(FetchError::GitHubApi(format!("status {status}: {body}")));
        }

        let bytes = response
            .bytes()
            .map_err(|err| FetchError::Network(err.to_string()))?;
        Ok(bytes.to_vec())
    }
}

impl Default for GithubSource {
    fn default() -> Self {
        Self::new()
    }
}

impl BinarySource for GithubSource {
    fn source_type(&self) -> &'static str {
        "github"
    }

    fn resolve(&self, spec: &PackageSpec) -> Result<Vec<ResolvedVersion>, FetchError> {
        let (repo, asset_pattern) = match &spec.source {
            SourceConfig::Github {
                repo,
                asset_pattern,
            } => (repo.as_str(), asset_pattern.as_deref()),
            _ => {
                return Err(FetchError::Parse(
                    "GithubSource used with non-github spec".to_string(),
                ))
            }
        };

        let releases = self.fetch_releases(repo)?;
        let mut resolved = Vec::new();

        for release in releases
            .into_iter()
            .filter(|release| !release.prerelease && !release.draft)
        {
            match self.find_asset(&release, asset_pattern) {
                Ok(asset) => resolved.push(ResolvedVersion {
                    version: release.tag_name.clone(),
                    download_url: Some(asset.browser_download_url.clone()),
                    checksum: None,
                    engines: None,
                }),
                Err(FetchError::AssetNotFound { .. }) => continue,
                Err(err) => return Err(err),
            }
        }

        if resolved.is_empty() {
            return Err(FetchError::AssetNotFound {
                pattern: asset_pattern
                    .map(|pattern| pattern.to_string())
                    .unwrap_or_else(|| "platform".to_string()),
                available: Vec::new(),
            });
        }

        Ok(resolved)
    }

    fn fetch(
        &self,
        spec: &PackageSpec,
        version: &ResolvedVersion,
        target_dir: &Path,
        _runtime: &mut RuntimePool,
    ) -> Result<FetchedBinary, FetchError> {
        let (repo, asset_pattern) = match &spec.source {
            SourceConfig::Github {
                repo,
                asset_pattern,
            } => (repo.as_str(), asset_pattern.as_deref()),
            _ => {
                return Err(FetchError::Parse(
                    "GithubSource used with non-github spec".to_string(),
                ))
            }
        };

        let releases = self.fetch_releases(repo)?;
        let release = releases
            .iter()
            .find(|release| versions_match(&release.tag_name, &version.version))
            .ok_or_else(|| FetchError::Parse(format!("version {} not found", version.version)))?;

        let asset = self.find_asset(release, asset_pattern)?;

        if is_unsupported_archive(&asset.name) {
            return Err(FetchError::UnsupportedArchive(asset.name.clone()));
        }

        let asset_bytes = self.download_asset(asset)?;

        if let Some(checksum_asset) = find_checksum_asset(release, asset) {
            let checksum_bytes = self.download_asset(checksum_asset)?;
            let checksum_text = String::from_utf8_lossy(&checksum_bytes);
            let checksums = parse_checksum_file(&checksum_text);
            let expected = checksums
                .get(&asset.name)
                .or_else(|| {
                    let name = Path::new(&asset.name)
                        .file_name()
                        .and_then(|name| name.to_str())?;
                    checksums.get(name)
                })
                .cloned();

            let Some(expected) = expected else {
                return Err(FetchError::Parse(format!(
                    "checksum entry not found for {}",
                    asset.name
                )));
            };

            let actual = sha256_hex(&asset_bytes);
            if expected.to_lowercase() != actual {
                return Err(FetchError::Parse(format!(
                    "checksum mismatch for {}: expected {}, got {}",
                    asset.name, expected, actual
                )));
            }
        } else {
            eprintln!("warning: no checksum found for {}", asset.name);
        }

        fs::create_dir_all(target_dir)?;
        let binary_name = spec
            .binary_name
            .clone()
            .unwrap_or_else(|| repo_name(repo).to_string());

        let binary_path = match detect_archive_type(&asset.name) {
            ArchiveType::TarGz => extract_tar_gz(&asset_bytes, target_dir, &binary_name)?,
            ArchiveType::Zip => extract_zip(&asset_bytes, target_dir, &binary_name)?,
            ArchiveType::Raw => write_raw(&asset_bytes, target_dir, &binary_name)?,
        };

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
        installed: &InstalledBinary,
    ) -> Result<Option<ResolvedVersion>, FetchError> {
        let (repo, current_version) = match &installed.source {
            SourceSpec::Github { repo, version, .. } => (repo, version),
            _ => {
                return Err(FetchError::Parse(
                    "GithubSource used with non-github install".to_string(),
                ))
            }
        };

        let spec = PackageSpec {
            name: repo_name(repo).to_string(),
            version_req: None,
            source: SourceConfig::Github {
                repo: repo.clone(),
                asset_pattern: None,
            },
            binary_name: Some(installed.binary.clone()),
        };

        let releases = self.resolve(&spec)?;
        let Some(latest) = releases.first() else {
            return Ok(None);
        };

        if versions_match(&latest.version, current_version) {
            Ok(None)
        } else {
            Ok(Some(latest.clone()))
        }
    }
}

fn asset_rank(name: &str) -> u8 {
    let lower = name.to_lowercase();
    if lower.ends_with(".tar.gz") {
        0
    } else if lower.ends_with(".zip") {
        1
    } else {
        2
    }
}

fn repo_name(repo: &str) -> &str {
    repo.rsplit('/').next().unwrap_or(repo)
}

fn versions_match(left: &str, right: &str) -> bool {
    normalize_version(left) == normalize_version(right)
}

fn normalize_version(value: &str) -> &str {
    value.strip_prefix('v').unwrap_or(value)
}

fn is_unsupported_archive(name: &str) -> bool {
    let lower = name.to_lowercase();
    // Check for supported formats first (.tgz is equivalent to .tar.gz)
    if lower.ends_with(".tar.gz") || lower.ends_with(".tgz") || lower.ends_with(".zip") {
        return false;
    }
    // These are unsupported archive formats
    lower.ends_with(".tar")
        || lower.ends_with(".tar.bz2")
        || lower.ends_with(".tar.xz")
        || lower.ends_with(".gz")
        || lower.ends_with(".bz2")
        || lower.ends_with(".xz")
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
