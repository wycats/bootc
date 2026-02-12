use crate::error::CommonError;
use flate2::read::GzDecoder;
use lzma_rs::xz_decompress;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use tar::Archive;
use zip::ZipArchive;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveType {
    TarGz,
    TarXz,
    Zip,
    Raw,
}

pub fn detect_archive_type(name: &str) -> ArchiveType {
    let lower = name.to_lowercase();
    if lower.ends_with(".tar.gz") || lower.ends_with(".tgz") {
        ArchiveType::TarGz
    } else if lower.ends_with(".tar.xz") || lower.ends_with(".txz") {
        ArchiveType::TarXz
    } else if lower.ends_with(".zip") {
        ArchiveType::Zip
    } else {
        ArchiveType::Raw
    }
}

pub fn extract_tar_gz_binary(
    data: &[u8],
    target_dir: &Path,
    binary_name: &str,
) -> Result<PathBuf, CommonError> {
    fs::create_dir_all(target_dir)?;
    let cursor = Cursor::new(data);
    let decoder = GzDecoder::new(cursor);
    let mut archive = Archive::new(decoder);
    let extracted = extract_tar_entries(&mut archive, target_dir, 0)?;
    select_binary(&extracted, binary_name)
}

pub fn extract_tar_gz(
    data: &[u8],
    target_dir: &Path,
    strip_components: u32,
) -> Result<(), CommonError> {
    fs::create_dir_all(target_dir)?;
    let cursor = Cursor::new(data);
    let decoder = GzDecoder::new(cursor);
    let mut archive = Archive::new(decoder);
    extract_tar_entries(&mut archive, target_dir, strip_components)?;
    Ok(())
}

pub fn extract_tar_xz(
    data: &[u8],
    target_dir: &Path,
    strip_components: u32,
) -> Result<(), CommonError> {
    fs::create_dir_all(target_dir)?;
    let mut decompressed = Vec::new();
    xz_decompress(&mut Cursor::new(data), &mut decompressed)
        .map_err(|err| CommonError::Archive(err.to_string()))?;
    let mut archive = Archive::new(Cursor::new(&decompressed));
    extract_tar_entries(&mut archive, target_dir, strip_components)?;
    Ok(())
}

pub fn extract_zip_binary(
    data: &[u8],
    target_dir: &Path,
    binary_name: &str,
) -> Result<PathBuf, CommonError> {
    fs::create_dir_all(target_dir)?;
    let cursor = Cursor::new(data);
    let mut archive =
        ZipArchive::new(cursor).map_err(|err| CommonError::Archive(err.to_string()))?;
    let mut extracted = Vec::new();

    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .map_err(|err| CommonError::Archive(err.to_string()))?;
        let Some(enclosed) = file.enclosed_name() else {
            continue;
        };
        let out_path = target_dir.join(enclosed);

        if file.name().ends_with('/') {
            fs::create_dir_all(&out_path)?;
            continue;
        }

        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut outfile = fs::File::create(&out_path)?;
        std::io::copy(&mut file, &mut outfile)?;
        extracted.push(out_path);
    }

    select_binary(&extracted, binary_name)
}

pub fn extract_zip(data: &[u8], target_dir: &Path) -> Result<(), CommonError> {
    fs::create_dir_all(target_dir)?;
    let cursor = Cursor::new(data);
    let mut archive =
        ZipArchive::new(cursor).map_err(|err| CommonError::Archive(err.to_string()))?;

    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .map_err(|err| CommonError::Archive(err.to_string()))?;
        let Some(enclosed) = file.enclosed_name() else {
            continue;
        };
        let out_path = target_dir.join(enclosed);

        if file.name().ends_with('/') {
            fs::create_dir_all(&out_path)?;
            continue;
        }

        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut outfile = fs::File::create(&out_path)?;
        std::io::copy(&mut file, &mut outfile)?;
    }

    Ok(())
}

pub fn write_raw(
    data: &[u8],
    target_dir: &Path,
    binary_name: &str,
) -> Result<PathBuf, CommonError> {
    fs::create_dir_all(target_dir)?;
    let output = target_dir.join(binary_name);
    fs::write(&output, data)?;
    Ok(output)
}

pub fn set_executable(path: &Path) -> Result<(), CommonError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms)?;
    }

    Ok(())
}

fn extract_tar_entries<R: std::io::Read>(
    archive: &mut Archive<R>,
    target_dir: &Path,
    strip_components: u32,
) -> Result<Vec<PathBuf>, CommonError> {
    let mut extracted = Vec::new();
    let strip = strip_components as usize;

    for entry in archive
        .entries()
        .map_err(|err| CommonError::Archive(err.to_string()))?
    {
        let mut entry = entry.map_err(|err| CommonError::Archive(err.to_string()))?;
        let entry_path = entry
            .path()
            .map_err(|err| CommonError::Archive(err.to_string()))?
            .to_path_buf();
        let Some(stripped) = strip_path(&entry_path, strip) else {
            continue;
        };
        let out_path = target_dir.join(stripped);

        if entry.header().entry_type().is_dir() {
            fs::create_dir_all(&out_path)?;
            continue;
        }

        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)?;
        }
        entry
            .unpack(&out_path)
            .map_err(|err| CommonError::Archive(err.to_string()))?;

        if entry.header().entry_type().is_file() {
            extracted.push(out_path);
        }
    }

    Ok(extracted)
}

fn strip_path(path: &Path, strip_components: usize) -> Option<PathBuf> {
    if strip_components == 0 {
        return Some(path.to_path_buf());
    }

    let stripped: PathBuf = path.components().skip(strip_components).collect();
    if stripped.as_os_str().is_empty() {
        None
    } else {
        Some(stripped)
    }
}

fn select_binary(paths: &[PathBuf], binary_name: &str) -> Result<PathBuf, CommonError> {
    let matching: Vec<&PathBuf> = paths
        .iter()
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(|name| name == binary_name || name == format!("{binary_name}.exe"))
                .unwrap_or(false)
        })
        .collect();

    if matching.len() == 1 {
        return Ok(matching[0].clone());
    }

    if matching.is_empty() {
        if paths.len() == 1 {
            return Ok(paths[0].clone());
        }

        return Err(CommonError::Archive(format!(
            "archive contains {} files but none match expected binary name '{binary_name}'",
            paths.len()
        )));
    }

    Err(CommonError::Archive(format!(
        "multiple binaries matched for {binary_name}"
    )))
}
