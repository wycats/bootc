use crate::source::github::api::{Asset, Release};

pub use bkt_common::checksum::{parse_checksum_file, sha256_hex};

pub fn find_checksum_asset<'a>(release: &'a Release, asset: &Asset) -> Option<&'a Asset> {
    let preferred = format!("{}.sha256", asset.name);
    if let Some(asset) = release.assets.iter().find(|item| item.name == preferred) {
        return Some(asset);
    }

    let fallback = ["checksums.txt", "SHASUMS256.txt", "SHA256SUMS"];
    fallback
        .iter()
        .find_map(|name| release.assets.iter().find(|item| item.name == *name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_checksum_file() {
        let content = "\
# comment\n\
0d4a1185d1a6b9e9b8f5c773c9f1af0f3f0b0b8e6f2d22b7031a2c7c8e6b9a01  tool.tar.gz\n\
SHA256 (tool.zip) = 6e340b9cffb37a989ca544e6bb780a2c78901d3fb33738768511a30617afa01d\n\
6e340b9cffb37a989ca544e6bb780a2c78901d3fb33738768511a30617afa01d *./tool.bin\n\
";

        let map = parse_checksum_file(content);
        assert_eq!(
            map.get("tool.tar.gz"),
            Some(&"0d4a1185d1a6b9e9b8f5c773c9f1af0f3f0b0b8e6f2d22b7031a2c7c8e6b9a01".to_string())
        );
        assert_eq!(
            map.get("tool.zip"),
            Some(&"6e340b9cffb37a989ca544e6bb780a2c78901d3fb33738768511a30617afa01d".to_string())
        );
        assert_eq!(
            map.get("tool.bin"),
            Some(&"6e340b9cffb37a989ca544e6bb780a2c78901d3fb33738768511a30617afa01d".to_string())
        );
    }

    #[test]
    fn test_sha256_hex() {
        let hash = sha256_hex(b"hello");
        assert_eq!(
            hash,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn test_checksum_parsing() {
        let content = "SHA256 (tool.tar.gz) = 6e340b9cffb37a989ca544e6bb780a2c78901d3fb33738768511a30617afa01d";
        let map = parse_checksum_file(content);
        assert_eq!(
            map.get("tool.tar.gz"),
            Some(&"6e340b9cffb37a989ca544e6bb780a2c78901d3fb33738768511a30617afa01d".to_string())
        );
    }
}
