use sha2::{Digest, Sha256};
use std::collections::HashMap;

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

pub fn parse_checksum_file(content: &str) -> HashMap<String, String> {
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

    checksums
}
