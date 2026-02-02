use fetchbin::source::{PackageSpec, SourceConfig};
use fetchbin::{BinarySource, GithubSource};

#[test]
#[ignore]
fn test_resolve_lazygit_releases() {
    let source = GithubSource::new();
    let spec = PackageSpec {
        name: "lazygit".to_string(),
        version_req: None,
        source: SourceConfig::Github {
            repo: "jesseduffield/lazygit".to_string(),
            asset_pattern: None,
        },
        binary_name: None,
    };

    let releases = source.resolve(&spec).expect("resolve releases");
    assert!(!releases.is_empty());
}
