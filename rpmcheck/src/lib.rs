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

/// Expand DNF-style variables in a URL (e.g. `$basearch`).
pub fn expand_repo_url(url: &str) -> String {
    let basearch = match std::env::consts::ARCH {
        "x86_64" => "x86_64",
        "aarch64" => "aarch64",
        "arm" => "armhfp",
        "powerpc64" => "ppc64le",
        "s390x" => "s390x",
        other => other,
    };
    url.replace("$basearch", basearch)
}
