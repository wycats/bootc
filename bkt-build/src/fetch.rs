use anyhow::{Context, Result, anyhow, bail};
use bkt_common::archive::{self, ArchiveType, detect_archive_type};
use bkt_common::checksum::sha256_hex;
use bkt_common::http::download;
use bkt_common::manifest::{InstallConfig, UpstreamManifest};
use std::path::Path;

pub fn run(name: &str, manifest_path: &Path) -> Result<()> {
    let manifest = UpstreamManifest::load_from(manifest_path)
        .context("failed to load upstream manifest")?;

    let upstream = manifest
        .find(name)
        .ok_or_else(|| anyhow!("upstream '{}' not found in manifest", name))?;

    match &upstream.install {
        Some(InstallConfig::Script { .. }) => {
            bail!("script installs not handled by bkt-build fetch (use fragment)")
        }
        None => bail!("no install config for upstream '{}'", name),
        _ => {}
    }

    let url = upstream
        .pinned
        .url
        .as_ref()
        .ok_or_else(|| anyhow!("no pinned.url for '{}' — run bkt upstream pin first", name))?;

    eprintln!("Downloading {} from {}", name, url);
    let data = download(url).with_context(|| format!("failed to download {}", name))?;

    eprintln!("Verifying SHA256...");
    let actual = sha256_hex(&data);
    if actual != upstream.pinned.sha256 {
        bail!(
            "SHA256 mismatch for {}: expected {}, got {}",
            name,
            upstream.pinned.sha256,
            actual
        );
    }

    match upstream.install.as_ref().unwrap() {
        InstallConfig::Binary { install_path } => {
            install_binary(&data, url, install_path)?;
            eprintln!("Installed {} to {}", name, install_path);
        }
        InstallConfig::Archive {
            extract_to,
            strip_components,
        } => {
            install_archive(&data, url, extract_to, *strip_components)?;
            eprintln!("Extracted {} to {}", name, extract_to);
        }
        InstallConfig::Script { .. } => unreachable!(),
    }

    Ok(())
}

fn install_binary(data: &[u8], url: &str, install_path: &str) -> Result<()> {
    let path = Path::new(install_path);
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    if !parent.as_os_str().is_empty() {
        std::fs::create_dir_all(parent)?;
    }

    let archive_type = detect_archive_type(url);
    match archive_type {
        ArchiveType::TarGz => {
            let binary_name = path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| anyhow!("invalid install_path"))?;
            let extracted = archive::extract_tar_gz_binary(data, parent, binary_name)?;
            if extracted != path {
                std::fs::rename(&extracted, path)?;
            }
        }
        ArchiveType::Zip => {
            let binary_name = path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| anyhow!("invalid install_path"))?;
            let extracted = archive::extract_zip_binary(data, parent, binary_name)?;
            if extracted != path {
                std::fs::rename(&extracted, path)?;
            }
        }
        ArchiveType::TarXz => bail!("tar.xz not expected for binary install"),
        ArchiveType::Raw => {
            std::fs::write(path, data)?;
        }
    }
    archive::set_executable(path)?;
    Ok(())
}

fn install_archive(data: &[u8], url: &str, extract_to: &str, strip_components: u32) -> Result<()> {
    let target = Path::new(extract_to);
    std::fs::create_dir_all(target)?;

    let archive_type = detect_archive_type(url);
    match archive_type {
        ArchiveType::TarGz => archive::extract_tar_gz(data, target, strip_components)?,
        ArchiveType::TarXz => archive::extract_tar_xz(data, target, strip_components)?,
        ArchiveType::Zip => {
            if strip_components != 0 {
                bail!(
                    "strip_components is not supported for ZIP archives (got {})",
                    strip_components
                );
            }
            archive::extract_zip(data, target)?
        }
        ArchiveType::Raw => bail!("raw file not expected for archive install"),
    }
    Ok(())
}
