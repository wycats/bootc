use crate::error::FetchError;
use crate::manifest::{InstalledBinary, SourceSpec};
use crate::runtime::{RuntimePool, RuntimeVersion};
use crate::source::{
    BinarySource, EngineRequirements, FetchedBinary, PackageSpec, ResolvedVersion, SourceConfig,
};
use reqwest::blocking::Client;
use semver::{Version, VersionReq};
use serde::Deserialize;
use std::collections::HashMap;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct NpmSource {
    registry_base: String,
    client: Client,
}

impl NpmSource {
    pub fn new() -> Self {
        Self::with_registry_base("https://registry.npmjs.org")
    }

    pub fn with_registry_base(base: impl Into<String>) -> Self {
        Self {
            registry_base: base.into(),
            client: Client::new(),
        }
    }

    fn registry_url(&self, package: &str) -> String {
        let encoded = encode_package_name(package);
        format!("{}/{}", self.registry_base.trim_end_matches('/'), encoded)
    }

    fn fetch_metadata(&self, package: &str) -> Result<NpmPackageMetadata, FetchError> {
        let url = self.registry_url(package);
        let response = self
            .client
            .get(url)
            .send()
            .map_err(|err| FetchError::NpmRegistry(err.to_string()))?
            .error_for_status()
            .map_err(|err| FetchError::NpmRegistry(err.to_string()))?;

        response
            .json::<NpmPackageMetadata>()
            .map_err(|err| FetchError::Parse(err.to_string()))
    }
}

impl Default for NpmSource {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct NpmPackageMetadata {
    name: String,
    #[serde(rename = "dist-tags")]
    dist_tags: DistTags,
    versions: HashMap<String, NpmVersionMetadata>,
}

#[derive(Debug, Deserialize)]
struct DistTags {
    latest: String,
}

#[derive(Debug, Deserialize)]
struct NpmVersionMetadata {
    version: String,
    dist: NpmDist,
    bin: Option<BinField>,
    engines: Option<EnginesField>,
}

#[derive(Debug, Deserialize)]
struct NpmDist {
    #[serde(default)]
    tarball: Option<String>,
}

#[derive(Debug, Deserialize)]
struct EnginesField {
    #[serde(default)]
    node: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum BinField {
    Single(String),
    Multiple(HashMap<String, String>),
}

impl BinarySource for NpmSource {
    fn source_type(&self) -> &'static str {
        "npm"
    }

    fn resolve(&self, spec: &PackageSpec) -> Result<Vec<ResolvedVersion>, FetchError> {
        let package = match &spec.source {
            SourceConfig::Npm { package } => package.as_str(),
            _ => {
                return Err(FetchError::Parse(
                    "NpmSource used with non-npm spec".to_string(),
                ))
            }
        };

        let metadata = self.fetch_metadata(package)?;

        let resolved = if let Some(req) = spec.version_req.as_deref() {
            if req == "latest" {
                let latest = metadata.dist_tags.latest;
                let version_meta = metadata
                    .versions
                    .get(&latest)
                    .ok_or_else(|| FetchError::Parse(format!("version {latest} not found")))?;
                vec![resolved_from_metadata(version_meta)]
            } else if let Some(version_meta) = find_version(&metadata.versions, req) {
                vec![resolved_from_metadata(version_meta)]
            } else if let Ok(req) = VersionReq::parse(req) {
                let mut matching = matching_versions(&metadata.versions, &req);
                if matching.is_empty() {
                    return Err(FetchError::Parse(format!("no versions matching {req}")));
                }
                matching
                    .drain(..)
                    .map(|(_, meta)| resolved_from_metadata(meta))
                    .collect()
            } else {
                return Err(FetchError::Parse(format!(
                    "unsupported npm version requirement: {req}"
                )));
            }
        } else {
            let latest = metadata.dist_tags.latest;
            let version_meta = metadata
                .versions
                .get(&latest)
                .ok_or_else(|| FetchError::Parse(format!("version {latest} not found")))?;
            vec![resolved_from_metadata(version_meta)]
        };

        Ok(resolved)
    }

    fn fetch(
        &self,
        spec: &PackageSpec,
        version: &ResolvedVersion,
        target_dir: &Path,
        runtime: &mut RuntimePool,
    ) -> Result<FetchedBinary, FetchError> {
        let package = match &spec.source {
            SourceConfig::Npm { package } => package.as_str(),
            _ => {
                return Err(FetchError::Parse(
                    "NpmSource used with non-npm spec".to_string(),
                ))
            }
        };

        let metadata = self.fetch_metadata(package)?;
        let version_meta = metadata
            .versions
            .get(&version.version)
            .ok_or_else(|| FetchError::Parse(format!("version {} not found", version.version)))?;

        let node_requirement = version_meta
            .engines
            .as_ref()
            .and_then(|engines| engines.node.as_deref());
        let node_runtime = runtime
            .get_node(node_requirement)
            .map_err(|err| FetchError::Parse(err.to_string()))?;
        let pnpm_runtime = runtime
            .get_pnpm()
            .map_err(|err| FetchError::Parse(err.to_string()))?;

        let store_dir = runtime
            .data_dir
            .join("store")
            .join("npm")
            .join(package)
            .join(&version.version);
        if store_dir.exists() {
            fs::remove_dir_all(&store_dir)?;
        }
        fs::create_dir_all(&store_dir)?;

        let mut command = Command::new(&pnpm_runtime.pnpm_path);
        command
            .current_dir(&store_dir)
            .arg("add")
            .arg("--ignore-scripts")
            .arg(format!("{package}@{}", version.version));

        if let Some(node_bin_dir) = node_runtime.node_path.parent() {
            let current = env::var_os("PATH").unwrap_or_else(|| OsString::new());
            let mut paths: Vec<PathBuf> = env::split_paths(&current).collect();
            paths.insert(0, node_bin_dir.to_path_buf());
            if let Ok(joined) = env::join_paths(paths) {
                command.env("PATH", joined);
            }
        }

        let output = command
            .output()
            .map_err(|err| FetchError::PnpmInstallFailed(format!("failed to run pnpm: {err}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(FetchError::PnpmInstallFailed(stderr.to_string()));
        }

        let bins = collect_bins(package, version_meta.bin.as_ref())?;
        let binary_name = select_binary_name(package, &bins, spec.binary_name.as_deref())?;
        let bin_rel = bins
            .get(&binary_name)
            .ok_or_else(|| FetchError::BinaryNotFound {
                package: package.to_string(),
                searched: bins.keys().cloned().collect(),
            })?;

        let bin_dir = store_dir.join("node_modules").join(".bin");
        let link_path = bin_dir.join(&binary_name);
        let package_bin = store_dir
            .join("node_modules")
            .join(package)
            .join(bin_rel.trim_start_matches("./"));

        let js_binary_path = if package_bin.exists() {
            package_bin
        } else if link_path.exists() {
            link_path
        } else {
            return Err(FetchError::BinaryNotFound {
                package: package.to_string(),
                searched: vec![
                    link_path.display().to_string(),
                    package_bin.display().to_string(),
                ],
            });
        };

        set_executable(&js_binary_path)?;

        fs::create_dir_all(target_dir)?;
        let target_path = npm_wrapper_path(target_dir, &binary_name);
        if target_path.exists() {
            fs::remove_file(&target_path)?;
        }
        create_npm_wrapper(&target_path, &node_runtime.node_path, &js_binary_path)?;

        let sha256 = crate::source::github::checksum::sha256_hex(&fs::read(&target_path)?);

        Ok(FetchedBinary {
            binary_path: target_path,
            version: version.version.clone(),
            sha256,
            runtime_used: Some(RuntimeVersion::Node(node_runtime.version.clone())),
        })
    }

    fn check_update(
        &self,
        installed: &InstalledBinary,
    ) -> Result<Option<ResolvedVersion>, FetchError> {
        let (package, current_version) = match &installed.source {
            SourceSpec::Npm { package, version } => (package, version),
            _ => {
                return Err(FetchError::Parse(
                    "NpmSource used with non-npm install".to_string(),
                ))
            }
        };

        let spec = PackageSpec {
            name: package_name(package).to_string(),
            version_req: None,
            source: SourceConfig::Npm {
                package: package.clone(),
            },
            binary_name: Some(installed.binary.clone()),
        };

        let latest = self
            .resolve(&spec)?
            .into_iter()
            .next()
            .ok_or_else(|| FetchError::Parse("no versions returned".to_string()))?;

        if versions_match(&latest.version, current_version) {
            Ok(None)
        } else {
            Ok(Some(latest))
        }
    }
}

fn encode_package_name(package: &str) -> String {
    package.replace('@', "%40").replace('/', "%2F")
}

fn package_name(package: &str) -> &str {
    package.rsplit('/').next().unwrap_or(package)
}

fn resolved_from_metadata(metadata: &NpmVersionMetadata) -> ResolvedVersion {
    ResolvedVersion {
        version: metadata.version.clone(),
        download_url: metadata.dist.tarball.clone(),
        checksum: None,
        engines: metadata.engines.as_ref().map(|engines| EngineRequirements {
            node: engines.node.clone(),
        }),
    }
}

fn find_version<'a>(
    versions: &'a HashMap<String, NpmVersionMetadata>,
    requested: &str,
) -> Option<&'a NpmVersionMetadata> {
    versions
        .get(requested)
        .or_else(|| versions.get(requested.trim_start_matches('v')))
}

fn matching_versions<'a>(
    versions: &'a HashMap<String, NpmVersionMetadata>,
    req: &VersionReq,
) -> Vec<(Version, &'a NpmVersionMetadata)> {
    let mut matches = Vec::new();
    for (version, metadata) in versions {
        if let Ok(parsed) = Version::parse(version) {
            if req.matches(&parsed) {
                matches.push((parsed, metadata));
            }
        }
    }
    matches.sort_by(|(left, _), (right, _)| right.cmp(left));
    matches
}

fn collect_bins(
    package: &str,
    bin: Option<&BinField>,
) -> Result<HashMap<String, String>, FetchError> {
    let Some(bin) = bin else {
        return Err(FetchError::BinaryNotFound {
            package: package.to_string(),
            searched: Vec::new(),
        });
    };

    let bins = match bin {
        BinField::Single(path) => {
            let mut map = HashMap::new();
            map.insert(package_name(package).to_string(), path.clone());
            map
        }
        BinField::Multiple(map) => map.clone(),
    };

    if bins.is_empty() {
        return Err(FetchError::BinaryNotFound {
            package: package.to_string(),
            searched: Vec::new(),
        });
    }

    Ok(bins)
}

fn select_binary_name(
    package: &str,
    bins: &HashMap<String, String>,
    requested: Option<&str>,
) -> Result<String, FetchError> {
    if let Some(requested) = requested {
        if bins.contains_key(requested) {
            return Ok(requested.to_string());
        }

        return Err(FetchError::BinaryNotFound {
            package: package.to_string(),
            searched: bins.keys().cloned().collect(),
        });
    }

    if bins.len() > 1 {
        return Err(FetchError::MultipleBinaries {
            binaries: bins.keys().cloned().collect(),
        });
    }

    bins.keys()
        .next()
        .cloned()
        .ok_or_else(|| FetchError::BinaryNotFound {
            package: package.to_string(),
            searched: Vec::new(),
        })
}

fn versions_match(left: &str, right: &str) -> bool {
    normalize_version(left) == normalize_version(right)
}

fn normalize_version(value: &str) -> &str {
    value.trim_start_matches('v')
}

fn set_executable(path: &Path) -> Result<(), FetchError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms)?;
    }

    Ok(())
}

fn npm_wrapper_path(target_dir: &Path, binary_name: &str) -> PathBuf {
    #[cfg(windows)]
    {
        return target_dir.join(format!("{binary_name}.cmd"));
    }

    #[cfg(not(windows))]
    {
        return target_dir.join(binary_name);
    }
}

fn create_npm_wrapper(
    wrapper_path: &Path,
    node_path: &Path,
    js_binary_path: &Path,
) -> Result<(), FetchError> {
    #[cfg(windows)]
    let script = format!(
        "@echo off\r\n\"{}\" \"{}\" %*\r\n",
        node_path.display(),
        js_binary_path.display()
    );

    #[cfg(not(windows))]
    let script = format!(
        "#!/bin/sh\nexec \"{}\" \"{}\" \"$@\"\n",
        node_path.display(),
        js_binary_path.display()
    );

    fs::write(wrapper_path, &script)?;
    set_executable(wrapper_path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::SourceConfig;
    use mockito::Server;
    use tempfile::tempdir;

    #[test]
    fn test_parse_single_bin() {
        let json = r#"{
            "version": "1.0.0",
            "dist": { "tarball": "https://example.com/pkg.tgz" },
            "bin": "./cli.js"
        }"#;

        let meta: NpmVersionMetadata = serde_json::from_str(json).expect("parse");
        let bin = meta.bin.expect("bin");
        match bin {
            BinField::Single(path) => assert_eq!(path, "./cli.js"),
            _ => panic!("expected single bin"),
        }
    }

    #[test]
    fn test_parse_multiple_bins() {
        let json = r#"{
            "version": "1.0.0",
            "dist": { "tarball": "https://example.com/pkg.tgz" },
            "bin": { "a": "./a.js", "b": "./b.js" }
        }"#;

        let meta: NpmVersionMetadata = serde_json::from_str(json).expect("parse");
        let bin = meta.bin.expect("bin");
        match bin {
            BinField::Multiple(map) => {
                assert_eq!(map.get("a"), Some(&"./a.js".to_string()));
                assert_eq!(map.get("b"), Some(&"./b.js".to_string()));
            }
            _ => panic!("expected multiple bin"),
        }
    }

    #[test]
    fn test_parse_engines_node() {
        let json = r#"{
            "version": "1.0.0",
            "dist": { "tarball": "https://example.com/pkg.tgz" },
            "engines": { "node": ">=18" }
        }"#;

        let meta: NpmVersionMetadata = serde_json::from_str(json).expect("parse");
        let engines = meta.engines.expect("engines");
        assert_eq!(engines.node.as_deref(), Some(">=18"));
    }

    #[test]
    fn test_select_binary_name_with_requested() {
        let mut bins = HashMap::new();
        bins.insert("biome".to_string(), "./bin/biome".to_string());
        bins.insert("biome-lsp".to_string(), "./bin/biome-lsp".to_string());

        let result = select_binary_name("@biomejs/biome", &bins, Some("biome"));
        assert_eq!(result.unwrap(), "biome");
    }

    #[test]
    fn test_select_binary_name_requested_not_found() {
        let mut bins = HashMap::new();
        bins.insert("biome".to_string(), "./bin/biome".to_string());

        let result = select_binary_name("@biomejs/biome", &bins, Some("nonexistent"));
        match result {
            Err(FetchError::BinaryNotFound { package, searched }) => {
                assert_eq!(package, "@biomejs/biome");
                assert!(searched.contains(&"biome".to_string()));
            }
            _ => panic!("expected BinaryNotFound error"),
        }
    }

    #[test]
    fn test_select_binary_name_multiple_without_request() {
        let mut bins = HashMap::new();
        bins.insert("a".to_string(), "./a.js".to_string());
        bins.insert("b".to_string(), "./b.js".to_string());

        let result = select_binary_name("pkg", &bins, None);
        match result {
            Err(FetchError::MultipleBinaries { binaries }) => {
                assert_eq!(binaries.len(), 2);
            }
            _ => panic!("expected MultipleBinaries error"),
        }
    }

    #[test]
    fn test_select_binary_name_single_without_request() {
        let mut bins = HashMap::new();
        bins.insert("only-one".to_string(), "./only-one.js".to_string());

        let result = select_binary_name("pkg", &bins, None);
        assert_eq!(result.unwrap(), "only-one");
    }

    #[test]
    fn test_scoped_package_url() {
        assert_eq!(encode_package_name("@scope/pkg"), "%40scope%2Fpkg");
    }

    #[test]
    fn test_resolve_latest_version() {
        let mut server = Server::new();
        let body = r#"{
            "name": "left-pad",
            "dist-tags": { "latest": "1.3.0" },
            "versions": {
                "1.2.0": { "version": "1.2.0", "dist": { "tarball": "https://example.com/1.2.0.tgz" } },
                "1.3.0": { "version": "1.3.0", "dist": { "tarball": "https://example.com/1.3.0.tgz" } }
            }
        }"#;

        server
            .mock("GET", "/left-pad")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(body)
            .create();

        let source = NpmSource::with_registry_base(server.url());
        let spec = PackageSpec {
            name: "left-pad".to_string(),
            version_req: None,
            source: SourceConfig::Npm {
                package: "left-pad".to_string(),
            },
            binary_name: None,
        };

        let resolved = source.resolve(&spec).expect("resolve");
        assert_eq!(
            resolved.first().map(|item| item.version.as_str()),
            Some("1.3.0")
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_npm_wrapper_script_format() {
        let temp = tempdir().expect("tempdir");
        let wrapper_path = temp.path().join("cowsay");
        let node_path = Path::new("/opt/fetchbin/node/bin/node");
        let js_path = Path::new("/opt/fetchbin/store/npm/cowsay/index.js");

        create_npm_wrapper(&wrapper_path, node_path, js_path).expect("create wrapper");

        let contents = fs::read_to_string(&wrapper_path).expect("read wrapper");
        assert_eq!(
            contents,
            "#!/bin/sh\nexec \"/opt/fetchbin/node/bin/node\" \"/opt/fetchbin/store/npm/cowsay/index.js\" \"$@\"\n"
        );
    }

    #[cfg(windows)]
    #[test]
    fn test_npm_wrapper_script_format() {
        let temp = tempdir().expect("tempdir");
        let wrapper_path = temp.path().join("cowsay.cmd");
        let node_path = Path::new("C:\\fetchbin\\node\\node.exe");
        let js_path = Path::new("C:\\fetchbin\\store\\npm\\cowsay\\index.js");

        create_npm_wrapper(&wrapper_path, node_path, js_path).expect("create wrapper");

        let contents = fs::read_to_string(&wrapper_path).expect("read wrapper");
        assert_eq!(
            contents,
            "@echo off\r\n\"C:\\fetchbin\\node\\node.exe\" \"C:\\fetchbin\\store\\npm\\cowsay\\index.js\" %*\r\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_npm_wrapper_is_executable() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempdir().expect("tempdir");
        let wrapper_path = temp.path().join("cowsay");
        let node_path = Path::new("/opt/fetchbin/node/bin/node");
        let js_path = Path::new("/opt/fetchbin/store/npm/cowsay/index.js");

        create_npm_wrapper(&wrapper_path, node_path, js_path).expect("create wrapper");

        let perms = fs::metadata(&wrapper_path).expect("metadata").permissions();
        assert_eq!(perms.mode() & 0o777, 0o755);
    }
}
