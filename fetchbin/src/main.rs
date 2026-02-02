use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use fetchbin::manifest::{RuntimeVersionSpec, SourceSpec};
use fetchbin::source::SourceConfig;
use fetchbin::{
    BinarySource, CargoSource, FetchError, GithubSource, InstalledBinary, Manifest, PackageSpec,
    RuntimePool, RuntimeVersion,
};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Parser, Debug)]
#[command(name = "fetchbin")]
#[command(about = "Acquire binaries from multiple sources", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Install {
        spec: String,
        #[arg(short, long)]
        asset: Option<String>,
        /// Select a specific binary from packages with multiple binaries
        #[arg(short, long)]
        bin: Option<String>,
    },
    List,
    Update,
    Remove {
        name: String,
    },
}

fn main() {
    let cli = Cli::parse();

    if let Err(err) = run(cli) {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Commands::Install { spec, asset, bin } => {
            cmd_install(&spec, asset.as_deref(), bin.as_deref())
        }
        Commands::List => cmd_list(),
        Commands::Update => cmd_update(),
        Commands::Remove { name } => cmd_remove(&name),
    }
}

fn cmd_install(spec: &str, asset: Option<&str>, bin: Option<&str>) -> Result<()> {
    let data_dir = fetchbin_data_dir();
    let bin_dir = data_dir.join("bin");
    let store_dir = data_dir.join("store");
    let manifest_path = manifest_path(&data_dir)?;

    let mut spec = PackageSpec::from_str(spec)?;
    if let Some(asset) = asset {
        match &mut spec.source {
            SourceConfig::Github { asset_pattern, .. } => {
                *asset_pattern = Some(asset.to_string());
            }
            _ => bail!("--asset is only supported for github sources"),
        }
    }

    if let Some(bin) = bin {
        spec.binary_name = Some(bin.to_string());
    }

    println!("Installing {}...", spec.name);

    let mut runtime = RuntimePool::load(data_dir.clone())?;

    let resolved = resolve_versions(&spec, &data_dir)?;
    let latest = resolved
        .first()
        .cloned()
        .ok_or_else(|| FetchError::Parse("no versions resolved".to_string()))?;
    println!("  ✓ Resolved {}@{}", spec.name, latest.version);

    let target_dir = store_dir_for_spec(&spec, &latest.version, &store_dir);
    if target_dir.exists() {
        fs::remove_dir_all(&target_dir)?;
    }

    let fetched = fetch_version(&spec, &latest, &target_dir, &mut runtime, &data_dir)?;
    println!("  ✓ Downloaded and installed");

    fs::create_dir_all(&bin_dir)?;
    let binary_name = binary_name_from_path(&fetched.binary_path)?;
    let link_path = bin_dir.join(&binary_name);
    if link_path.exists() {
        fs::remove_file(&link_path)?;
    }
    create_symlink(&fetched.binary_path, &link_path)?;
    println!("  ✓ Linked to {}", link_path.display());

    let mut manifest = Manifest::load(&manifest_path)?;
    manifest.binaries.insert(
        binary_name.clone(),
        InstalledBinary {
            source: source_spec_from_package(&spec, &latest.version, asset),
            binary: binary_name,
            sha256: fetched.sha256,
            installed_at: current_timestamp(),
            runtime: runtime_spec_from_version(fetched.runtime_used.as_ref()),
        },
    );
    manifest.save(&manifest_path)?;

    // Prune unused Node versions
    let used_versions = collect_used_node_versions(&manifest);
    let _ = runtime.prune(&used_versions);
    runtime.save()?;

    Ok(())
}

fn cmd_list() -> Result<()> {
    let data_dir = fetchbin_data_dir();
    let manifest_path = manifest_path(&data_dir)?;
    let manifest = Manifest::load(&manifest_path)?;

    let mut entries: BTreeMap<String, (String, String)> = BTreeMap::new();
    for (name, entry) in manifest.binaries.iter() {
        let (version, source) = installed_version_source(entry);
        entries.insert(name.clone(), (version, source));
    }

    for (name, (version, source)) in entries {
        println!("  {:<12} {:<8} {}", name, version, source);
    }

    Ok(())
}

fn cmd_update() -> Result<()> {
    let data_dir = fetchbin_data_dir();
    let bin_dir = data_dir.join("bin");
    let store_dir = data_dir.join("store");
    let manifest_path = manifest_path(&data_dir)?;

    let mut manifest = Manifest::load(&manifest_path)?;
    let mut runtime = RuntimePool::load(data_dir.clone())?;

    let keys: Vec<String> = manifest.binaries.keys().cloned().collect();
    let mut updated = 0;

    for name in keys {
        let installed = match manifest.binaries.get(&name).cloned() {
            Some(value) => value,
            None => continue,
        };

        let spec = package_from_installed(&installed)?;
        let update = check_update(&spec, &installed, &data_dir)?;
        let Some(new_version) = update else {
            continue;
        };

        println!("Updating {}...", name);
        println!("  ✓ Resolved {}@{}", spec.name, new_version.version);

        let target_dir = store_dir_for_spec(&spec, &new_version.version, &store_dir);
        if target_dir.exists() {
            fs::remove_dir_all(&target_dir)?;
        }

        let fetched = fetch_version(&spec, &new_version, &target_dir, &mut runtime, &data_dir)?;
        fs::create_dir_all(&bin_dir)?;
        let link_path = bin_dir.join(&installed.binary);
        if link_path.exists() {
            fs::remove_file(&link_path)?;
        }
        create_symlink(&fetched.binary_path, &link_path)?;

        let previous_store = store_dir_for_installed(&installed, &store_dir);
        if previous_store.exists() {
            fs::remove_dir_all(&previous_store)?;
        }

        manifest.binaries.insert(
            name.clone(),
            InstalledBinary {
                source: source_spec_from_installed(&installed, &new_version.version),
                binary: installed.binary.clone(),
                sha256: fetched.sha256,
                installed_at: current_timestamp(),
                runtime: runtime_spec_from_version(fetched.runtime_used.as_ref()),
            },
        );
        updated += 1;
    }

    manifest.save(&manifest_path)?;

    // Prune unused Node versions
    let used_versions = collect_used_node_versions(&manifest);
    let _ = runtime.prune(&used_versions);
    runtime.save()?;

    if updated == 0 {
        println!("All binaries are already up to date.");
    }

    Ok(())
}

fn cmd_remove(name: &str) -> Result<()> {
    let data_dir = fetchbin_data_dir();
    let bin_dir = data_dir.join("bin");
    let store_dir = data_dir.join("store");
    let manifest_path = manifest_path(&data_dir)?;

    let mut manifest = Manifest::load(&manifest_path)?;
    let installed = manifest
        .binaries
        .remove(name)
        .ok_or_else(|| anyhow::anyhow!("binary '{name}' not found"))?;

    let link_path = bin_dir.join(&installed.binary);
    if link_path.exists() {
        fs::remove_file(&link_path)?;
    }

    let store_path = store_dir_for_installed(&installed, &store_dir);
    if store_path.exists() {
        fs::remove_dir_all(&store_path)?;
    }

    manifest.save(&manifest_path)?;
    println!("Removed {}", name);
    Ok(())
}

fn fetchbin_data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("fetchbin")
}

fn manifest_path(data_dir: &Path) -> Result<PathBuf> {
    Ok(Manifest::default_path().unwrap_or_else(|| data_dir.join("manifest.json")))
}

fn resolve_versions(spec: &PackageSpec, data_dir: &Path) -> Result<Vec<fetchbin::ResolvedVersion>> {
    let resolved = match &spec.source {
        SourceConfig::Npm { .. } => fetchbin::source::npm::NpmSource::new().resolve(spec)?,
        SourceConfig::Cargo { .. } => CargoSource::new(data_dir.to_path_buf()).resolve(spec)?,
        SourceConfig::Github { .. } => GithubSource::new().resolve(spec)?,
    };
    Ok(resolved)
}

fn fetch_version(
    spec: &PackageSpec,
    version: &fetchbin::ResolvedVersion,
    target_dir: &Path,
    runtime: &mut RuntimePool,
    data_dir: &Path,
) -> Result<fetchbin::FetchedBinary> {
    let fetched = match &spec.source {
        SourceConfig::Npm { .. } => {
            fetchbin::source::npm::NpmSource::new().fetch(spec, version, target_dir, runtime)?
        }
        SourceConfig::Cargo { .. } => {
            CargoSource::new(data_dir.to_path_buf()).fetch(spec, version, target_dir, runtime)?
        }
        SourceConfig::Github { .. } => {
            GithubSource::new().fetch(spec, version, target_dir, runtime)?
        }
    };
    Ok(fetched)
}

fn check_update(
    spec: &PackageSpec,
    installed: &InstalledBinary,
    data_dir: &Path,
) -> Result<Option<fetchbin::ResolvedVersion>> {
    let update = match &spec.source {
        SourceConfig::Npm { .. } => {
            fetchbin::source::npm::NpmSource::new().check_update(installed)?
        }
        SourceConfig::Cargo { .. } => {
            CargoSource::new(data_dir.to_path_buf()).check_update(installed)?
        }
        SourceConfig::Github { .. } => GithubSource::new().check_update(installed)?,
    };
    Ok(update)
}

fn store_dir_for_spec(spec: &PackageSpec, version: &str, store_root: &Path) -> PathBuf {
    match &spec.source {
        SourceConfig::Npm { package } => store_root
            .join("npm")
            .join(sanitize_component(package))
            .join(version),
        SourceConfig::Cargo { crate_name } => store_root
            .join("cargo")
            .join(sanitize_component(crate_name))
            .join(version),
        SourceConfig::Github { repo, .. } => store_root
            .join("github")
            .join(sanitize_component(repo))
            .join(version),
    }
}

fn store_dir_for_installed(installed: &InstalledBinary, store_root: &Path) -> PathBuf {
    match &installed.source {
        SourceSpec::Npm { package, version } => store_root
            .join("npm")
            .join(sanitize_component(package))
            .join(version),
        SourceSpec::Cargo {
            crate_name,
            version,
        } => store_root
            .join("cargo")
            .join(sanitize_component(crate_name))
            .join(version),
        SourceSpec::Github { repo, version, .. } => store_root
            .join("github")
            .join(sanitize_component(repo))
            .join(version),
    }
}

fn sanitize_component(value: &str) -> String {
    value.replace('/', "__").replace('@', "")
}

fn binary_name_from_path(path: &Path) -> Result<String> {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.to_string())
        .context("binary path missing file name")
}

fn source_spec_from_package(spec: &PackageSpec, version: &str, asset: Option<&str>) -> SourceSpec {
    match &spec.source {
        SourceConfig::Npm { package } => SourceSpec::Npm {
            package: package.clone(),
            version: version.to_string(),
        },
        SourceConfig::Cargo { crate_name } => SourceSpec::Cargo {
            crate_name: crate_name.clone(),
            version: version.to_string(),
        },
        SourceConfig::Github {
            repo,
            asset_pattern,
        } => SourceSpec::Github {
            repo: repo.clone(),
            asset: asset_pattern
                .as_deref()
                .or(asset)
                .unwrap_or("platform")
                .to_string(),
            version: version.to_string(),
        },
    }
}

fn source_spec_from_installed(installed: &InstalledBinary, version: &str) -> SourceSpec {
    match &installed.source {
        SourceSpec::Npm { package, .. } => SourceSpec::Npm {
            package: package.clone(),
            version: version.to_string(),
        },
        SourceSpec::Cargo { crate_name, .. } => SourceSpec::Cargo {
            crate_name: crate_name.clone(),
            version: version.to_string(),
        },
        SourceSpec::Github { repo, asset, .. } => SourceSpec::Github {
            repo: repo.clone(),
            asset: asset.clone(),
            version: version.to_string(),
        },
    }
}

fn package_from_installed(installed: &InstalledBinary) -> Result<PackageSpec> {
    match &installed.source {
        SourceSpec::Npm { package, .. } => Ok(PackageSpec {
            name: package.clone(),
            version_req: None,
            source: SourceConfig::Npm {
                package: package.clone(),
            },
            binary_name: Some(installed.binary.clone()),
        }),
        SourceSpec::Cargo { crate_name, .. } => Ok(PackageSpec {
            name: crate_name.clone(),
            version_req: None,
            source: SourceConfig::Cargo {
                crate_name: crate_name.clone(),
            },
            binary_name: Some(installed.binary.clone()),
        }),
        SourceSpec::Github { repo, asset, .. } => {
            let asset_pattern = if asset == "platform" {
                None
            } else {
                Some(asset.clone())
            };
            Ok(PackageSpec {
                name: repo.clone(),
                version_req: None,
                source: SourceConfig::Github {
                    repo: repo.clone(),
                    asset_pattern,
                },
                binary_name: Some(installed.binary.clone()),
            })
        }
    }
}

fn runtime_spec_from_version(version: Option<&RuntimeVersion>) -> Option<RuntimeVersionSpec> {
    match version {
        Some(RuntimeVersion::Node(version)) => Some(RuntimeVersionSpec::Node {
            version: version.clone(),
        }),
        None => None,
    }
}

fn installed_version_source(installed: &InstalledBinary) -> (String, String) {
    match &installed.source {
        SourceSpec::Npm { version, .. } => (version.clone(), "npm".to_string()),
        SourceSpec::Cargo { version, .. } => (version.clone(), "cargo".to_string()),
        SourceSpec::Github { version, .. } => (version.clone(), "github".to_string()),
    }
}

fn current_timestamp() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}", now.as_secs())
}

fn create_symlink(target: &Path, link: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link)?;
    }

    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_file(target, link)?;
    }

    #[cfg(not(any(unix, windows)))]
    {
        fs::copy(target, link)?;
    }

    Ok(())
}

fn collect_used_node_versions(manifest: &Manifest) -> HashSet<String> {
    manifest
        .binaries
        .values()
        .filter_map(|binary| {
            binary.runtime.as_ref().and_then(|rt| match rt {
                RuntimeVersionSpec::Node { version } => Some(version.clone()),
            })
        })
        .collect()
}
