use rpmcheck::{expand_repo_url, Manifest};

#[test]
fn manifest_repo_urls_are_reachable() {
    if std::env::var("RPMCHECK_LIVE").is_err() {
        eprintln!("skipping live URL test (set RPMCHECK_LIVE=1 to enable)");
        return;
    }

    let manifest_path =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../manifests/external-repos.json");

    let content = std::fs::read_to_string(&manifest_path)
        .unwrap_or_else(|e| panic!("reading {}: {e}", manifest_path.display()));

    let manifest: Manifest =
        serde_json::from_str(&content).expect("parsing external-repos.json");

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .expect("building HTTP client");

    for repo in &manifest.repos {
        let url = expand_repo_url(&repo.baseurl);
        let repomd_url = format!("{}/repodata/repomd.xml", url.trim_end_matches('/'));

        eprintln!("  {}: HEAD {repomd_url}", repo.name);

        // Try HEAD first; fall back to GET if the server rejects it
        // (some repos block HEAD while allowing GET).
        let resp = client.head(&repomd_url).send();
        let resp = match resp {
            Ok(r) if r.status().is_success() => r,
            _ => {
                eprintln!("  {}: HEAD failed, retrying with GET", repo.name);
                client
                    .get(&repomd_url)
                    .send()
                    .unwrap_or_else(|e| panic!("repo '{}': request failed: {e}", repo.name))
            }
        };

        assert!(
            resp.status().is_success(),
            "repo '{}': expected 200, got {} for {repomd_url}",
            repo.name,
            resp.status()
        );

        eprintln!("  {} ✓", repo.name);
    }
}
