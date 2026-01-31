use crate::error::FetchError;
use flate2::read::GzDecoder;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use tar::Archive;
use zip::ZipArchive;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveType {
    TarGz,
    Zip,
    Raw,
}

pub fn detect_archive_type(name: &str) -> ArchiveType {
    let lower = name.to_lowercase();
    if lower.ends_with(".tar.gz") || lower.ends_with(".tgz") {
        ArchiveType::TarGz
    } else if lower.ends_with(".zip") {
        ArchiveType::Zip
    } else {
        ArchiveType::Raw
    }
}

pub fn extract_tar_gz(
    data: &[u8],
    target_dir: &Path,
    binary_name: &str,
) -> Result<PathBuf, FetchError> {
    fs::create_dir_all(target_dir)?;
    let cursor = Cursor::new(data);
    let decoder = GzDecoder::new(cursor);
    let mut archive = Archive::new(decoder);
    let mut extracted = Vec::new();

    for entry in archive
        .entries()
        .map_err(|err| FetchError::Parse(err.to_string()))?
    {
        let mut entry = entry.map_err(|err| FetchError::Parse(err.to_string()))?;
        let entry_path = entry
            .path()
            .map_err(|err| FetchError::Parse(err.to_string()))?
            .to_path_buf();
        entry
            .unpack_in(target_dir)
            .map_err(|err| FetchError::Parse(err.to_string()))?;

        if entry.header().entry_type().is_file() {
            extracted.push(target_dir.join(entry_path));
        }
    }

    select_binary(&extracted, binary_name)
}

pub fn extract_zip(
    data: &[u8],
    target_dir: &Path,
    binary_name: &str,
) -> Result<PathBuf, FetchError> {
    fs::create_dir_all(target_dir)?;
    let cursor = Cursor::new(data);
    let mut archive = ZipArchive::new(cursor).map_err(|err| FetchError::Parse(err.to_string()))?;
    let mut extracted = Vec::new();

    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .map_err(|err| FetchError::Parse(err.to_string()))?;
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

pub fn write_raw(data: &[u8], target_dir: &Path, binary_name: &str) -> Result<PathBuf, FetchError> {
    fs::create_dir_all(target_dir)?;
    let output = target_dir.join(binary_name);
    fs::write(&output, data)?;
    Ok(output)
}

fn select_binary(paths: &[PathBuf], binary_name: &str) -> Result<PathBuf, FetchError> {
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

        return Err(FetchError::Parse(format!(
            "archive contains {} files but none match expected binary name '{binary_name}'",
            paths.len()
        )));
    }

    Err(FetchError::Parse(format!(
        "multiple binaries matched for {binary_name}"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_archive_type() {
        assert_eq!(detect_archive_type("tool.tar.gz"), ArchiveType::TarGz);
        assert_eq!(detect_archive_type("tool.zip"), ArchiveType::Zip);
        assert_eq!(detect_archive_type("tool"), ArchiveType::Raw);
    }
}
