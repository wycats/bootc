use std::collections::{BTreeMap, HashSet};
use std::io::Read;

use anyhow::{bail, Context, Result};
use flate2::read::GzDecoder;
use quick_xml::events::Event;
use quick_xml::Reader;
use rpmcheck::{expand_repo_url, Manifest, RepoEntry};
use serde::Serialize;
use sha2::{Digest, Sha256};

// ---------------------------------------------------------------------------
// Package version (from primary.xml)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Ord, PartialOrd, Eq, PartialEq)]
struct PackageVersion {
    name: String,
    epoch: String,
    version: String,
    release: String,
}

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

fn print_usage() {
    eprintln!("Usage: rpmcheck <manifest.json> [--baseline <hash>] [--json]");
    eprintln!();
    eprintln!("Check external RPM repos for package version changes.");
    eprintln!("Outputs a SHA-256 hash of tracked package versions.");
    eprintln!();
    eprintln!("Exit codes:");
    eprintln!("  0  Success (or unchanged when --baseline given)");
    eprintln!("  1  Versions changed from baseline");
    eprintln!("  2  Error");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    let mut manifest_path = None;
    let mut baseline = None;
    let mut json = false;
    let mut i = 1;

    while i < args.len() {
        match args[i].as_str() {
            "--baseline" | "-b" => {
                i += 1;
                baseline = args.get(i).cloned();
            }
            "--json" | "-j" => json = true,
            "-h" | "--help" => {
                print_usage();
                std::process::exit(0);
            }
            arg if !arg.starts_with('-') => {
                manifest_path = Some(arg.to_string());
            }
            other => {
                eprintln!("unknown argument: {other}");
                print_usage();
                std::process::exit(2);
            }
        }
        i += 1;
    }

    let manifest_path = match manifest_path {
        Some(p) => p,
        None => {
            print_usage();
            std::process::exit(2);
        }
    };

    if let Err(e) = run(&manifest_path, baseline.as_deref(), json) {
        eprintln!("error: {e:#}");
        std::process::exit(2);
    }
}

// ---------------------------------------------------------------------------
// Core logic
// ---------------------------------------------------------------------------

fn run(manifest_path: &str, baseline: Option<&str>, json: bool) -> Result<()> {
    let manifest: Manifest = serde_json::from_str(
        &std::fs::read_to_string(manifest_path)
            .with_context(|| format!("reading {manifest_path}"))?,
    )
    .context("parsing manifest JSON")?;

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .context("building HTTP client")?;

    let mut all: BTreeMap<String, Vec<PackageVersion>> = BTreeMap::new();

    for repo in &manifest.repos {
        eprintln!("repo: {} ({})", repo.name, repo.baseurl);
        let tracked: HashSet<&str> = repo.packages.iter().map(|s| s.as_str()).collect();

        let versions = check_repo(&client, repo, &tracked)
            .with_context(|| format!("checking repo '{}'", repo.name))?;

        // Warn about tracked packages not found in repo
        let found_names: HashSet<&str> = versions.iter().map(|p| p.name.as_str()).collect();
        for pkg in &repo.packages {
            if !found_names.contains(pkg.as_str()) {
                eprintln!("  warning: '{}' not found in repo", pkg);
            }
        }

        for pv in versions {
            eprintln!("  {} {}-{}", pv.name, pv.version, pv.release);
            all.entry(pv.name.clone()).or_default().push(pv);
        }
    }

    // Sort deterministically
    for v in all.values_mut() {
        v.sort();
    }

    // Hash tracked package versions
    let mut hasher = Sha256::new();
    for (name, versions) in &all {
        for pv in versions {
            hasher.update(format!(
                "{}\t{}\t{}\t{}\n",
                name, pv.epoch, pv.version, pv.release
            ));
        }
    }
    let hash = format!("{:x}", hasher.finalize());
    let changed = baseline.map(|b| b != hash);

    // Output
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "hash": hash,
                "changed": changed.unwrap_or(false),
                "packages": all,
            }))?
        );
    } else {
        println!("{hash}");
        match changed {
            Some(true) => {
                eprintln!("changed (was: {})", baseline.unwrap());
                std::process::exit(1);
            }
            Some(false) => eprintln!("unchanged"),
            None => {}
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Repo checking
// ---------------------------------------------------------------------------

fn check_repo(
    client: &reqwest::blocking::Client,
    repo: &RepoEntry,
    tracked: &HashSet<&str>,
) -> Result<Vec<PackageVersion>> {
    let baseurl = expand_repo_url(&repo.baseurl);

    // 1. Fetch repomd.xml to discover primary.xml.gz location
    let repomd_url = format!("{}/repodata/repomd.xml", baseurl.trim_end_matches('/'));
    let repomd_body = client
        .get(&repomd_url)
        .send()
        .and_then(|r| r.error_for_status())
        .and_then(|r| r.text())
        .with_context(|| format!("fetching {repomd_url}"))?;

    let primary_href =
        find_primary_href(&repomd_body).context("finding primary.xml.gz in repomd.xml")?;

    // 2. Fetch and decompress primary.xml.gz
    let primary_url = format!("{}/{}", baseurl.trim_end_matches('/'), primary_href);
    eprintln!("  fetching {primary_url}");

    let compressed = client
        .get(&primary_url)
        .send()
        .and_then(|r| r.error_for_status())
        .and_then(|r| r.bytes())
        .with_context(|| format!("fetching {primary_url}"))?;

    let mut xml = String::new();
    GzDecoder::new(&compressed[..])
        .read_to_string(&mut xml)
        .context("decompressing primary.xml.gz")?;

    // 3. Parse for tracked packages
    parse_packages(&xml, tracked).context("parsing primary.xml")
}

// ---------------------------------------------------------------------------
// repomd.xml parser — find the <location href="..."> for type="primary"
// ---------------------------------------------------------------------------

fn find_primary_href(repomd_xml: &str) -> Result<String> {
    let mut reader = Reader::from_str(repomd_xml);
    reader.config_mut().trim_text(true);

    let mut in_primary = false;

    loop {
        match reader.read_event()? {
            Event::Start(ref e) | Event::Empty(ref e) => {
                let local = tag_local(e.name());

                if local == "data" {
                    for attr in e.attributes() {
                        let attr = attr?;
                        if attr.key.as_ref() == b"type" && attr.value.as_ref() == b"primary" {
                            in_primary = true;
                        }
                    }
                }

                if in_primary && local == "location" {
                    for attr in e.attributes() {
                        let attr = attr?;
                        if attr.key.as_ref() == b"href" {
                            return Ok(std::str::from_utf8(&attr.value)?.to_string());
                        }
                    }
                }
            }
            Event::End(ref e) => {
                if tag_local(e.name()) == "data" {
                    in_primary = false;
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }

    bail!("no <data type=\"primary\"> found in repomd.xml")
}

// ---------------------------------------------------------------------------
// primary.xml parser — extract (name, epoch, version, release) for tracked pkgs
// ---------------------------------------------------------------------------

fn parse_packages(xml: &str, tracked: &HashSet<&str>) -> Result<Vec<PackageVersion>> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut results = Vec::new();
    let mut in_package = false;
    let mut current_name = String::new();
    let mut reading_name = false;

    loop {
        match reader.read_event()? {
            Event::Start(ref e) => {
                let local = tag_local(e.name());

                if local == "package" {
                    in_package = true;
                    current_name.clear();
                } else if in_package && local == "name" {
                    reading_name = true;
                }
            }
            Event::Empty(ref e) => {
                let local = tag_local(e.name());

                if in_package && local == "version" && tracked.contains(current_name.as_str()) {
                    let mut epoch = String::from("0");
                    let mut ver = String::new();
                    let mut rel = String::new();

                    for attr in e.attributes() {
                        let attr = attr?;
                        match std::str::from_utf8(attr.key.as_ref())? {
                            "epoch" => epoch = std::str::from_utf8(&attr.value)?.to_string(),
                            "ver" => ver = std::str::from_utf8(&attr.value)?.to_string(),
                            "rel" => rel = std::str::from_utf8(&attr.value)?.to_string(),
                            _ => {}
                        }
                    }

                    results.push(PackageVersion {
                        name: current_name.clone(),
                        epoch,
                        version: ver,
                        release: rel,
                    });
                }
            }
            Event::Text(ref e) => {
                if reading_name {
                    current_name = e.unescape()?.to_string();
                    reading_name = false;
                }
            }
            Event::End(ref e) => {
                if tag_local(e.name()) == "package" {
                    in_package = false;
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }

    Ok(results)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract the local tag name as a String from a quick-xml element.
fn tag_local(name: quick_xml::name::QName<'_>) -> String {
    let raw = std::str::from_utf8(name.as_ref()).unwrap_or("");
    local_name(raw).to_string()
}

/// Strip namespace prefix: "repo:data" → "data", "data" → "data"
fn local_name(tag: &str) -> &str {
    tag.rsplit_once(':').map_or(tag, |(_, local)| local)
}
