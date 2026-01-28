use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Release {
    pub tag_name: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub assets: Vec<Asset>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Asset {
    pub name: String,
    #[serde(default)]
    pub browser_download_url: String,
    #[serde(default)]
    pub size: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_release() {
        let json = r#"
        {
            "tag_name": "v1.2.3",
            "name": "Release 1.2.3",
            "prerelease": false,
            "draft": false,
            "assets": [
                {
                    "name": "tool-linux-x64.tar.gz",
                    "browser_download_url": "https://example.com/tool.tar.gz",
                    "size": 1234
                }
            ]
        }
        "#;

        let release: Release = serde_json::from_str(json).expect("deserialize");
        assert_eq!(release.tag_name, "v1.2.3");
        assert_eq!(release.name.as_deref(), Some("Release 1.2.3"));
        assert!(!release.prerelease);
        assert!(!release.draft);
        assert_eq!(release.assets.len(), 1);
        assert_eq!(release.assets[0].name, "tool-linux-x64.tar.gz");
        assert_eq!(
            release.assets[0].browser_download_url,
            "https://example.com/tool.tar.gz"
        );
        assert_eq!(release.assets[0].size, 1234);
    }

    #[test]
    fn test_parse_release_response() {
        let json = r#"
        [
            {
                "tag_name": "v0.1.0",
                "name": "Release 0.1.0",
                "prerelease": false,
                "draft": false,
                "assets": []
            },
            {
                "tag_name": "v0.2.0",
                "name": "Release 0.2.0",
                "prerelease": false,
                "draft": false,
                "assets": [
                    {
                        "name": "tool-linux-x64.tar.gz",
                        "browser_download_url": "https://example.com/tool-0.2.0.tar.gz",
                        "size": 2048
                    }
                ]
            }
        ]
        "#;

        let releases: Vec<Release> = serde_json::from_str(json).expect("deserialize");
        assert_eq!(releases.len(), 2);
        assert_eq!(releases[0].tag_name, "v0.1.0");
        assert_eq!(releases[1].tag_name, "v0.2.0");
        assert_eq!(releases[1].assets.len(), 1);
        assert_eq!(releases[1].assets[0].name, "tool-linux-x64.tar.gz");
    }
}
