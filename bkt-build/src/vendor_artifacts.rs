//! Vendor artifact resolution and installation.
//!
//! Resolves vendor feed URLs to concrete artifact metadata, and installs
//! resolved RPM artifacts during container builds.

use anyhow::{anyhow, bail, Context, Result};
use bkt_common::checksum::sha256_hex;
use bkt_common::http;
use bkt_common::manifest::{
    ResolvedVendorArtifact, ResolvedVendorArtifactsManifest, VendorArtifactsManifest, VendorSource,
};
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

/// Resolve all vendor artifacts from a manifest file.
///
/// Reads the intent manifest, queries each artifact's vendor feed,
/// and writes a resolved manifest to the output path.
pub fn resolve(manifest_path: &Path, output_path: &Path) -> Result<()> {
    let manifest = VendorArtifactsManifest::load_from(manifest_path)
        .with_context(|| format!("failed to load manifest from {}", manifest_path.display()))?;

    let arch = std::env::consts::ARCH;
    let mut resolved = Vec::new();

    for artifact in &manifest.artifacts {
        let entry = match &artifact.source {
            VendorSource::VendorFeed {
                url,
                params,
                platforms,
                response_map,
            } => resolve_vendor_feed(
                &artifact.name,
                &artifact.kind,
                url,
                params,
                platforms,
                response_map,
                arch,
            )?,
        };
        resolved.push(entry);
    }

    let resolved_manifest = ResolvedVendorArtifactsManifest {
        resolved_at: chrono::Utc::now(),
        arch: arch.to_string(),
        artifacts: resolved,
    };

    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory {}", parent.display()))?;
    }

    let json = serde_json::to_string_pretty(&resolved_manifest)
        .context("failed to serialize resolved manifest")?;
    std::fs::write(output_path, &json).with_context(|| {
        format!(
            "failed to write resolved manifest to {}",
            output_path.display()
        )
    })?;

    eprintln!(
        "Resolved {} vendor artifact(s) to {}",
        resolved_manifest.artifacts.len(),
        output_path.display()
    );

    Ok(())
}

/// Resolve a single vendor-feed artifact using the response_map for field extraction.
fn resolve_vendor_feed(
    name: &str,
    kind: &bkt_common::manifest::ArtifactKind,
    url_template: &str,
    manifest_params: &HashMap<String, String>,
    platforms: &HashMap<String, String>,
    response_map: &HashMap<String, String>,
    arch: &str,
) -> Result<ResolvedVendorArtifact> {
    // Build parameter map: start with explicit params
    let mut params = manifest_params.clone();

    // Derive platform from platforms map
    if let Some(platform) = platforms.get(arch) {
        params.insert("platform".to_string(), platform.clone());
    } else if !platforms.is_empty() {
        bail!(
            "no platform mapping for architecture '{}' in artifact '{}'",
            arch,
            name
        );
    }

    // Expand template
    let url = expand_template(url_template, &params)
        .with_context(|| format!("failed to expand URL template for artifact '{}'", name))?;

    eprintln!("Resolving {} from {}", name, url);

    // Fetch the vendor feed response
    let body: serde_json::Value = http::download_json(&url, &[])
        .with_context(|| format!("failed to fetch vendor feed for '{}'", name))?;

    eprintln!(
        "  vendor response: {}",
        serde_json::to_string(&body).unwrap_or_default()
    );

    // Extract fields using response_map
    let extract = |key: &str| -> Result<String> {
        let vendor_field = response_map
            .get(key)
            .ok_or_else(|| anyhow!("response_map for '{}' missing required key '{}'", name, key))?;
        body[vendor_field]
            .as_str()
            .map(String::from)
            .ok_or_else(|| {
                anyhow!(
                    "vendor response for '{}' missing field '{}' (mapped from '{}')",
                    name,
                    vendor_field,
                    key
                )
            })
    };

    let artifact_url = extract("url")?;
    let version = extract("version")?;
    let sha256 = extract("sha256")?;
    let vendor_revision = response_map
        .get("vendor_revision")
        .and_then(|field| body[field].as_str().map(String::from));

    let kind_str = match kind {
        bkt_common::manifest::ArtifactKind::Rpm => "rpm",
    };

    eprintln!("  → {} {} ({})", name, version, artifact_url);

    Ok(ResolvedVendorArtifact {
        name: name.to_string(),
        kind: kind_str.to_string(),
        version,
        url: artifact_url,
        sha256,
        vendor_revision,
    })
}

/// Expand `{param}` placeholders in a URL template.
fn expand_template(
    template: &str,
    params: &std::collections::HashMap<String, String>,
) -> Result<String> {
    let mut result = template.to_string();
    let mut pos = 0;

    // Find all {param} placeholders and verify they have values
    while let Some(start) = result[pos..].find('{') {
        let abs_start = pos + start;
        if let Some(end) = result[abs_start..].find('}') {
            let abs_end = abs_start + end;
            let param_name = &result[abs_start + 1..abs_end];

            let value = params.get(param_name).ok_or_else(|| {
                anyhow!(
                    "missing template parameter '{}' (available: {})",
                    param_name,
                    params.keys().cloned().collect::<Vec<_>>().join(", ")
                )
            })?;

            result.replace_range(abs_start..=abs_end, value);
            pos = abs_start + value.len();
        } else {
            break;
        }
    }

    Ok(result)
}

/// Install a resolved vendor artifact by name.
///
/// Reads the resolved manifest, downloads the artifact, verifies its
/// checksum, and installs it.
pub fn install(name: &str, resolved_path: &Path) -> Result<()> {
    let manifest =
        ResolvedVendorArtifactsManifest::load_from(resolved_path).with_context(|| {
            format!(
                "failed to load resolved manifest from {}",
                resolved_path.display()
            )
        })?;

    let artifact = manifest
        .find(name)
        .ok_or_else(|| anyhow!("artifact '{}' not found in resolved manifest", name))?;

    match artifact.kind.as_str() {
        "rpm" => install_rpm(artifact)?,
        other => bail!("unsupported artifact kind '{}' for '{}'", other, name),
    }

    Ok(())
}

/// Download and install an RPM artifact.
fn install_rpm(artifact: &ResolvedVendorArtifact) -> Result<()> {
    eprintln!("Downloading {} v{} ...", artifact.name, artifact.version);

    let data = bkt_common::http::download(&artifact.url)
        .with_context(|| format!("failed to download {}", artifact.name))?;

    eprintln!("Verifying SHA256...");
    let actual = sha256_hex(&data);
    if actual != artifact.sha256 {
        bail!(
            "SHA256 mismatch for {}: expected {}, got {}",
            artifact.name,
            artifact.sha256,
            actual
        );
    }

    // Write RPM to /rpms/ (kept for RPM DB finalization in a later stage)
    std::fs::create_dir_all("/rpms").context("failed to create /rpms")?;
    let rpm_path = format!("/rpms/{}.rpm", artifact.name);
    std::fs::write(&rpm_path, &data)
        .with_context(|| format!("failed to write RPM to {}", rpm_path))?;

    eprintln!("Installing {} via rpm...", artifact.name);
    let output = Command::new("rpm")
        .args(["-i", "--nodb", "--noscripts", "--nodeps", &rpm_path])
        .output()
        .with_context(|| format!("failed to execute rpm for {}", artifact.name))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("rpm install failed for {}: {}", artifact.name, stderr);
    }

    eprintln!("Installed {} v{}", artifact.name, artifact.version);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_expand_template_basic() {
        let mut params = HashMap::new();
        params.insert("channel".to_string(), "stable".to_string());
        params.insert("platform".to_string(), "linux-rpm-x64".to_string());

        let result = expand_template(
            "https://example.com/api/{platform}/{channel}/latest",
            &params,
        )
        .unwrap();

        assert_eq!(
            result,
            "https://example.com/api/linux-rpm-x64/stable/latest"
        );
    }

    #[test]
    fn test_expand_template_missing_param() {
        let params = HashMap::new();
        let result = expand_template("https://example.com/{missing}", &params);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("missing"));
    }

    #[test]
    fn test_expand_template_no_placeholders() {
        let params = HashMap::new();
        let result = expand_template("https://example.com/plain", &params).unwrap();
        assert_eq!(result, "https://example.com/plain");
    }
}
