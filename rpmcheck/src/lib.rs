use serde::Deserialize;

// ---------------------------------------------------------------------------
// Manifest types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct Manifest {
    pub repos: Vec<RepoEntry>,
}

#[derive(Deserialize)]
pub struct RepoEntry {
    pub name: String,
    pub baseurl: String,
    pub packages: Vec<String>,
}

// ---------------------------------------------------------------------------
// URL expansion
// ---------------------------------------------------------------------------

/// Expand DNF-style variables in a URL (e.g. `$basearch` or `${basearch}`).
pub fn expand_repo_url(url: &str) -> String {
    let basearch = match std::env::consts::ARCH {
        "x86_64" => "x86_64",
        "aarch64" => "aarch64",
        "arm" => "armhfp",
        "powerpc64" => "ppc64le",
        "s390x" => "s390x",
        other => other,
    };
    url.replace("${basearch}", basearch)
        .replace("$basearch", basearch)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_bare_basearch() {
        let url = "https://example.com/rpm/stable/$basearch";
        let expanded = expand_repo_url(url);
        assert!(!expanded.contains('$'), "unexpanded variable in: {expanded}");
        assert!(expanded.ends_with(std::env::consts::ARCH) || expanded.ends_with("x86_64"));
    }

    #[test]
    fn expand_braced_basearch() {
        let url = "https://example.com/rpm/stable/${basearch}";
        let expanded = expand_repo_url(url);
        assert!(!expanded.contains('$'), "unexpanded variable in: {expanded}");
    }

    #[test]
    fn no_variables_unchanged() {
        let url = "https://example.com/rpm/stable/x86_64";
        assert_eq!(expand_repo_url(url), url);
    }
}
