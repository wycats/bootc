use crate::error::CommonError;
use std::io::Read;

pub fn download(url: &str) -> Result<Vec<u8>, CommonError> {
    let mut bytes = Vec::new();
    ureq::get(url)
        .call()
        .map_err(|e| CommonError::Http(e.to_string()))?
        .body_mut()
        .as_reader()
        .read_to_end(&mut bytes)
        .map_err(|e| CommonError::Http(e.to_string()))?;
    Ok(bytes)
}
