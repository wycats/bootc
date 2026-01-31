//! Fetchbin command implementation.

use crate::manifest::{HostBinariesManifest, HostBinary, HostBinarySource};
use crate::output::Output;
use crate::pipeline::ExecutionPlan;
use crate::plan::{
    ExecuteContext, ExecutionReport, Operation, Plan, PlanContext, PlanSummary, Plannable, Verb,
};
use crate::pr::ensure_repo;
use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use fetchbin::manifest::{RuntimeVersionSpec, SourceSpec};
use fetchbin::source::SourceConfig;
use fetchbin::{
    BinarySource, CargoSource, FetchError, GithubSource, InstalledBinary, Manifest, PackageSpec,
    RuntimePool, RuntimeVersion,
};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Args)]
pub struct FetchbinArgs {
    #[command(subcommand)]
    pub action: FetchbinAction,
}

#[derive(Debug, Subcommand)]
pub enum FetchbinAction {
    /// Add a host binary to the manifest
    Add {
        /// Source spec (e.g., npm:turbo, cargo:bat, github:owner/repo)
        spec: String,
        /// Optional binary name (useful when packages expose multiple binaries)
        #[arg(long)]
        binary: Option<String>,
        /// GitHub asset pattern (only for github sources)
        #[arg(long)]
        asset: Option<String>,
    },
    /// Remove a host binary from the manifest
    Remove {
        /// Binary name to remove
        name: String,
    },
    /// List host binaries in the manifest
    List {
        /// Output format (table, json)
        #[arg(short, long, default_value = "table")]
        format: String,
    },
    /// Sync: install binaries from manifest
    Sync,
    /// Capture installed binaries into the manifest
    Capture,
}

pub fn run(args: FetchbinArgs, plan: &ExecutionPlan) -> Result<()> {
    match args.action {
        FetchbinAction::Add {
            spec,
            binary,
            asset,
        } => handle_add(&spec, binary, asset, plan),
        FetchbinAction::Remove { name } => handle_remove(&name, plan),
        FetchbinAction::List { format } => handle_list(&format),
        FetchbinAction::Sync => handle_sync(plan),
        FetchbinAction::Capture => handle_capture(plan),
    }
}

// =============================================================================
// Add Command
// =============================================================================

fn handle_add(
    spec: &str,
    binary: Option<String>,
    asset: Option<String>,
    plan: &ExecutionPlan,
) -> Result<()> {
    let manifests_dir = get_manifest_path()?;
    let mut manifest = HostBinariesManifest::load_from_dir(&manifests_dir)?;

    let mut spec = PackageSpec::from_str(spec)?;
    if let Some(asset) = asset.clone() {
        match &mut spec.source {
            SourceConfig::Github { asset_pattern, .. } => {
                *asset_pattern = Some(asset);
            }
            _ => bail!("--asset is only supported for github sources"),
        }
    }

    let name = binary.clone().unwrap_or_else(|| spec.name.clone());
    let entry = host_binary_from_spec(name.clone(), &spec, binary.clone());

    let already_exists = manifest.find(&name).is_some();
    if already_exists {
        Output::warning(format!(
            "Host binary '{}' already in manifest, updating",
            name
        ));
    }

    if plan.should_update_local_manifest() {
        manifest.upsert(entry.clone());
        manifest.save_to_dir(&manifests_dir)?;
        Output::success(format!("Added fetchbin entry '{}'", name));
    } else if plan.dry_run {
        Output::dry_run(format!("Would add fetchbin entry '{}'", name));
    }

    if plan.should_execute_locally() {
        let mut fetchbin_manifest = load_fetchbin_manifest()?;
        let mut runtime = RuntimePool::load(fetchbin_data_dir())?;

        if is_fetchbin_installed(&fetchbin_manifest, &entry) {
            Output::info(format!("Already installed: {}", name));
        } else {
            install_host_binary(&entry, &mut fetchbin_manifest, &mut runtime)?;
            save_fetchbin_manifest(&fetchbin_manifest)?;
            runtime.save()?;
            Output::success(format!("Installed {}", name));
        }
    } else if plan.dry_run {
        Output::dry_run(format!("Would install '{}' locally", name));
    }

    Ok(())
}

// =============================================================================
// Remove Command
// =============================================================================

fn handle_remove(name: &str, plan: &ExecutionPlan) -> Result<()> {
    let manifests_dir = get_manifest_path()?;
    let mut manifest = HostBinariesManifest::load_from_dir(&manifests_dir)?;

    let entry = match manifest.find(name).cloned() {
        Some(entry) => entry,
        None => {
            Output::warning(format!("Host binary '{}' not found in manifest", name));
            return Ok(());
        }
    };

    if plan.should_update_local_manifest() {
        manifest.remove(name);
        manifest.save_to_dir(&manifests_dir)?;
        Output::success(format!("Removed '{}' from manifest", name));
    } else if plan.dry_run {
        Output::dry_run(format!("Would remove '{}' from manifest", name));
    }

    if plan.should_execute_locally() {
        if remove_installed_binary(&entry)? {
            Output::success(format!("Removed '{}' from fetchbin", name));
        } else {
            Output::warning(format!("'{}' was not installed", name));
        }
    } else if plan.dry_run {
        Output::dry_run(format!("Would uninstall '{}' locally", name));
    }

    Ok(())
}

// =============================================================================
// List Command
// =============================================================================

#[derive(serde::Serialize)]
struct FetchbinListEntry {
    name: String,
    source: String,
    version: Option<String>,
    binary: Option<String>,
    installed: bool,
    installed_version: Option<String>,
}

fn handle_list(format: &str) -> Result<()> {
    let manifests_dir = get_manifest_path()?;
    let manifest = HostBinariesManifest::load_from_dir(&manifests_dir)?;
    let fetchbin_manifest = load_fetchbin_manifest().unwrap_or_default();

    let mut rows = Vec::new();
    for entry in &manifest.binaries {
        let binary_name = binary_name_for_entry(entry);
        let installed = fetchbin_manifest.binaries.get(&binary_name);
        rows.push(FetchbinListEntry {
            name: entry.name.clone(),
            source: source_label(&entry.source),
            version: entry.version.clone(),
            binary: entry.binary.clone(),
            installed: installed.is_some(),
            installed_version: installed.map(installed_version),
        });
    }

    if format == "json" {
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }

    if rows.is_empty() {
        Output::info("No fetchbin entries in manifest");
        return Ok(());
    }

    println!(
        "{:<20} {:<18} {:<12} {:<12} Status",
        "Name", "Source", "Wanted", "Installed"
    );
    println!("{}", "-".repeat(80));

    for row in &rows {
        let status = if row.installed {
            if let (Some(wanted), Some(installed)) = (&row.version, &row.installed_version) {
                if wanted == installed {
                    "ok"
                } else {
                    "version-mismatch"
                }
            } else {
                "installed"
            }
        } else {
            "missing"
        };

        println!(
            "{:<20} {:<18} {:<12} {:<12} {}",
            row.name,
            row.source,
            row.version.clone().unwrap_or_else(|| "-".to_string()),
            row.installed_version
                .clone()
                .unwrap_or_else(|| "-".to_string()),
            status
        );
    }

    let manifest_names: HashSet<String> = manifest
        .binaries
        .iter()
        .map(binary_name_for_entry)
        .collect();

    let extra: Vec<String> = fetchbin_manifest
        .binaries
        .keys()
        .filter(|name| !manifest_names.contains(*name))
        .cloned()
        .collect();

    if !extra.is_empty() {
        Output::blank();
        Output::header("Untracked fetchbin binaries");
        for name in extra {
            Output::list_item(name);
        }
    }

    Ok(())
}

// =============================================================================
// Sync Command
// =============================================================================

fn handle_sync(plan: &ExecutionPlan) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let plan_ctx = PlanContext::new(cwd, plan.clone());

    let cmd = FetchbinSyncCommand;
    let sync_plan = cmd.plan(&plan_ctx)?;

    if sync_plan.is_empty() {
        Output::success("All fetchbin entries are already installed.");
        return Ok(());
    }

    let summary = sync_plan.describe();
    print!("{}", summary);

    if plan.dry_run {
        Output::info("Run without --dry-run to apply these changes.");
        return Ok(());
    }

    let total_ops = summary.action_count();
    let mut exec_ctx = ExecuteContext::new(plan.clone());
    exec_ctx.set_total_ops(total_ops);

    let report = sync_plan.execute(&mut exec_ctx)?;
    print!("{}", report);

    Ok(())
}

// =============================================================================
// Capture Command
// =============================================================================

fn handle_capture(plan: &ExecutionPlan) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let plan_ctx = PlanContext::new(cwd, plan.clone());

    let cmd = FetchbinCaptureCommand;
    let capture_plan = cmd.plan(&plan_ctx)?;

    if capture_plan.is_empty() {
        Output::success("Nothing to capture. All fetchbin binaries are already in the manifest.");
        return Ok(());
    }

    let summary = capture_plan.describe();
    print!("{}", summary);

    if plan.dry_run {
        Output::info("Run without --dry-run to capture these binaries.");
        return Ok(());
    }

    let mut exec_ctx = ExecuteContext::new(plan.clone());
    let report = capture_plan.execute(&mut exec_ctx)?;
    print!("{}", report);

    Ok(())
}

// =============================================================================
// Plan-based Sync/Capture
// =============================================================================

pub struct FetchbinSyncCommand;

pub struct FetchbinSyncPlan {
    pub to_install: Vec<HostBinary>,
    pub to_update: Vec<HostBinary>,
    pub already_installed: usize,
}

impl Plannable for FetchbinSyncCommand {
    type Plan = FetchbinSyncPlan;

    fn plan(&self, _ctx: &PlanContext) -> Result<Self::Plan> {
        let manifests_dir = get_manifest_path()?;
        let manifest = HostBinariesManifest::load_from_dir(&manifests_dir)?;
        let fetchbin_manifest = load_fetchbin_manifest().unwrap_or_default();

        let mut to_install = Vec::new();
        let mut to_update = Vec::new();
        let mut already_installed = 0;

        for entry in &manifest.binaries {
            let binary_name = binary_name_for_entry(entry);
            let installed = fetchbin_manifest.binaries.get(&binary_name);

            match installed {
                None => to_install.push(entry.clone()),
                Some(installed) => {
                    if version_matches(entry, installed) {
                        already_installed += 1;
                    } else {
                        to_update.push(entry.clone());
                    }
                }
            }
        }

        Ok(FetchbinSyncPlan {
            to_install,
            to_update,
            already_installed,
        })
    }
}

impl Plan for FetchbinSyncPlan {
    fn is_empty(&self) -> bool {
        self.to_install.is_empty() && self.to_update.is_empty()
    }

    fn describe(&self) -> PlanSummary {
        let mut summary = PlanSummary::new(format!(
            "Fetchbin Sync: {} to install, {} to update, {} already installed",
            self.to_install.len(),
            self.to_update.len(),
            self.already_installed
        ));

        for entry in &self.to_install {
            summary.add_operation(Operation::new(
                Verb::Install,
                format!("fetchbin:{}", entry.name),
            ));
        }

        for entry in &self.to_update {
            summary.add_operation(Operation::new(
                Verb::Update,
                format!("fetchbin:{}", entry.name),
            ));
        }

        summary
    }

    fn execute(self, ctx: &mut ExecuteContext) -> Result<ExecutionReport> {
        let mut report = ExecutionReport::new();

        let mut fetchbin_manifest = load_fetchbin_manifest()?;
        let mut runtime = RuntimePool::load(fetchbin_data_dir())?;

        for entry in self.to_install {
            match install_host_binary(&entry, &mut fetchbin_manifest, &mut runtime) {
                Ok(()) => report.record_success_and_notify(
                    ctx,
                    Verb::Install,
                    format!("fetchbin:{}", entry.name),
                ),
                Err(err) => report.record_failure_and_notify(
                    ctx,
                    Verb::Install,
                    format!("fetchbin:{}", entry.name),
                    err.to_string(),
                ),
            }
        }

        for entry in self.to_update {
            match install_host_binary(&entry, &mut fetchbin_manifest, &mut runtime) {
                Ok(()) => report.record_success_and_notify(
                    ctx,
                    Verb::Update,
                    format!("fetchbin:{}", entry.name),
                ),
                Err(err) => report.record_failure_and_notify(
                    ctx,
                    Verb::Update,
                    format!("fetchbin:{}", entry.name),
                    err.to_string(),
                ),
            }
        }

        save_fetchbin_manifest(&fetchbin_manifest)?;
        runtime.save()?;

        Ok(report)
    }
}

pub struct FetchbinCaptureCommand;

pub struct FetchbinCapturePlan {
    pub to_capture: Vec<HostBinary>,
    pub already_in_manifest: usize,
}

impl Plannable for FetchbinCaptureCommand {
    type Plan = FetchbinCapturePlan;

    fn plan(&self, _ctx: &PlanContext) -> Result<Self::Plan> {
        let manifests_dir = get_manifest_path()?;
        let manifest = HostBinariesManifest::load_from_dir(&manifests_dir)?;
        let fetchbin_manifest = load_fetchbin_manifest().unwrap_or_default();

        let mut to_capture = Vec::new();
        let mut already_in_manifest = 0;

        for (name, installed) in &fetchbin_manifest.binaries {
            if manifest_contains_binary(&manifest, name) {
                already_in_manifest += 1;
                continue;
            }

            if let Some(entry) = host_binary_from_installed(name, installed) {
                to_capture.push(entry);
            }
        }

        to_capture.sort_by(|a, b| a.name.cmp(&b.name));

        Ok(FetchbinCapturePlan {
            to_capture,
            already_in_manifest,
        })
    }
}

impl Plan for FetchbinCapturePlan {
    fn is_empty(&self) -> bool {
        self.to_capture.is_empty()
    }

    fn describe(&self) -> PlanSummary {
        let mut summary = PlanSummary::new(format!(
            "Fetchbin Capture: {} to add, {} already in manifest",
            self.to_capture.len(),
            self.already_in_manifest
        ));

        for entry in &self.to_capture {
            summary.add_operation(Operation::with_details(
                Verb::Capture,
                format!("fetchbin:{}", entry.name),
                source_label(&entry.source),
            ));
        }

        summary
    }

    fn execute(self, ctx: &mut ExecuteContext) -> Result<ExecutionReport> {
        let mut report = ExecutionReport::new();
        let manifests_dir = get_manifest_path()?;
        let mut manifest = HostBinariesManifest::load_from_dir(&manifests_dir)?;

        for entry in self.to_capture {
            manifest.upsert(entry.clone());
            report.record_success_and_notify(
                ctx,
                Verb::Capture,
                format!("fetchbin:{}", entry.name),
            );
        }

        manifest.save_to_dir(&manifests_dir)?;

        Ok(report)
    }
}

// =============================================================================
// Helpers
// =============================================================================

fn get_manifest_path() -> Result<PathBuf> {
    let repo_path = ensure_repo()?;
    Ok(repo_path.join("manifests"))
}

fn fetchbin_data_dir() -> PathBuf {
    let base = directories::BaseDirs::new()
        .map(|d| d.data_dir().to_path_buf())
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|h| PathBuf::from(h).join(".local/share"))
        })
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("fetchbin")
}

fn fetchbin_manifest_path() -> PathBuf {
    fetchbin::Manifest::default_path().unwrap_or_else(|| fetchbin_data_dir().join("manifest.json"))
}

fn load_fetchbin_manifest() -> Result<fetchbin::Manifest> {
    let path = fetchbin_manifest_path();
    fetchbin::Manifest::load(&path).map_err(|err| anyhow::anyhow!(err.to_string()))
}

fn save_fetchbin_manifest(manifest: &fetchbin::Manifest) -> Result<()> {
    let path = fetchbin_manifest_path();
    manifest
        .save(&path)
        .map_err(|err| anyhow::anyhow!(err.to_string()))
}

fn is_fetchbin_installed(fetchbin_manifest: &fetchbin::Manifest, entry: &HostBinary) -> bool {
    let binary_name = binary_name_for_entry(entry);
    fetchbin_manifest.binaries.contains_key(&binary_name)
}

fn manifest_contains_binary(manifest: &HostBinariesManifest, binary_name: &str) -> bool {
    manifest
        .binaries
        .iter()
        .any(|entry| binary_name_for_entry(entry) == binary_name)
}

fn version_matches(entry: &HostBinary, installed: &InstalledBinary) -> bool {
    let Some(expected) = &entry.version else {
        return true;
    };

    match &installed.source {
        SourceSpec::Npm { version, .. } => version == expected,
        SourceSpec::Cargo { version, .. } => version == expected,
        SourceSpec::Github { version, .. } => version == expected,
    }
}

fn binary_name_for_entry(entry: &HostBinary) -> String {
    entry.binary.clone().unwrap_or_else(|| entry.name.clone())
}

fn source_label(source: &HostBinarySource) -> String {
    match source {
        HostBinarySource::Npm { package } => format!("npm:{}", package),
        HostBinarySource::Cargo { crate_name } => format!("cargo:{}", crate_name),
        HostBinarySource::Github { repo, .. } => format!("github:{}", repo),
    }
}

fn host_binary_from_spec(name: String, spec: &PackageSpec, binary: Option<String>) -> HostBinary {
    let source = match &spec.source {
        SourceConfig::Npm { package } => HostBinarySource::Npm {
            package: package.clone(),
        },
        SourceConfig::Cargo { crate_name } => HostBinarySource::Cargo {
            crate_name: crate_name.clone(),
        },
        SourceConfig::Github {
            repo,
            asset_pattern,
        } => HostBinarySource::Github {
            repo: repo.clone(),
            asset_pattern: asset_pattern.clone(),
        },
    };

    HostBinary {
        name,
        source,
        version: spec.version_req.clone(),
        binary,
    }
}

fn host_binary_from_installed(name: &str, installed: &InstalledBinary) -> Option<HostBinary> {
    let (source, version) = match &installed.source {
        SourceSpec::Npm { package, version } => (
            HostBinarySource::Npm {
                package: package.clone(),
            },
            Some(version.clone()),
        ),
        SourceSpec::Cargo {
            crate_name,
            version,
        } => (
            HostBinarySource::Cargo {
                crate_name: crate_name.clone(),
            },
            Some(version.clone()),
        ),
        SourceSpec::Github {
            repo,
            asset,
            version,
        } => (
            HostBinarySource::Github {
                repo: repo.clone(),
                asset_pattern: if asset == "platform" {
                    None
                } else {
                    Some(asset.clone())
                },
            },
            Some(version.clone()),
        ),
    };

    Some(HostBinary {
        name: name.to_string(),
        source,
        version,
        binary: None,
    })
}

fn package_spec_from_host_binary(entry: &HostBinary) -> PackageSpec {
    let source = match &entry.source {
        HostBinarySource::Npm { package } => SourceConfig::Npm {
            package: package.clone(),
        },
        HostBinarySource::Cargo { crate_name } => SourceConfig::Cargo {
            crate_name: crate_name.clone(),
        },
        HostBinarySource::Github {
            repo,
            asset_pattern,
        } => SourceConfig::Github {
            repo: repo.clone(),
            asset_pattern: asset_pattern.clone(),
        },
    };

    PackageSpec {
        name: entry.name.clone(),
        version_req: entry.version.clone(),
        source,
        binary_name: entry.binary.clone(),
    }
}

fn install_host_binary(
    entry: &HostBinary,
    fetchbin_manifest: &mut Manifest,
    runtime: &mut RuntimePool,
) -> Result<()> {
    let data_dir = fetchbin_data_dir();
    let bin_dir = data_dir.join("bin");
    let store_dir = data_dir.join("store");

    let spec = package_spec_from_host_binary(entry);

    let resolved = resolve_versions(&spec, &data_dir)?;
    let latest = resolved
        .first()
        .cloned()
        .ok_or_else(|| FetchError::Parse("no versions resolved".to_string()))?;

    let target_dir = store_dir_for_spec(&spec, &latest.version, &store_dir);
    if target_dir.exists() {
        fs::remove_dir_all(&target_dir)?;
    }

    let fetched = fetch_version(&spec, &latest, &target_dir, runtime, &data_dir)?;

    fs::create_dir_all(&bin_dir)?;
    let binary_name = binary_name_from_path(&fetched.binary_path)?;
    let link_path = bin_dir.join(&binary_name);
    if link_path.exists() {
        fs::remove_file(&link_path)?;
    }
    create_symlink(&fetched.binary_path, &link_path)?;

    fetchbin_manifest.binaries.insert(
        binary_name.clone(),
        InstalledBinary {
            source: source_spec_from_package(&spec, &latest.version),
            binary: binary_name,
            sha256: fetched.sha256,
            installed_at: current_timestamp(),
            runtime: runtime_spec_from_version(fetched.runtime_used.as_ref()),
        },
    );

    Ok(())
}

fn remove_installed_binary(entry: &HostBinary) -> Result<bool> {
    let data_dir = fetchbin_data_dir();
    let bin_dir = data_dir.join("bin");
    let store_dir = data_dir.join("store");
    let manifest_path = fetchbin_manifest_path();

    let mut manifest = fetchbin::Manifest::load(&manifest_path).unwrap_or_default();
    let binary_name = binary_name_for_entry(entry);

    let installed = match manifest.binaries.remove(&binary_name) {
        Some(installed) => installed,
        None => return Ok(false),
    };

    let link_path = bin_dir.join(&installed.binary);
    if link_path.exists() {
        fs::remove_file(&link_path)?;
    }

    let store_path = store_dir_for_installed(&installed, &store_dir);
    if store_path.exists() {
        fs::remove_dir_all(&store_path)?;
    }

    manifest
        .save(&manifest_path)
        .map_err(|err| anyhow::anyhow!(err.to_string()))?;
    Ok(true)
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

fn source_spec_from_package(spec: &PackageSpec, version: &str) -> SourceSpec {
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
            asset: asset_pattern.as_deref().unwrap_or("platform").to_string(),
            version: version.to_string(),
        },
    }
}

fn runtime_spec_from_version(version: Option<&RuntimeVersion>) -> Option<RuntimeVersionSpec> {
    version.map(|RuntimeVersion::Node(version)| RuntimeVersionSpec::Node {
        version: version.clone(),
    })
}

fn installed_version(installed: &InstalledBinary) -> String {
    match &installed.source {
        SourceSpec::Npm { version, .. } => version.clone(),
        SourceSpec::Cargo { version, .. } => version.clone(),
        SourceSpec::Github { version, .. } => version.clone(),
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
