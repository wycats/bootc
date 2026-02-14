//! Distrobox command implementation.

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::command_runner::{CommandOptions, CommandRunner};
use crate::context::{CommandDomain, run_command};
use crate::manifest::{DistroboxBins, DistroboxContainer, DistroboxManifest};
use crate::output::Output;
use crate::pipeline::ExecutionPlan;
use crate::plan::{
    ExecuteContext, ExecutionReport, Operation, Plan, PlanContext, PlanSummary, Plannable, Verb,
};
use crate::repo::find_repo_path;

#[derive(Debug, Args)]
pub struct DistroboxArgs {
    #[command(subcommand)]
    pub action: DistroboxAction,
}

#[derive(Debug, Subcommand)]
pub enum DistroboxAction {
    /// Apply distrobox manifest (generate distrobox.ini and assemble)
    Apply,
    /// Capture distrobox.ini into the manifest
    Capture {
        /// Capture packages from a running container
        #[arg(long, value_name = "CONTAINER")]
        packages: Option<String>,

        /// Only capture packages (skip INI parsing)
        #[arg(long, requires = "packages")]
        only_packages: bool,
    },
}

pub fn run(args: DistroboxArgs, plan: &ExecutionPlan) -> Result<()> {
    plan.validate_domain(CommandDomain::Distrobox)?;

    let runner = plan.runner();

    let cwd = std::env::current_dir()?;
    let plan_ctx = PlanContext::new(cwd, plan.clone());

    match args.action {
        DistroboxAction::Apply => {
            let cmd = DistroboxSyncCommand;
            let plan = cmd.plan(&plan_ctx)?;

            if plan.is_empty() {
                Output::success("Nothing to apply. Distrobox is in sync with manifest.");
                return Ok(());
            }

            let summary = plan.describe();
            print!("{}", summary);

            if plan_ctx.is_dry_run() {
                Output::info("Run without --dry-run to apply these changes.");
                return Ok(());
            }

            Output::info("Applying distrobox changes...");
            println!();

            let total_ops = summary.action_count();
            let mut exec_ctx = ExecuteContext::new(plan_ctx.execution_plan().clone());
            exec_ctx.set_total_ops(total_ops);
            exec_ctx.set_progress_callback(super::apply::print_progress);

            let report = plan.execute(&mut exec_ctx)?;
            println!();
            print!("{}", report);
            Ok(())
        }
        DistroboxAction::Capture {
            packages,
            only_packages,
        } => {
            if let Some(container_name) = packages {
                let manifest = DistroboxManifest::load_from_dir(plan_ctx.manifest_dir())?;
                let base_image = get_container_base_image(&manifest, &container_name)?;
                let captured = capture_container_packages(&container_name, &base_image, runner)?;

                if captured.is_empty() {
                    println!(
                        "✓ No user-installed packages found in '{}'.",
                        container_name
                    );
                } else {
                    println!(
                        "✓ Found {} user-installed packages in '{}':",
                        captured.len(),
                        container_name
                    );
                    for pkg in &captured {
                        println!("  + {}", pkg);
                    }

                    if !plan_ctx.is_dry_run() {
                        let mut manifest = manifest;
                        if let Some(container) = manifest.containers.get_mut(&container_name) {
                            container.merge_packages(captured);
                        }
                        manifest.save_to_dir(plan_ctx.manifest_dir())?;
                        println!("✓ Manifest updated.");
                    } else {
                        println!("ℹ Run without --dry-run to update manifest.");
                    }
                }

                if only_packages {
                    return Ok(());
                }
            }

            let cmd = DistroboxCaptureCommand;
            let plan = cmd.plan(&plan_ctx)?;

            if plan.is_empty() {
                Output::success("Nothing to capture. Distrobox manifest is in sync.");
                return Ok(());
            }

            let summary = plan.describe();
            print!("{}", summary);

            if plan_ctx.is_dry_run() {
                Output::info("Run without --dry-run to capture these changes.");
                return Ok(());
            }

            let mut exec_ctx = ExecuteContext::new(plan_ctx.execution_plan().clone());
            let report = plan.execute(&mut exec_ctx)?;
            print!("{}", report);
            Ok(())
        }
    }
}

// ============================================================================
// Apply (manifest -> distrobox.ini + assemble + export)
// ============================================================================

pub struct DistroboxSyncCommand;

pub struct DistroboxSyncPlan {
    ini_path: PathBuf,
    ini_content: String,
    write_ini: bool,
    containers: Vec<ContainerApplyPlan>,
}

#[derive(Debug, Clone)]
struct ContainerApplyPlan {
    name: String,
    bins_from: Vec<String>,
    bins_also: Vec<String>,
    bins_to: String,
}

impl Plannable for DistroboxSyncCommand {
    type Plan = DistroboxSyncPlan;

    fn plan(&self, ctx: &PlanContext) -> Result<Self::Plan> {
        let manifest = DistroboxManifest::load_from_dir(ctx.manifest_dir())?;

        if manifest.containers.is_empty() {
            return Ok(DistroboxSyncPlan {
                ini_path: distrobox_ini_path()?,
                ini_content: String::new(),
                write_ini: false,
                containers: Vec::new(),
            });
        }

        for (name, container) in &manifest.containers {
            container.validate(name)?;
        }

        let ini_content = render_distrobox_ini(&manifest)?;
        let ini_path = distrobox_ini_path()?;
        let existing = fs::read_to_string(&ini_path).unwrap_or_default();
        let write_ini = existing != ini_content;

        let mut containers = Vec::new();
        for (name, container) in &manifest.containers {
            containers.push(ContainerApplyPlan {
                name: name.clone(),
                bins_from: container.bins.from.clone(),
                bins_also: container.bins.also.clone(),
                bins_to: container
                    .bins
                    .to
                    .clone()
                    .unwrap_or_else(|| "~/.local/bin".to_string()),
            });
        }

        Ok(DistroboxSyncPlan {
            ini_path,
            ini_content,
            write_ini,
            containers,
        })
    }
}

impl Plan for DistroboxSyncPlan {
    fn describe(&self) -> PlanSummary {
        let mut summary = PlanSummary::new("Distrobox Apply");

        if self.write_ini {
            summary.add_operation(Operation::new(Verb::Update, "distrobox.ini".to_string()));
        }

        for container in &self.containers {
            summary.add_operation(Operation::new(
                Verb::Update,
                format!("distrobox:{}", container.name),
            ));

            for dir in &container.bins_from {
                summary.add_operation(Operation::new(
                    Verb::Create,
                    format!("distrobox-export-dir:{}", dir),
                ));
            }

            for bin in &container.bins_also {
                summary.add_operation(Operation::new(
                    Verb::Create,
                    format!("distrobox-export:{}", bin),
                ));
            }
        }

        summary
    }

    fn execute(self, ctx: &mut ExecuteContext) -> Result<ExecutionReport> {
        let mut report = ExecutionReport::new();

        if self.write_ini {
            if let Some(parent) = self.ini_path.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("Failed to create directory {}", parent.display()))?;
            }
            fs::write(&self.ini_path, &self.ini_content)
                .with_context(|| format!("Failed to write {}", self.ini_path.display()))?;
            report.record_success_and_notify(ctx, Verb::Update, "distrobox.ini".to_string());
        }

        for container in self.containers {
            Output::info(format!(
                "Applying distrobox container '{}' (this may take a while)...",
                container.name
            ));
            run_assemble(&container.name, &self.ini_path)?;
            report.record_success_and_notify(
                ctx,
                Verb::Update,
                format!("distrobox:{}", container.name),
            );

            for dir in &container.bins_from {
                let expanded_dir = expand_home(dir);
                let bins = list_bins_in_dir(&container.name, &expanded_dir, dir)?;
                for bin in bins {
                    Output::info(format!("Exporting '{}' from '{}'.", bin, container.name));
                    run_export(&container.name, &bin, &container.bins_to)?;
                }
                report.record_success_and_notify(
                    ctx,
                    Verb::Create,
                    format!("distrobox-export-dir:{}", dir),
                );
            }

            for bin in &container.bins_also {
                Output::info(format!("Exporting '{}' from '{}'.", bin, container.name));
                run_export(&container.name, bin, &container.bins_to)?;
                report.record_success_and_notify(
                    ctx,
                    Verb::Create,
                    format!("distrobox-export:{}", bin),
                );
            }
        }

        Ok(report)
    }

    fn is_empty(&self) -> bool {
        self.containers.is_empty() && !self.write_ini
    }
}

fn run_assemble(container: &str, ini_path: &Path) -> Result<()> {
    let ini = ini_path.to_string_lossy().to_string();
    let args = vec![
        "assemble",
        "create",
        "--replace",
        "--name",
        container,
        "--file",
        &ini,
    ];
    let output = run_command("distrobox", &args)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = stderr.trim();
        let stdout = stdout.trim();
        let mut detail = String::new();
        if !stderr.is_empty() {
            detail.push_str(stderr);
        }
        if !stdout.is_empty() {
            if !detail.is_empty() {
                detail.push('\n');
            }
            detail.push_str(stdout);
        }
        if detail.is_empty() {
            bail!("distrobox assemble failed for {}", container);
        }
        bail!("distrobox assemble failed for {}: {}", container, detail);
    }
    Ok(())
}

fn run_export(container: &str, bin: &str, export_path: &str) -> Result<()> {
    let bin = expand_home(bin);
    let export_path = expand_home(export_path);

    fs::create_dir_all(&export_path)
        .with_context(|| format!("Failed to create export path {}", export_path))?;

    // Make export idempotent and safe:
    // - If the target already exists as a distrobox-export shim for the same container+bin, skip.
    // - If the target exists but is not a distrobox shim, refuse to overwrite.
    if let Some(file_name) = Path::new(&bin).file_name() {
        let dest = Path::new(&export_path).join(file_name);
        if dest.exists() {
            let contents = fs::read_to_string(&dest).unwrap_or_default();
            if contents.contains("distrobox_binary") {
                let expected_name = format!("# name: {}", container);
                if contents.contains(&expected_name) && contents.contains(&bin) {
                    return Ok(());
                }

                bail!(
                    "Refusing to overwrite existing distrobox-export shim at {} (different target)",
                    dest.display()
                );
            }

            bail!(
                "Refusing to overwrite existing file at {} (not a distrobox-export shim)",
                dest.display()
            );
        }
    }
    let args = vec![
        "enter",
        container,
        "--",
        "distrobox-export",
        "--bin",
        &bin,
        "--export-path",
        &export_path,
    ];
    let output = run_command("distrobox", &args)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = stderr.trim();
        let stdout = stdout.trim();
        let mut detail = String::new();
        if !stderr.is_empty() {
            detail.push_str(stderr);
        }
        if !stdout.is_empty() {
            if !detail.is_empty() {
                detail.push('\n');
            }
            detail.push_str(stdout);
        }
        if detail.is_empty() {
            bail!("distrobox-export failed for {}", bin);
        }
        bail!("distrobox-export failed for {}: {}", bin, detail);
    }
    Ok(())
}

fn list_bins_in_dir(container: &str, dir: &str, original_dir: &str) -> Result<Vec<String>> {
    let check_args = vec!["enter", container, "--", "test", "-d", dir];
    let output = run_command("distrobox", &check_args)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = stderr.trim();
        let stdout = stdout.trim();
        let mut detail = String::new();
        if !stderr.is_empty() {
            detail.push_str(stderr);
        }
        if !stdout.is_empty() {
            if !detail.is_empty() {
                detail.push('\n');
            }
            detail.push_str(stdout);
        }
        bail!(
            "Distrobox container '{}' is missing bins.from directory '{}': {}",
            container,
            original_dir,
            if detail.is_empty() {
                "(no output)"
            } else {
                &detail
            }
        );
    }

    let find_args = vec![
        "enter",
        container,
        "--",
        "find",
        dir,
        "-maxdepth",
        "1",
        "(",
        "-type",
        "f",
        "-o",
        "-type",
        "l",
        ")",
        "-print",
    ];
    let output = run_command("distrobox", &find_args)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = stderr.trim();
        let stdout = stdout.trim();
        let mut detail = String::new();
        if !stderr.is_empty() {
            detail.push_str(stderr);
        }
        if !stdout.is_empty() {
            if !detail.is_empty() {
                detail.push('\n');
            }
            detail.push_str(stdout);
        }
        bail!(
            "Failed to list bins for distrobox container '{}' in '{}': {}",
            container,
            original_dir,
            if detail.is_empty() {
                "(no output)"
            } else {
                &detail
            }
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect())
}

/// Validate that a container exists and is running.
fn validate_container_state(container_name: &str, runner: &dyn CommandRunner) -> Result<()> {
    let exists = runner.run_status(
        "podman",
        &["container", "exists", container_name],
        &CommandOptions::default(),
    )?;

    if !exists.success() {
        bail!(
            "Container '{}' does not exist. Create it first with:\n  bkt distrobox apply",
            container_name
        );
    }

    let ps_output = runner.run_output(
        "podman",
        &[
            "ps",
            "--filter",
            &format!("name=^{}$", container_name),
            "--format",
            "{{.State}}",
        ],
        &CommandOptions::default(),
    )?;

    let state = String::from_utf8_lossy(&ps_output.stdout)
        .trim()
        .to_string();
    if state != "running" {
        bail!(
            "Container '{}' exists but is not running (state: {}).\nStart it with:\n  distrobox enter {}",
            container_name,
            if state.is_empty() { "stopped" } else { &state },
            container_name
        );
    }

    Ok(())
}

/// Get the base image for a container from the manifest.
fn get_container_base_image(manifest: &DistroboxManifest, container_name: &str) -> Result<String> {
    manifest
        .containers
        .get(container_name)
        .map(|c| c.image.clone())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Container '{}' not found in manifest. Available: {}",
                container_name,
                manifest
                    .containers
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
}

/// Extract packages from a running distrobox container.
fn extract_packages_from_container(
    container_name: &str,
    runner: &dyn CommandRunner,
) -> Result<std::collections::BTreeSet<String>> {
    let output = runner.run_output(
        "distrobox",
        &[
            "enter",
            container_name,
            "--",
            "rpm",
            "-qa",
            "--qf",
            "%{NAME}\n",
        ],
        &CommandOptions::default(),
    )?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "Failed to extract packages from container {}: {}",
            container_name,
            stderr.trim()
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let packages: std::collections::BTreeSet<String> = stdout
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    Ok(packages)
}

/// Extract packages from a container image.
fn extract_packages_from_image(
    image: &str,
    runner: &dyn CommandRunner,
) -> Result<std::collections::BTreeSet<String>> {
    let output = runner.run_output(
        "podman",
        &[
            "run",
            "--rm",
            "--entrypoint",
            "rpm",
            image,
            "-qa",
            "--qf",
            "%{NAME}\n",
        ],
        &CommandOptions::default(),
    )?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "Failed to extract packages from image {}: {}",
            image,
            stderr.trim()
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let packages: std::collections::BTreeSet<String> = stdout
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    Ok(packages)
}

/// Capture user-installed packages from a running distrobox container.
fn capture_container_packages(
    container_name: &str,
    base_image: &str,
    runner: &dyn CommandRunner,
) -> Result<Vec<String>> {
    validate_container_state(container_name, runner)?;

    println!("Extracting packages from container '{}'...", container_name);
    let container_packages = extract_packages_from_container(container_name, runner)?;
    println!("Found {} packages in container", container_packages.len());

    println!("Extracting packages from base image '{}'...", base_image);
    let base_packages = extract_packages_from_image(base_image, runner)?;
    println!("Found {} packages in base image", base_packages.len());

    let user_installed: Vec<String> = container_packages
        .difference(&base_packages)
        .cloned()
        .collect();

    Ok(user_installed)
}

// ============================================================================
// Capture (distrobox.ini -> manifest)
// ============================================================================

pub struct DistroboxCaptureCommand;

pub struct DistroboxCapturePlan {
    manifest: DistroboxManifest,
    manifest_dir: PathBuf,
}

impl Plannable for DistroboxCaptureCommand {
    type Plan = DistroboxCapturePlan;

    fn plan(&self, ctx: &PlanContext) -> Result<Self::Plan> {
        let ini_path = distrobox_ini_path()?;
        if !ini_path.exists() {
            return Ok(DistroboxCapturePlan {
                manifest: DistroboxManifest {
                    schema: Some("../schemas/distrobox.schema.json".to_string()),
                    containers: BTreeMap::new(),
                },
                manifest_dir: ctx.manifest_dir().clone(),
            });
        }

        let contents = fs::read_to_string(&ini_path)
            .with_context(|| format!("Failed to read {}", ini_path.display()))?;

        let existing_manifest = DistroboxManifest::load_from_dir(ctx.manifest_dir())?;
        let mut manifest = parse_distrobox_ini(&contents)?;

        for (name, container) in manifest.containers.iter_mut() {
            if let Some(existing) = existing_manifest.containers.get(name) {
                if !existing.bins.from.is_empty() {
                    container.bins.from = existing.bins.from.clone();
                }
                if container.bins.to.is_none() {
                    container.bins.to = existing.bins.to.clone();
                }
            }
        }

        for (name, container) in &manifest.containers {
            container.validate(name)?;
        }
        Ok(DistroboxCapturePlan {
            manifest,
            manifest_dir: ctx.manifest_dir().clone(),
        })
    }
}

impl Plan for DistroboxCapturePlan {
    fn describe(&self) -> PlanSummary {
        let mut summary = PlanSummary::new("Distrobox Capture");

        for name in self.manifest.containers.keys() {
            summary.add_operation(Operation::new(Verb::Capture, format!("distrobox:{}", name)));
        }

        summary
    }

    fn execute(self, _ctx: &mut ExecuteContext) -> Result<ExecutionReport> {
        let mut report = ExecutionReport::new();
        self.manifest.save_to_dir(&self.manifest_dir)?;

        for name in self.manifest.containers.keys() {
            report.record_success(Verb::Capture, format!("distrobox:{}", name));
        }

        Ok(report)
    }

    fn is_empty(&self) -> bool {
        self.manifest.containers.is_empty()
    }
}

// ============================================================================
// INI rendering/parsing helpers
// ============================================================================

fn distrobox_ini_path() -> Result<PathBuf> {
    Ok(find_repo_path()?.join("distrobox.ini"))
}

fn render_distrobox_ini(manifest: &DistroboxManifest) -> Result<String> {
    let mut out = String::new();
    out.push_str("# Generated by bkt\n");
    out.push_str("# Source: manifests/distrobox.json\n\n");

    for (name, container) in &manifest.containers {
        container.validate(name)?;
        out.push_str(&format!("[{}]\n", name));
        out.push_str(&format!("image={}\n", container.image));

        if !container.packages.is_empty() {
            out.push_str(&format!(
                "additional_packages={}\n",
                format_ini_value(&container.packages.join(" "))
            ));
        }

        if !container.bins.also.is_empty() {
            let bins = container
                .bins
                .also
                .iter()
                .map(|b| expand_home(b))
                .collect::<Vec<_>>()
                .join(" ");
            out.push_str(&format!("exported_bins={}\n", format_ini_value(&bins)));

            let export_path = container.bins.to.as_deref().unwrap_or("~/.local/bin");
            out.push_str(&format!(
                "exported_bins_path={}\n",
                format_ini_value(&expand_home(export_path))
            ));
        }

        if !container.exported_apps.is_empty() {
            out.push_str(&format!(
                "exported_apps={}\n",
                format_ini_value(&container.exported_apps.join(" "))
            ));
        }

        if !container.init_hooks.is_empty() {
            out.push_str(&format!(
                "init_hooks={}\n",
                format_ini_value(&join_hooks(&container.init_hooks))
            ));
        }

        if !container.pre_init_hooks.is_empty() {
            out.push_str(&format!(
                "pre_init_hooks={}\n",
                format_ini_value(&join_hooks(&container.pre_init_hooks))
            ));
        }

        if !container.volume.is_empty() {
            out.push_str(&format!(
                "volume={}\n",
                format_ini_value(&container.volume.join(" "))
            ));
        }

        out.push_str(&format!("pull={}\n", container.pull));
        out.push_str(&format!("init={}\n", container.init));
        out.push_str(&format!("root={}\n", container.root));

        let flags = build_additional_flags(container)?;
        if !flags.is_empty() {
            out.push_str(&format!(
                "additional_flags={}\n",
                format_ini_value(&flags.join(" "))
            ));
        }

        out.push('\n');
    }

    Ok(out)
}

fn format_ini_value(value: &str) -> String {
    if value.contains(' ') || value.contains('\t') {
        format!("\"{}\"", value)
    } else {
        value.to_string()
    }
}

fn join_hooks(hooks: &[String]) -> String {
    if hooks.len() == 1 {
        hooks[0].clone()
    } else {
        hooks.join(" && ")
    }
}

fn build_additional_flags(container: &DistroboxContainer) -> Result<Vec<String>> {
    container.validate("(render)")?;

    let mut flags = Vec::new();
    flags.extend(container.additional_flags.clone());

    if !container.env.is_empty() {
        let mut keys: Vec<&String> = container.env.keys().collect();
        keys.sort();
        for key in keys {
            let value = container.env.get(key).unwrap();
            flags.push(format!("--env={}={}", key, value));
        }
    }

    if !container.path.is_empty() {
        let mut parts = container
            .path
            .iter()
            .map(|p| expand_home(p))
            .collect::<Vec<_>>();
        if !parts.iter().any(|p| p == "$PATH") {
            parts.push("$PATH".to_string());
        }
        flags.push(format!("--env=PATH={}", parts.join(":")));
    }

    Ok(flags)
}

fn parse_distrobox_ini(contents: &str) -> Result<DistroboxManifest> {
    let sections = parse_ini_sections(contents);
    let mut containers = BTreeMap::new();

    for (name, values) in sections {
        if name.is_empty() {
            continue;
        }

        let image = values.get("image").cloned().unwrap_or_default();
        let packages = split_list(values.get("additional_packages"));
        let exported_bins = split_list(values.get("exported_bins"))
            .into_iter()
            .map(|v| collapse_home(&v))
            .collect::<Vec<_>>();
        let exported_bins_path = values.get("exported_bins_path").map(|v| collapse_home(v));
        let mut bins = DistroboxBins::default();
        if !exported_bins.is_empty() || exported_bins_path.is_some() {
            bins.also = exported_bins;
            bins.to = exported_bins_path;
        }
        let exported_apps = split_list(values.get("exported_apps"));
        let init_hooks = split_hooks(values.get("init_hooks"));
        let pre_init_hooks = split_hooks(values.get("pre_init_hooks"));
        let volume = split_list(values.get("volume"));
        let pull = parse_bool(values.get("pull"));
        let init = parse_bool(values.get("init"));
        let root = parse_bool(values.get("root"));

        let mut additional_flags = Vec::new();
        let mut env = BTreeMap::new();
        let mut path = Vec::new();

        if let Some(flags_value) = values.get("additional_flags") {
            let tokens = shlex::split(flags_value).unwrap_or_default();
            let (remaining, env_map, path_parts) = parse_additional_flags(tokens);
            additional_flags = remaining;
            env = env_map;
            if !path_parts.is_empty() {
                path = path_parts.into_iter().map(|p| collapse_home(&p)).collect();
            }
        }

        let container = DistroboxContainer {
            image,
            packages,
            bins,
            exported_apps,
            init_hooks,
            pre_init_hooks,
            volume,
            pull,
            init,
            root,
            path,
            env,
            additional_flags,
        };

        container.validate(&name)?;
        containers.insert(name, container);
    }

    Ok(DistroboxManifest {
        schema: Some("../schemas/distrobox.schema.json".to_string()),
        containers,
    })
}

fn parse_ini_sections(contents: &str) -> BTreeMap<String, BTreeMap<String, String>> {
    let mut sections: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    let mut current = String::new();

    for raw_line in contents.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }

        if line.starts_with('[') && line.ends_with(']') {
            current = line[1..line.len() - 1].trim().to_string();
            continue;
        }

        if let Some((key, value)) = line.split_once('=') {
            let value = value.trim();
            let value = value
                .strip_prefix('"')
                .and_then(|v| v.strip_suffix('"'))
                .unwrap_or(value)
                .to_string();

            sections
                .entry(current.clone())
                .or_default()
                .insert(key.trim().to_string(), value);
        }
    }

    sections
}

fn split_list(value: Option<&String>) -> Vec<String> {
    value
        .map(|v| v.split_whitespace().map(|s| s.to_string()).collect())
        .unwrap_or_default()
}

fn split_hooks(value: Option<&String>) -> Vec<String> {
    let value = match value {
        Some(v) => v.trim(),
        None => return Vec::new(),
    };

    if value.contains('\n') {
        return value
            .lines()
            .map(|line| line.trim().to_string())
            .filter(|line| !line.is_empty())
            .collect();
    }

    if value.contains("&&") {
        return value
            .split("&&")
            .map(|part| part.trim().to_string())
            .filter(|part| !part.is_empty())
            .collect();
    }

    if value.contains(';') {
        return value
            .split(';')
            .map(|part| part.trim().to_string())
            .filter(|part| !part.is_empty())
            .collect();
    }

    vec![value.to_string()]
}

fn parse_bool(value: Option<&String>) -> bool {
    value
        .map(|v| v.trim().eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

fn parse_additional_flags(
    tokens: Vec<String>,
) -> (Vec<String>, BTreeMap<String, String>, Vec<String>) {
    let mut remaining = Vec::new();
    let mut env = BTreeMap::new();
    let mut path = Vec::new();

    let mut i = 0;
    while i < tokens.len() {
        let token = &tokens[i];

        if (token == "--env" || token == "-e")
            && let Some(next) = tokens.get(i + 1)
            && let Some((key, value)) = split_env_pair(next)
        {
            if key == "PATH" {
                path = value.split(':').map(|s| s.to_string()).collect();
            } else {
                env.insert(key, value);
            }
            i += 2;
            continue;
        }

        if let Some(env_pair) = token.strip_prefix("--env=")
            && let Some((key, value)) = split_env_pair(env_pair)
        {
            if key == "PATH" {
                path = value.split(':').map(|s| s.to_string()).collect();
            } else {
                env.insert(key, value);
            }
            i += 1;
            continue;
        }

        if let Some(env_pair) = token.strip_prefix("-e=")
            && let Some((key, value)) = split_env_pair(env_pair)
        {
            if key == "PATH" {
                path = value.split(':').map(|s| s.to_string()).collect();
            } else {
                env.insert(key, value);
            }
            i += 1;
            continue;
        }

        remaining.push(token.clone());
        i += 1;
    }

    (remaining, env, path)
}

fn split_env_pair(value: &str) -> Option<(String, String)> {
    let (key, val) = value.split_once('=')?;
    Some((key.to_string(), val.to_string()))
}

fn expand_home(value: &str) -> String {
    if let Some(rest) = value.strip_prefix("~/")
        && let Ok(home) = std::env::var("HOME")
    {
        return format!("{}/{}", home, rest);
    }
    value.to_string()
}

fn collapse_home(value: &str) -> String {
    if let Ok(home) = std::env::var("HOME") {
        let prefix = format!("{}/", home);
        if let Some(rest) = value.strip_prefix(&prefix) {
            return format!("~/{}", rest);
        }
        if value == home {
            return "~".to_string();
        }
    }
    value.to_string()
}
