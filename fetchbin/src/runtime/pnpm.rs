use crate::error::RuntimeError;
use crate::platform::{Arch, Os, Platform};
use crate::source::github::checksum::{parse_checksum_file, sha256_hex};
use reqwest::blocking::Client;
use reqwest::header::USER_AGENT;
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct PnpmRuntime {
    pub version: String,
    pub pnpm_path: PathBuf,
}

#[derive(Debug, Deserialize)]
struct PnpmRelease {
    tag_name: String,
}

pub fn fetch_latest_pnpm_version(client: &Client) -> Result<String, RuntimeError> {
    fetch_latest_pnpm_version_with_base(client, "https://api.github.com")
}

fn fetch_latest_pnpm_version_with_base(
    client: &Client,
    base_url: &str,
) -> Result<String, RuntimeError> {
    let url = format!("{base_url}/repos/pnpm/pnpm/releases/latest");
    let response = client
        .get(url)
        .header(USER_AGENT, "fetchbin")
        .send()
        .map_err(|err| RuntimeError::PnpmDownloadFailed {
            version: "latest".to_string(),
            details: err.to_string(),
        })?
        .error_for_status()
        .map_err(|err| RuntimeError::PnpmDownloadFailed {
            version: "latest".to_string(),
            details: err.to_string(),
        })?;

    let release = response
        .json::<PnpmRelease>()
        .map_err(|err| RuntimeError::PnpmDownloadFailed {
            version: "latest".to_string(),
            details: err.to_string(),
        })?;

    Ok(release.tag_name.trim_start_matches('v').to_string())
}

pub fn download_pnpm(
    client: &Client,
    version: &str,
    dest: &Path,
    platform: &Platform,
) -> Result<PnpmRuntime, RuntimeError> {
    download_pnpm_with_base(client, version, dest, platform, "https://github.com")
}

fn download_pnpm_with_base(
    client: &Client,
    version: &str,
    dest: &Path,
    platform: &Platform,
    base_url: &str,
) -> Result<PnpmRuntime, RuntimeError> {
    let normalized = version.trim_start_matches('v');
    let asset = pnpm_asset_name(platform)?;
    let tag = format!("v{normalized}");
    let url = format!("{base_url}/pnpm/pnpm/releases/download/{tag}/{asset}");

    let bytes = client
        .get(url)
        .send()
        .map_err(|err| RuntimeError::PnpmDownloadFailed {
            version: normalized.to_string(),
            details: err.to_string(),
        })?
        .error_for_status()
        .map_err(|err| RuntimeError::PnpmDownloadFailed {
            version: normalized.to_string(),
            details: err.to_string(),
        })?
        .bytes()
        .map_err(|err| RuntimeError::PnpmDownloadFailed {
            version: normalized.to_string(),
            details: err.to_string(),
        })?
        .to_vec();

    let checksum_url = format!("{base_url}/pnpm/pnpm/releases/download/{tag}/{asset}.sha256");
    let checksum_bytes = client
        .get(checksum_url)
        .send()
        .map_err(|err| RuntimeError::PnpmDownloadFailed {
            version: normalized.to_string(),
            details: err.to_string(),
        })?
        .error_for_status()
        .map_err(|err| RuntimeError::PnpmDownloadFailed {
            version: normalized.to_string(),
            details: err.to_string(),
        })?
        .bytes()
        .map_err(|err| RuntimeError::PnpmDownloadFailed {
            version: normalized.to_string(),
            details: err.to_string(),
        })?
        .to_vec();

    let checksum_text = String::from_utf8_lossy(&checksum_bytes);
    verify_pnpm_checksum(asset, &checksum_text, &bytes)?;

    if dest.exists() {
        fs::remove_dir_all(dest)?;
    }
    fs::create_dir_all(dest)?;

    let pnpm_path = dest.join("pnpm");
    fs::write(&pnpm_path, bytes)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&pnpm_path)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&pnpm_path, permissions)?;
    }

    Ok(PnpmRuntime {
        version: normalized.to_string(),
        pnpm_path,
    })
}

fn verify_pnpm_checksum(
    asset_name: &str,
    checksum_text: &str,
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
        .cloned()
        .or_else(|| parse_single_hash(checksum_text));

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

fn parse_single_hash(content: &str) -> Option<String> {
    content.lines().find_map(|line| {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            return None;
        }

        let hash = line.split_whitespace().next()?;
        if hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit()) {
            Some(hash.to_lowercase())
        } else {
            None
        }
    })
}

pub(crate) fn pnpm_asset_name(platform: &Platform) -> Result<&'static str, RuntimeError> {
    match (&platform.os, &platform.arch) {
        (Os::Linux, Arch::X86_64) => Ok("pnpm-linux-x64"),
        (Os::Linux, Arch::Aarch64) => Ok("pnpm-linux-arm64"),
        (Os::MacOs, Arch::X86_64) => Ok("pnpm-macos-x64"),
        (Os::MacOs, Arch::Aarch64) => Ok("pnpm-macos-arm64"),
        (Os::Windows, Arch::X86_64) => Ok("pnpm-win-x64.exe"),
        (Os::Windows, Arch::Aarch64) => Ok("pnpm-win-arm64.exe"),
        _ => Err(RuntimeError::UnsupportedPlatform {
            os: platform.os.as_str().to_string(),
            arch: platform.arch.as_str().to_string(),
        }),
    }
}

pub(crate) fn resolve_pnpm_runtime(version: &str, dest: &Path) -> Option<PnpmRuntime> {
    let pnpm_path = dest.join("pnpm");
    if pnpm_path.exists() {
        return Some(PnpmRuntime {
            version: version.to_string(),
            pnpm_path,
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Server;

    #[test]
    fn test_pnpm_asset_name() {
        let platform = Platform {
            os: Os::Linux,
            arch: Arch::X86_64,
        };
        assert_eq!(pnpm_asset_name(&platform).expect("asset"), "pnpm-linux-x64");

        let platform = Platform {
            os: Os::Linux,
            arch: Arch::Aarch64,
        };
        assert_eq!(pnpm_asset_name(&platform).expect("asset"), "pnpm-linux-arm64");

        let platform = Platform {
            os: Os::MacOs,
            arch: Arch::X86_64,
        };
        assert_eq!(pnpm_asset_name(&platform).expect("asset"), "pnpm-macos-x64");

        let platform = Platform {
            os: Os::MacOs,
            arch: Arch::Aarch64,
        };
        assert_eq!(pnpm_asset_name(&platform).expect("asset"), "pnpm-macos-arm64");

        let platform = Platform {
            os: Os::Windows,
            arch: Arch::X86_64,
        };
        assert_eq!(pnpm_asset_name(&platform).expect("asset"), "pnpm-win-x64.exe");

        let platform = Platform {
            os: Os::Windows,
            arch: Arch::Aarch64,
        };
        assert_eq!(pnpm_asset_name(&platform).expect("asset"), "pnpm-win-arm64.exe");
    }

    #[test]
    fn test_fetch_latest_pnpm_version() {
        let mut server = Server::new();
        let mock = server
            .mock("GET", "/repos/pnpm/pnpm/releases/latest")
            .match_header("user-agent", "fetchbin")
            .with_status(200)
            .with_body(r#"{"tag_name":"v9.1.0"}"#)
            .create();

        let client = Client::new();
        let version =
            fetch_latest_pnpm_version_with_base(&client, &server.url()).expect("version");

        mock.assert();
        assert_eq!(version, "9.1.0");
    }

    #[test]
    fn test_verify_pnpm_checksum() {
        let bytes = b"hello";
        let hash = sha256_hex(bytes);
        let checksum_text = format!("{hash}  pnpm-linux-x64\n");

        verify_pnpm_checksum("pnpm-linux-x64", &checksum_text, bytes)
            .expect("checksum ok");
    }

    #[test]
    fn test_verify_pnpm_checksum_mismatch() {
        let bytes = b"hello";
        let checksum_text = "deadbeef  pnpm-linux-x64\n";

        let err = verify_pnpm_checksum("pnpm-linux-x64", checksum_text, bytes)
            .expect_err("checksum error");
        assert!(matches!(err, RuntimeError::ChecksumMismatch { .. }));
    }
}
