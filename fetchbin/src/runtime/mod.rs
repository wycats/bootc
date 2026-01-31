use crate::error::RuntimeError;
use crate::manifest::RuntimeManifest;
use crate::platform::Platform;
use crate::runtime::node::{download_node, resolve_node_runtime, NodeVersionIndex};
use crate::runtime::pnpm::{
    download_pnpm, fetch_latest_pnpm_version, resolve_pnpm_runtime, PnpmRuntime,
};
use crate::source::github::checksum::{parse_checksum_file, sha256_hex};
use flate2::read::GzDecoder;
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, USER_AGENT};
use semver::VersionReq;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use tar::Archive;

pub mod node;
pub mod pnpm;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RuntimeVersion {
    Node(String),
}

#[derive(Debug, Clone, Default)]
pub struct RuntimePool {
    pub data_dir: PathBuf,
    pub manifest: RuntimeManifest,
}

impl RuntimePool {
    pub fn new(data_dir: PathBuf, manifest: RuntimeManifest) -> Self {
        Self { data_dir, manifest }
    }

    pub fn load(data_dir: PathBuf) -> Result<Self, RuntimeError> {
        fs::create_dir_all(&data_dir)?;
        let manifest_path = runtime_manifest_path(&data_dir);
        let manifest = if manifest_path.exists() {
            let content = fs::read_to_string(&manifest_path)?;
            serde_json::from_str(&content).map_err(|err| RuntimeError::Config(err.to_string()))?
        } else {
            RuntimeManifest::default()
        };

        Ok(Self { data_dir, manifest })
    }

    pub fn get_node(
        &mut self,
        requirement: Option<&str>,
    ) -> Result<node::NodeRuntime, RuntimeError> {
        let requirement = requirement
            .or(self.manifest.node.default.as_deref())
            .unwrap_or("lts");

        if let Some(runtime) = self.find_cached_node(requirement) {
            return Ok(runtime);
        }

        let client = Client::new();
        let index = NodeVersionIndex::fetch(&client)?;
        let version_info = if requirement.eq_ignore_ascii_case("lts") {
            index.current_lts()
        } else {
            index.find_compatible(requirement)
        }
        .ok_or_else(|| RuntimeError::NoCompatibleNode(requirement.to_string()))?;

        let version = version_info.version.clone();
        let dest = self.data_dir.join("toolchains").join("node").join(&version);
        let runtime = download_node(&client, &version, &dest, &Platform::current())?;

        if !self.manifest.node.installed.contains(&version) {
            self.manifest.node.installed.push(version.clone());
        }
        if self.manifest.node.default.is_none()
            || requirement.eq_ignore_ascii_case("lts")
            || requirement == version
        {
            self.manifest.node.default = Some(version);
        }

        Ok(runtime)
    }

    pub fn get_pnpm(&mut self) -> Result<PnpmRuntime, RuntimeError> {
        let version = match self.manifest.pnpm.version.clone() {
            Some(version) => version,
            None => {
                let client = Client::new();
                let version = fetch_latest_pnpm_version(&client)?;
                self.manifest.pnpm.version = Some(version.clone());
                version
            }
        };
        let normalized = version.trim_start_matches('v').to_string();

        if let Some(runtime) = self.find_cached_pnpm(&normalized) {
            return Ok(runtime);
        }

        let client = Client::new();
        let dest = self
            .data_dir
            .join("toolchains")
            .join("pnpm")
            .join(&normalized);
        let runtime = download_pnpm(&client, &normalized, &dest, &Platform::current())?;

        self.manifest.pnpm.version = Some(normalized);

        Ok(runtime)
    }

    pub fn get_binstall(&mut self) -> Result<PathBuf, RuntimeError> {
        let platform = Platform::current();
        let asset_name = binstall_asset_name(&platform)?;

        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static("fetchbin"));
        if let Ok(token) = std::env::var("GITHUB_TOKEN") {
            if let Ok(value) = HeaderValue::from_str(&format!("token {token}")) {
                headers.insert(AUTHORIZATION, value);
            }
        }

        let client = Client::builder()
            .default_headers(headers)
            .build()
            .unwrap_or_else(|_| Client::new());

        let release = fetch_binstall_release(&client)?;
        let version = release.tag_name.trim_start_matches('v').to_string();
        let dest = self
            .data_dir
            .join("toolchains")
            .join("cargo-binstall")
            .join(&version);

        if let Some(existing) = resolve_binstall_binary(&dest) {
            return Ok(existing);
        }

        let asset = release
            .assets
            .iter()
            .find(|asset| asset.name == asset_name)
            .ok_or_else(|| RuntimeError::BinstallAssetNotFound(asset_name.to_string()))?;

        let bytes = client
            .get(&asset.browser_download_url)
            .send()
            .map_err(|err| RuntimeError::BinstallDownloadFailed(err.to_string()))?
            .error_for_status()
            .map_err(|err| RuntimeError::BinstallDownloadFailed(err.to_string()))?
            .bytes()
            .map_err(|err| RuntimeError::BinstallDownloadFailed(err.to_string()))?
            .to_vec();

        let checksum_asset = release
            .assets
            .iter()
            .find(|asset| asset.name == "checksums.txt")
            .ok_or_else(|| RuntimeError::BinstallAssetNotFound("checksums.txt".to_string()))?;
        let checksum_bytes = client
            .get(&checksum_asset.browser_download_url)
            .send()
            .map_err(|err| RuntimeError::BinstallDownloadFailed(err.to_string()))?
            .error_for_status()
            .map_err(|err| RuntimeError::BinstallDownloadFailed(err.to_string()))?
            .bytes()
            .map_err(|err| RuntimeError::BinstallDownloadFailed(err.to_string()))?
            .to_vec();
        let checksum_text = String::from_utf8_lossy(&checksum_bytes);
        verify_binstall_checksum(&checksum_text, &asset.name, &bytes)?;

        if dest.exists() {
            fs::remove_dir_all(&dest)?;
        }
        fs::create_dir_all(&dest)?;

        extract_tgz(&bytes, &dest)?;

        let binstall_path =
            resolve_binstall_binary(&dest).ok_or(RuntimeError::BinstallBinaryNotFound)?;
        set_executable(&binstall_path)?;

        Ok(binstall_path)
    }

    pub fn update(&mut self) -> Result<RuntimeUpdateReport, RuntimeError> {
        let client = Client::new();
        let index = NodeVersionIndex::fetch(&client)?;
        let mut report = RuntimeUpdateReport::default();

        if let Some(lts) = index.current_lts() {
            let version = lts.version.clone();
            let already_installed = self.manifest.node.installed.contains(&version);
            if !already_installed {
                let dest = self.data_dir.join("toolchains").join("node").join(&version);
                download_node(&client, &version, &dest, &Platform::current())?;
                self.manifest.node.installed.push(version.clone());
                report.updated.push(format!("node:{version}"));
            }
            self.manifest.node.default = Some(version);
        }

        Ok(report)
    }

    pub fn prune(&mut self, used_versions: &HashSet<String>) -> Result<PruneReport, RuntimeError> {
        let mut removed = Vec::new();
        let mut retained = Vec::new();

        for version in &self.manifest.node.installed {
            if used_versions.contains(version) {
                retained.push(version.clone());
                continue;
            }

            let path = self.data_dir.join("toolchains").join("node").join(version);
            if path.exists() {
                fs::remove_dir_all(&path)?;
            }
            removed.push(version.clone());
        }

        self.manifest.node.installed = retained;
        if let Some(default) = &self.manifest.node.default {
            if !used_versions.contains(default) {
                self.manifest.node.default = None;
            }
        }

        Ok(PruneReport { removed })
    }

    pub fn save(&self) -> Result<(), RuntimeError> {
        let manifest_path = runtime_manifest_path(&self.data_dir);
        if let Some(parent) = manifest_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(&self.manifest)
            .map_err(|err| RuntimeError::Config(err.to_string()))?;
        fs::write(manifest_path, content)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RuntimeUpdateReport {
    pub updated: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PruneReport {
    pub removed: Vec<String>,
}

fn runtime_manifest_path(data_dir: &Path) -> PathBuf {
    data_dir.join("runtime.json")
}

impl RuntimePool {
    fn find_cached_node(&self, requirement: &str) -> Option<node::NodeRuntime> {
        let installed = self.manifest.node.installed.clone();

        let match_version = if requirement.eq_ignore_ascii_case("lts") {
            self.manifest.node.default.clone()
        } else if let Ok(req) = VersionReq::parse(requirement) {
            let mut best: Option<(semver::Version, String)> = None;
            for version in installed {
                if let Ok(parsed) = semver::Version::parse(&version) {
                    if req.matches(&parsed) {
                        let is_better = best
                            .as_ref()
                            .map(|(current, _)| parsed > *current)
                            .unwrap_or(true);
                        if is_better {
                            best = Some((parsed, version));
                        }
                    }
                }
            }
            best.map(|(_, version)| version)
        } else {
            let normalized = requirement.trim_start_matches('v');
            installed.into_iter().find(|version| version == normalized)
        };

        let version = match match_version {
            Some(value) => value,
            None => return None,
        };

        let base = self.data_dir.join("toolchains").join("node").join(&version);
        resolve_node_runtime(&version, &base)
    }

    fn find_cached_pnpm(&self, version: &str) -> Option<PnpmRuntime> {
        let base = self.data_dir.join("toolchains").join("pnpm").join(version);
        resolve_pnpm_runtime(version, &base)
    }
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    assets: Vec<GithubAsset>,
}

#[derive(Debug, Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}

fn fetch_binstall_release(client: &Client) -> Result<GithubRelease, RuntimeError> {
    let response = client
        .get("https://api.github.com/repos/cargo-bins/cargo-binstall/releases/latest")
        .send()
        .map_err(|err| RuntimeError::BinstallDownloadFailed(err.to_string()))?
        .error_for_status()
        .map_err(|err| RuntimeError::BinstallDownloadFailed(err.to_string()))?;

    response
        .json::<GithubRelease>()
        .map_err(|err| RuntimeError::BinstallDownloadFailed(err.to_string()))
}

pub(crate) fn binstall_asset_name(platform: &Platform) -> Result<&'static str, RuntimeError> {
    match (&platform.os, &platform.arch) {
        (crate::platform::Os::Linux, crate::platform::Arch::X86_64) => {
            Ok("cargo-binstall-x86_64-unknown-linux-gnu.tgz")
        }
        (crate::platform::Os::Linux, crate::platform::Arch::Aarch64) => {
            Ok("cargo-binstall-aarch64-unknown-linux-gnu.tgz")
        }
        _ => Err(RuntimeError::UnsupportedPlatform {
            os: platform.os.as_str().to_string(),
            arch: platform.arch.as_str().to_string(),
        }),
    }
}

fn extract_tgz(bytes: &[u8], dest: &Path) -> Result<(), RuntimeError> {
    let cursor = Cursor::new(bytes);
    let decoder = GzDecoder::new(cursor);
    let mut archive = Archive::new(decoder);
    archive
        .unpack(dest)
        .map_err(|err| RuntimeError::BinstallDownloadFailed(err.to_string()))?;
    Ok(())
}

fn verify_binstall_checksum(
    checksum_text: &str,
    asset_name: &str,
    bytes: &[u8],
) -> Result<(), RuntimeError> {
    let checksums = parse_checksum_file(checksum_text);
    let expected = checksums
        .get(asset_name)
        .or_else(|| {
            let name = Path::new(asset_name)
                .file_name()
                .and_then(|name| name.to_str())?;
            checksums.get(name)
        })
        .cloned();

    let Some(expected) = expected else {
        return Err(RuntimeError::ShasumParse(format!(
            "checksum entry not found for {asset_name}"
        )));
    };

    let actual = sha256_hex(bytes);
    if expected.to_lowercase() != actual {
        return Err(RuntimeError::ChecksumMismatch {
            filename: asset_name.to_string(),
            expected,
            actual,
        });
    }

    Ok(())
}

fn resolve_binstall_binary(dest: &Path) -> Option<PathBuf> {
    find_binstall_binary(dest)
}

fn find_binstall_binary(dir: &Path) -> Option<PathBuf> {
    let entries = fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_binstall_binary(&path) {
                return Some(found);
            }
        } else if path
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| name == "cargo-binstall" || name == "cargo-binstall.exe")
            .unwrap_or(false)
        {
            return Some(path);
        }
    }
    None
}

fn set_executable(path: &Path) -> Result<(), RuntimeError> {
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
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_runtime_dir(label: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("fetchbin-{label}-{stamp}"));
        fs::create_dir_all(&path).expect("create temp dir");
        path
    }

    #[test]
    fn test_get_node_uses_cached() {
        let data_dir = temp_runtime_dir("cached-node");
        let node_dir = data_dir
            .join("toolchains")
            .join("node")
            .join("22.2.0")
            .join("bin");
        fs::create_dir_all(&node_dir).expect("create node dir");
        fs::write(node_dir.join("node"), "").expect("node binary");
        fs::write(node_dir.join("npm"), "").expect("npm binary");

        let mut manifest = RuntimeManifest::default();
        manifest.node.installed.push("22.2.0".to_string());
        manifest.node.default = Some("22.2.0".to_string());

        let mut pool = RuntimePool::new(data_dir.clone(), manifest);
        let runtime = pool.get_node(Some("=22.2.0")).expect("cached runtime");

        assert_eq!(runtime.version, "22.2.0");
        assert_eq!(runtime.node_path, node_dir.join("node"));
    }

    #[test]
    fn test_prune_removes_unused() {
        let data_dir = temp_runtime_dir("prune-node");
        let node_base = data_dir.join("toolchains").join("node");
        let v18_dir = node_base.join("18.0.0").join("bin");
        let v20_dir = node_base.join("20.0.0").join("bin");
        fs::create_dir_all(&v18_dir).expect("create v18 dir");
        fs::create_dir_all(&v20_dir).expect("create v20 dir");
        fs::write(v18_dir.join("node"), "").expect("node binary");
        fs::write(v18_dir.join("npm"), "").expect("npm binary");
        fs::write(v20_dir.join("node"), "").expect("node binary");
        fs::write(v20_dir.join("npm"), "").expect("npm binary");

        let mut manifest = RuntimeManifest::default();
        manifest.node.installed = vec!["18.0.0".to_string(), "20.0.0".to_string()];
        manifest.node.default = Some("20.0.0".to_string());

        let mut pool = RuntimePool::new(data_dir.clone(), manifest);
        let mut used = HashSet::new();
        used.insert("20.0.0".to_string());
        let report = pool.prune(&used).expect("prune");

        assert_eq!(report.removed, vec!["18.0.0".to_string()]);
        assert!(!node_base.join("18.0.0").exists());
        assert!(node_base.join("20.0.0").exists());
    }

    #[test]
    fn test_get_pnpm_uses_cached() {
        let data_dir = temp_runtime_dir("cached-pnpm");
        let pnpm_dir = data_dir.join("toolchains").join("pnpm").join("8.15.0");
        fs::create_dir_all(&pnpm_dir).expect("create pnpm dir");
        fs::write(pnpm_dir.join("pnpm"), "").expect("pnpm binary");

        let mut manifest = RuntimeManifest::default();
        manifest.pnpm.version = Some("8.15.0".to_string());

        let mut pool = RuntimePool::new(data_dir.clone(), manifest);
        let runtime = pool.get_pnpm().expect("cached runtime");

        assert_eq!(runtime.version, "8.15.0");
        assert_eq!(runtime.pnpm_path, pnpm_dir.join("pnpm"));
    }

    #[test]
    fn test_binstall_asset_pattern() {
        let platform = Platform {
            os: crate::platform::Os::Linux,
            arch: crate::platform::Arch::X86_64,
        };
        assert_eq!(
            binstall_asset_name(&platform).expect("asset"),
            "cargo-binstall-x86_64-unknown-linux-gnu.tgz"
        );

        let platform = Platform {
            os: crate::platform::Os::Linux,
            arch: crate::platform::Arch::Aarch64,
        };
        assert_eq!(
            binstall_asset_name(&platform).expect("asset"),
            "cargo-binstall-aarch64-unknown-linux-gnu.tgz"
        );
    }

    #[test]
    fn test_verify_binstall_checksum() {
        let bytes = b"hello";
        let hash = sha256_hex(bytes);
        let checksum_text = format!("{hash}  cargo-binstall.tgz\n");

        verify_binstall_checksum(&checksum_text, "cargo-binstall.tgz", bytes)
            .expect("checksum ok");
    }

    #[test]
    fn test_verify_binstall_checksum_mismatch() {
        let bytes = b"hello";
        let checksum_text = "deadbeef  cargo-binstall.tgz\n";

        let err = verify_binstall_checksum(&checksum_text, "cargo-binstall.tgz", bytes)
            .expect_err("checksum error");
        assert!(matches!(err, RuntimeError::ChecksumMismatch { .. }));
    }
}
