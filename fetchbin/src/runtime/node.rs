use crate::error::RuntimeError;
use crate::platform::{Arch, Os, Platform};
use crate::source::github::checksum::sha256_hex;
use flate2::read::GzDecoder;
use reqwest::blocking::Client;
use semver::VersionReq;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use tar::Archive;

#[derive(Debug, Clone)]
pub struct NodeRuntime {
    pub version: String,
    pub node_path: PathBuf,
    pub npm_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct NodeVersionIndex {
    pub versions: Vec<NodeVersionInfo>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NodeVersionInfo {
    pub version: String,
    #[serde(default)]
    pub lts: Option<String>,
    pub date: String,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum LtsValue {
    Bool(bool),
    String(String),
}

#[derive(Debug, Deserialize)]
struct RawNodeVersionInfo {
    version: String,
    #[serde(default)]
    lts: Option<LtsValue>,
    date: String,
}

impl NodeVersionIndex {
    pub fn fetch(client: &Client) -> Result<Self, RuntimeError> {
        let response = client
            .get("https://nodejs.org/dist/index.json")
            .send()
            .map_err(|err| RuntimeError::VersionIndexFetch(err.to_string()))?
            .error_for_status()
            .map_err(|err| RuntimeError::VersionIndexFetch(err.to_string()))?;

        let content = response
            .text()
            .map_err(|err| RuntimeError::VersionIndexFetch(err.to_string()))?;

        Self::parse_json(&content)
    }

    pub fn find_compatible(&self, requirement: &str) -> Option<&NodeVersionInfo> {
        if requirement.eq_ignore_ascii_case("lts") {
            return self.current_lts();
        }

        if let Ok(req) = VersionReq::parse(requirement) {
            let mut fallback = None;
            for info in &self.versions {
                if let Ok(version) = semver::Version::parse(&info.version) {
                    if req.matches(&version) {
                        if info.lts.is_some() {
                            return Some(info);
                        }
                        if fallback.is_none() {
                            fallback = Some(info);
                        }
                    }
                }
            }
            return fallback;
        }

        let normalized = requirement.trim_start_matches('v');
        self.versions
            .iter()
            .find(|info| info.version == normalized)
    }

    pub fn current_lts(&self) -> Option<&NodeVersionInfo> {
        self.versions.iter().find(|info| info.lts.is_some())
    }

    fn parse_json(content: &str) -> Result<Self, RuntimeError> {
        let raw: Vec<RawNodeVersionInfo> =
            serde_json::from_str(content).map_err(|err| RuntimeError::VersionIndexFetch(err.to_string()))?;
        let versions = raw
            .into_iter()
            .map(|info| NodeVersionInfo {
                version: info.version.trim_start_matches('v').to_string(),
                lts: match info.lts {
                    Some(LtsValue::String(value)) => Some(value),
                    Some(LtsValue::Bool(true)) => Some("lts".to_string()),
                    _ => None,
                },
                date: info.date,
            })
            .collect();
        Ok(Self { versions })
    }
}

pub fn download_node(
    client: &Client,
    version: &str,
    dest: &Path,
    platform: &Platform,
) -> Result<NodeRuntime, RuntimeError> {
    let filename = node_download_filename(version, platform)?;
    let base_url = format!("https://nodejs.org/dist/v{version}");
    let archive_url = format!("{base_url}/{filename}");
    let shasum_url = format!("{base_url}/SHASUMS256.txt");

    let shasum_content = client
        .get(shasum_url)
        .send()
        .map_err(|err| RuntimeError::NodeDownloadFailed {
            version: version.to_string(),
            details: err.to_string(),
        })?
        .error_for_status()
        .map_err(|err| RuntimeError::NodeDownloadFailed {
            version: version.to_string(),
            details: err.to_string(),
        })?
        .text()
        .map_err(|err| RuntimeError::NodeDownloadFailed {
            version: version.to_string(),
            details: err.to_string(),
        })?;

    let checksums = parse_shasums(&shasum_content)?;
    let expected = checksums.get(&filename).ok_or_else(|| {
        RuntimeError::ShasumParse(format!("missing checksum for {filename}"))
    })?;

    let archive_bytes = client
        .get(archive_url)
        .send()
        .map_err(|err| RuntimeError::NodeDownloadFailed {
            version: version.to_string(),
            details: err.to_string(),
        })?
        .error_for_status()
        .map_err(|err| RuntimeError::NodeDownloadFailed {
            version: version.to_string(),
            details: err.to_string(),
        })?
        .bytes()
        .map_err(|err| RuntimeError::NodeDownloadFailed {
            version: version.to_string(),
            details: err.to_string(),
        })?
        .to_vec();

    let actual = sha256_hex(&archive_bytes);
    if actual != expected.to_lowercase() {
        return Err(RuntimeError::ChecksumMismatch {
            filename,
            expected: expected.to_string(),
            actual,
        });
    }

    if dest.exists() {
        fs::remove_dir_all(dest)?;
    }
    fs::create_dir_all(dest)?;

    let cursor = Cursor::new(archive_bytes);
    let decoder = GzDecoder::new(cursor);
    let mut archive = Archive::new(decoder);
    archive
        .unpack(dest)
        .map_err(|err| RuntimeError::NodeDownloadFailed {
            version: version.to_string(),
            details: err.to_string(),
        })?;

    resolve_node_runtime(version, dest).ok_or_else(|| RuntimeError::NodeDownloadFailed {
        version: version.to_string(),
        details: "node binary not found after extraction".to_string(),
    })
}

fn node_download_filename(version: &str, platform: &Platform) -> Result<String, RuntimeError> {
    let slug = node_platform_slug(platform)?;
    Ok(format!("node-v{version}-{slug}.tar.gz"))
}

fn node_download_url(version: &str, platform: &Platform) -> Result<String, RuntimeError> {
    let filename = node_download_filename(version, platform)?;
    Ok(format!(
        "https://nodejs.org/dist/v{version}/{filename}"
    ))
}

fn node_platform_slug(platform: &Platform) -> Result<&'static str, RuntimeError> {
    match (&platform.os, &platform.arch) {
        (Os::Linux, Arch::X86_64) => Ok("linux-x64"),
        (Os::Linux, Arch::Aarch64) => Ok("linux-arm64"),
        (Os::MacOs, Arch::X86_64) => Ok("darwin-x64"),
        (Os::MacOs, Arch::Aarch64) => Ok("darwin-arm64"),
        (Os::Windows, Arch::X86_64) => Ok("win-x64"),
        (Os::Windows, Arch::Aarch64) => Ok("win-arm64"),
        _ => Err(RuntimeError::UnsupportedPlatform {
            os: platform.os.as_str().to_string(),
            arch: platform.arch.as_str().to_string(),
        }),
    }
}

fn parse_shasums(content: &str) -> Result<HashMap<String, String>, RuntimeError> {
    let mut checksums = HashMap::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if let Some((algo, rest)) = line.split_once('(') {
            if algo.trim().eq_ignore_ascii_case("sha256") {
                if let Some((file_part, hash_part)) = rest.split_once(')') {
                    if let Some((_, hash)) = hash_part.split_once('=') {
                        let filename = file_part.trim().trim_start_matches("./");
                        let hash = hash.trim();
                        if !filename.is_empty() && !hash.is_empty() {
                            checksums.insert(filename.to_string(), hash.to_lowercase());
                            continue;
                        }
                    }
                }
            }
        }

        let mut parts = line.split_whitespace();
        let hash = match parts.next() {
            Some(value) => value,
            None => continue,
        };
        let filename = match parts.next() {
            Some(value) => value,
            None => continue,
        };

        let filename = filename.trim_start_matches('*').trim_start_matches("./");
        if !filename.is_empty() {
            checksums.insert(filename.to_string(), hash.to_lowercase());
        }
    }

    if checksums.is_empty() {
        return Err(RuntimeError::ShasumParse("no checksums found".to_string()));
    }

    Ok(checksums)
}

pub(crate) fn resolve_node_runtime(version: &str, dest: &Path) -> Option<NodeRuntime> {
    let direct_node = dest.join("bin").join("node");
    let direct_npm = dest.join("bin").join("npm");
    if direct_node.exists() && direct_npm.exists() {
        return Some(NodeRuntime {
            version: version.to_string(),
            node_path: direct_node,
            npm_path: direct_npm,
        });
    }

    if let Ok(entries) = fs::read_dir(dest) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let node_path = path.join("bin").join("node");
                let npm_path = path.join("bin").join("npm");
                if node_path.exists() && npm_path.exists() {
                    return Some(NodeRuntime {
                        version: version.to_string(),
                        node_path,
                        npm_path,
                    });
                }
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_version_index() {
        let json = r#"[
            {"version": "v20.10.0", "lts": "Iron", "date": "2023-11-21"},
            {"version": "v21.2.0", "lts": false, "date": "2023-11-22"}
        ]"#;

        let index = NodeVersionIndex::parse_json(json).expect("parse");
        assert_eq!(index.versions.len(), 2);
        assert_eq!(index.versions[0].version, "20.10.0");
        assert_eq!(index.versions[0].lts.as_deref(), Some("Iron"));
        assert_eq!(index.versions[1].lts, None);
    }

    #[test]
    fn test_find_compatible_lts() {
        let index = NodeVersionIndex {
            versions: vec![
                NodeVersionInfo {
                    version: "21.2.0".to_string(),
                    lts: None,
                    date: "2023-11-22".to_string(),
                },
                NodeVersionInfo {
                    version: "20.10.0".to_string(),
                    lts: Some("Iron".to_string()),
                    date: "2023-11-21".to_string(),
                },
                NodeVersionInfo {
                    version: "20.9.0".to_string(),
                    lts: None,
                    date: "2023-11-15".to_string(),
                },
            ],
        };

        let info = index.find_compatible("^20").expect("match");
        assert_eq!(info.version, "20.10.0");
        assert!(info.lts.is_some());
    }

    #[test]
    fn test_parse_shasum() {
        let content = "\
# comment\n\
0d4a1185d1a6b9e9b8f5c773c9f1af0f3f0b0b8e6f2d22b7031a2c7c8e6b9a01  node.tar.gz\n\
SHA256 (node.zip) = 6e340b9cffb37a989ca544e6bb780a2c78901d3fb33738768511a30617afa01d\n\
6e340b9cffb37a989ca544e6bb780a2c78901d3fb33738768511a30617afa01d *./node.bin\n\
";

        let map = parse_shasums(content).expect("parse");
        assert_eq!(
            map.get("node.tar.gz"),
            Some(&"0d4a1185d1a6b9e9b8f5c773c9f1af0f3f0b0b8e6f2d22b7031a2c7c8e6b9a01".to_string())
        );
        assert_eq!(
            map.get("node.zip"),
            Some(&"6e340b9cffb37a989ca544e6bb780a2c78901d3fb33738768511a30617afa01d".to_string())
        );
        assert_eq!(
            map.get("node.bin"),
            Some(&"6e340b9cffb37a989ca544e6bb780a2c78901d3fb33738768511a30617afa01d".to_string())
        );
    }

    #[test]
    fn test_node_download_url() {
        let platform = Platform {
            os: Os::Linux,
            arch: Arch::X86_64,
        };

        let url = node_download_url("20.10.0", &platform).expect("url");
        assert_eq!(
            url,
            "https://nodejs.org/dist/v20.10.0/node-v20.10.0-linux-x64.tar.gz"
        );
    }
}
