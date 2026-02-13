use crate::error::CommonError;
use serde::de::DeserializeOwned;
use std::io::Read;

/// Download raw bytes from a URL.
pub fn download(url: &str) -> Result<Vec<u8>, CommonError> {
    download_with_headers(url, &[])
}

/// Download raw bytes from a URL with custom headers.
pub fn download_with_headers(
    url: &str,
    headers: &[(&str, &str)],
) -> Result<Vec<u8>, CommonError> {
    let mut request = ureq::get(url);
    for &(key, value) in headers {
        request = request.header(key, value);
    }
    let mut bytes = Vec::new();
    request
        .call()
        .map_err(|e| CommonError::Http(e.to_string()))?
        .body_mut()
        .as_reader()
        .read_to_end(&mut bytes)
        .map_err(|e| CommonError::Http(e.to_string()))?;
    Ok(bytes)
}

/// Download and deserialize JSON from a URL with custom headers.
pub fn download_json<T: DeserializeOwned>(
    url: &str,
    headers: &[(&str, &str)],
) -> Result<T, CommonError> {
    let bytes = download_with_headers(url, headers)?;
    serde_json::from_slice(&bytes).map_err(CommonError::Json)
}

/// Download text content from a URL.
pub fn download_text(url: &str) -> Result<String, CommonError> {
    download_text_with_headers(url, &[])
}

/// Download text content from a URL with custom headers.
pub fn download_text_with_headers(
    url: &str,
    headers: &[(&str, &str)],
) -> Result<String, CommonError> {
    let bytes = download_with_headers(url, headers)?;
    String::from_utf8(bytes).map_err(|e| CommonError::Http(format!("invalid UTF-8: {e}")))
}
