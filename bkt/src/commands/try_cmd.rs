//! `bkt try` command implementation.
//!
//! Provides transient overlay installs while capturing manifest changes and PRs.

use anyhow::{Context, Result, bail};
use chrono::Utc;
use clap::{ArgGroup, Args};
use is_terminal::IsTerminal;
use owo_colors::OwoColorize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::command_runner::{CommandOptions, CommandRunner};
use crate::containerfile::{
    ContainerfileEditor, Section, generate_copr_repos, generate_system_packages,
};
use crate::context::CommandDomain;
use crate::dbus::SystemdManager;
use crate::manifest::{
    ServiceState, SystemPackagesManifest, SystemdServicesManifest, TryPendingEntry,
    TryPendingManifest,
};
use crate::output::Output;
use crate::pipeline::ExecutionPlan;
use crate::pr::{ensure_preflight, ensure_repo};
use crate::repo::RepoConfig;
use crate::validation::validate_dnf_package;

/// Arguments for `bkt try`.
#[derive(Debug, Args)]
#[command(
    group(
        ArgGroup::new("action")
            .required(true)
            .args(["packages", "remove", "status", "cleanup"])
    )
)]
pub struct TryArgs {
    /// Packages to install into the transient overlay
    #[arg(value_name = "PACKAGE", num_args = 1..)]
    pub packages: Vec<String>,

    /// Remove a package from the manifest (overlay is unchanged)
    #[arg(long, value_name = "PACKAGE")]
    pub remove: Option<String>,

    /// Show pending try state for the current boot
    #[arg(long)]
    pub status: bool,

    /// Remove persistent side effects (Phase 2)
    #[arg(long, value_name = "PACKAGE")]
    pub cleanup: Option<String>,
}

/// Execute `bkt try`.
pub fn run(args: TryArgs, plan: &ExecutionPlan) -> Result<()> {
    plan.validate_domain(CommandDomain::System)?;

    if args.status {
        return show_status();
    }

    if let Some(package) = args.cleanup {
        return handle_cleanup(&package, plan);
    }

    if let Some(package) = args.remove {
        return handle_remove(&package, plan);
    }

    handle_try_install(&args.packages, plan)
}

fn show_status() -> Result<()> {
    let manifest = TryPendingManifest::load()?;

    if !manifest.is_valid()? {
        Output::info("No pending try state for this boot.");
        return Ok(());
    }

    if manifest.packages.is_empty() {
        Output::info("No pending try packages.");
        return Ok(());
    }

    Output::header("Pending try packages");
    for entry in manifest.packages.values() {
        let pr = entry
            .pr
            .map(|n| format!("PR #{}", n))
            .unwrap_or_else(|| "PR pending".to_string());
        println!(
            "  {} {} ({})",
            entry.package.cyan(),
            entry.installed_at.to_rfc3339().dimmed(),
            pr
        );
        if !entry.services_enabled.is_empty() {
            println!("    services: {}", entry.services_enabled.join(", "));
        }
    }

    Ok(())
}

fn handle_cleanup(package: &str, plan: &ExecutionPlan) -> Result<()> {
    if plan.dry_run {
        Output::dry_run(format!("Would clean up side effects for {}", package));
        return Ok(());
    }

    let mut manifest = TryPendingManifest::load()?;
    if manifest.packages.remove(package).is_some() {
        manifest.save()?;
        Output::warning("Cleanup is not implemented in Phase 1; removed from tracking only.");
    } else {
        Output::info(format!("No pending try entry for {}", package));
    }

    Ok(())
}

fn handle_remove(package: &str, plan: &ExecutionPlan) -> Result<()> {
    let runner = plan.runner();

    if plan.should_create_pr() {
        ensure_preflight(runner, plan.skip_preflight)?;
    }

    let repo_path = if plan.should_create_pr() {
        Some(ensure_repo(runner)?)
    } else {
        None
    };

    let mut repo_manifest = match &repo_path {
        Some(path) => SystemPackagesManifest::load(&path.join("manifests/system-packages.json"))?,
        None => {
            if plan.should_update_local_manifest() {
                SystemPackagesManifest::load_user()?
            } else {
                SystemPackagesManifest::default()
            }
        }
    };

    if !repo_manifest.remove_package(package) {
        Output::info(format!("{} is not in the manifest", package));
        return Ok(());
    }

    if plan.should_create_pr() {
        let repo_path = repo_path.as_ref().expect("repo path available");
        let manifest_path = repo_path.join("manifests/system-packages.json");
        repo_manifest.save(&manifest_path)?;

        let mut changed_files = vec![PathBuf::from("manifests/system-packages.json")];
        if sync_containerfile_sections(repo_path, &repo_manifest)? {
            changed_files.push(PathBuf::from("Containerfile"));
        }

        let title = format!("chore(try): remove {}", package);
        let body = format!(
            "This PR was created by `bkt try --remove`.

## Changes
- Removed `{}` from `manifests/system-packages.json`

---
*Created by bkt CLI*",
            package
        );

        let branch = format!("try/{}", sanitize_branch_component(package));
        commit_try_changes(runner, repo_path, &branch, &changed_files, &title, &body)?;
    } else if plan.should_update_local_manifest() {
        if plan.dry_run {
            Output::dry_run(format!("Would remove {} from user manifest", package));
        } else {
            repo_manifest.save_user()?;
        }
    } else if plan.dry_run {
        Output::dry_run(format!("Would remove {} from manifest", package));
    }

    Output::success(format!("Removed {} from manifest", package));
    Ok(())
}

fn handle_try_install(packages: &[String], plan: &ExecutionPlan) -> Result<()> {
    if packages.is_empty() {
        bail!("No packages specified");
    }

    let runner = plan.runner();

    // Validate packages first to catch typos early.
    if !plan.dry_run {
        for pkg in packages {
            validate_dnf_package(runner, pkg)?;
        }
    }

    let mut pending = TryPendingManifest::load()?;
    let current_boot_id = TryPendingManifest::current_boot_id()?;
    if !pending.is_valid()? {
        pending = TryPendingManifest::new_with_boot_id(current_boot_id.clone());
    } else if pending.boot_id.is_empty() {
        pending.boot_id = current_boot_id.clone();
    }

    let repo_path = if plan.should_create_pr() {
        ensure_preflight(runner, plan.skip_preflight)?;
        Some(ensure_repo(runner)?)
    } else {
        None
    };

    let mut repo_manifest = match &repo_path {
        Some(path) => SystemPackagesManifest::load(&path.join("manifests/system-packages.json"))?,
        None => SystemPackagesManifest::default(),
    };

    let mut user_manifest = if plan.should_update_local_manifest() {
        SystemPackagesManifest::load_user()?
    } else {
        SystemPackagesManifest::default()
    };

    let mut new_packages = Vec::new();
    for pkg in packages {
        if repo_manifest.find_package(pkg) {
            Output::info(format!("Already in manifest: {}", pkg));
        } else {
            new_packages.push(pkg.clone());
        }
    }

    if new_packages.is_empty() {
        Output::success("All packages already in manifest.");
        return Ok(());
    }

    let branch = pending
        .packages
        .values()
        .next()
        .map(|entry| entry.branch.clone())
        .unwrap_or_else(|| format!("try/{}", sanitize_branch_component(&new_packages[0])));

    let mut services_enabled: HashMap<String, Vec<String>> = HashMap::new();

    if plan.should_execute_locally() {
        ensure_usroverlay(runner, plan.dry_run)?;
        ensure_rpm_state_dir(runner, plan.dry_run)?;

        for pkg in &new_packages {
            if let Some(size) = get_download_size_bytes(runner, pkg)?
                && size > 100 * 1024 * 1024
            {
                let mb = size as f64 / (1024.0 * 1024.0);
                let message = format!(
                    "{} requires {:.0}MB download (overlay is RAM-backed). Continue?",
                    pkg, mb
                );
                if !prompt_continue(&message)? {
                    Output::info("Cancelled.");
                    return Ok(());
                }
            }

            install_package(runner, pkg, plan.dry_run)?;

            if let Some(unit) = detect_service_unit(runner, pkg)? {
                let message = format!("Enable {}?", unit.cyan());
                if prompt_continue(&message)? {
                    if plan.dry_run {
                        Output::dry_run(format!("Would enable {}", unit));
                    } else {
                        let manager = SystemdManager::new()?;
                        manager.enable(&unit)?;
                        services_enabled
                            .entry(pkg.clone())
                            .or_default()
                            .push(unit.clone());
                    }
                }
            }
        }
    }

    // Update local manifest if allowed (user manifest).
    if plan.should_update_local_manifest() && !plan.dry_run {
        for pkg in &new_packages {
            user_manifest.add_package(pkg.clone());
        }
        user_manifest.save_user()?;
    }

    if plan.should_create_pr() {
        let repo_path = repo_path.as_ref().expect("repo path available");

        for pkg in &new_packages {
            repo_manifest.add_package(pkg.clone());
        }

        let manifest_path = repo_path.join("manifests/system-packages.json");
        repo_manifest.save(&manifest_path)?;

        let mut changed_files = vec![PathBuf::from("manifests/system-packages.json")];

        if !services_enabled.is_empty() {
            let services_path = repo_path.join("manifests/systemd-services.json");
            let mut services_manifest = SystemdServicesManifest::load(&services_path)?;

            for units in services_enabled.values() {
                for unit in units {
                    services_manifest
                        .services
                        .insert(unit.clone(), ServiceState::Enabled);
                }
            }

            services_manifest.save(&services_path)?;
            changed_files.push(PathBuf::from("manifests/systemd-services.json"));
        }

        if sync_containerfile_sections(repo_path, &repo_manifest)? {
            changed_files.push(PathBuf::from("Containerfile"));
        }

        let title = format!("feat(try): add {}", new_packages.join(", "));
        let body = format!(
            "This PR was created by `bkt try`.

## Changes
- Added {} to `manifests/system-packages.json`

---
*Created by bkt CLI*",
            new_packages
                .iter()
                .map(|p| format!("`{}`", p))
                .collect::<Vec<_>>()
                .join(", ")
        );

        commit_try_changes(runner, repo_path, &branch, &changed_files, &title, &body)?;

        if plan.should_execute_locally() && !plan.dry_run {
            let pr_number = fetch_pr_number(runner, repo_path)?;
            for pkg in &new_packages {
                let entry = TryPendingEntry {
                    package: pkg.clone(),
                    installed_at: Utc::now(),
                    pr: pr_number,
                    branch: branch.clone(),
                    services_enabled: services_enabled.get(pkg).cloned().unwrap_or_default(),
                };
                pending.add_package(entry);
            }
            pending.save()?;
        }
    }

    if plan.should_execute_locally() {
        Output::success(format!("Installed {} package(s)", new_packages.len()));
    } else if plan.should_create_pr() {
        Output::success(format!("Captured {} package(s)", new_packages.len()));
    }
    Ok(())
}

fn ensure_usroverlay(runner: &dyn CommandRunner, dry_run: bool) -> Result<()> {
    if overlay_is_unlocked(runner)? {
        return Ok(());
    }

    if dry_run {
        Output::dry_run("Would unlock /usr overlay via rpm-ostree usroverlay");
        return Ok(());
    }

    Output::info("Unlocking /usr overlay...");
    run_pkexec_status(runner, "/usr/bin/rpm-ostree", &["usroverlay"])?;
    Ok(())
}

fn ensure_rpm_state_dir(runner: &dyn CommandRunner, dry_run: bool) -> Result<()> {
    if dry_run {
        Output::dry_run("Would create /var/lib/rpm-state");
        return Ok(());
    }
    run_pkexec_status(runner, "/usr/bin/mkdir", &["-p", "/var/lib/rpm-state"])?;
    Ok(())
}

fn install_package(runner: &dyn CommandRunner, package: &str, dry_run: bool) -> Result<()> {
    if dry_run {
        Output::dry_run(format!("Would install {}", package));
        return Ok(());
    }

    Output::info(format!("Installing {} via dnf5...", package.cyan()));
    run_pkexec_status(runner, "/usr/bin/dnf5", &["install", "-y", package])?;
    Ok(())
}

fn overlay_is_unlocked(runner: &dyn CommandRunner) -> Result<bool> {
    let output = runner.run_output(
        "rpm-ostree",
        &["status", "--json"],
        &CommandOptions::default(),
    )?;

    if !output.status.success() {
        bail!("Failed to get rpm-ostree status");
    }

    let json: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let deployments = json
        .get("deployments")
        .and_then(|d| d.as_array())
        .ok_or_else(|| anyhow::anyhow!("No deployments found"))?;

    let booted = deployments
        .iter()
        .find(|d| d.get("booted").and_then(|b| b.as_bool()).unwrap_or(false))
        .ok_or_else(|| anyhow::anyhow!("No booted deployment found"))?;

    let unlocked = booted
        .get("unlocked")
        .and_then(|v| v.as_str())
        .unwrap_or("none");
    Ok(!unlocked.is_empty() && unlocked != "none")
}

fn detect_service_unit(runner: &dyn CommandRunner, package: &str) -> Result<Option<String>> {
    let socket = format!("{}.socket", package);
    let service = format!("{}.service", package);

    let output = runner.run_output(
        "systemctl",
        &[
            "list-unit-files",
            &socket,
            &service,
            "--no-legend",
            "--no-pager",
        ],
        &CommandOptions::default(),
    )?;

    if !output.status.success() {
        return Ok(None);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let unit = line.split_whitespace().next().unwrap_or("");
        if unit == socket || unit == service {
            return Ok(Some(unit.to_string()));
        }
    }

    Ok(None)
}

fn prompt_continue(message: &str) -> Result<bool> {
    use std::io::{Write, stdin, stdout};

    if !stdin().is_terminal() || !stdout().is_terminal() {
        Output::warning("Non-interactive mode detected; skipping prompt.");
        return Ok(false);
    }

    print!("{} [y/N] ", message);
    stdout().flush()?;

    let mut input = String::new();
    stdin().read_line(&mut input)?;
    let input = input.trim().to_lowercase();

    Ok(input == "y" || input == "yes")
}

fn get_download_size_bytes(runner: &dyn CommandRunner, package: &str) -> Result<Option<u64>> {
    let output = runner.run_output(
        "/usr/bin/dnf5",
        &["info", package],
        &CommandOptions::default(),
    )?;

    if !output.status.success() {
        bail!("dnf5 info failed for {}", package);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let trimmed = line.trim();
        if !trimmed.to_lowercase().starts_with("download size") {
            continue;
        }
        let value = trimmed
            .split_once(':')
            .map(|x| x.1)
            .map(str::trim)
            .unwrap_or("");
        if let Some(bytes) = parse_size_bytes(value) {
            return Ok(Some(bytes));
        }
    }

    Ok(None)
}

fn parse_size_bytes(value: &str) -> Option<u64> {
    let mut parts = value.split_whitespace();
    let number = parts.next()?;
    let unit = parts.next().unwrap_or("B");

    let number = number.replace(',', "");
    let value: f64 = number.parse().ok()?;

    let multiplier = match unit.to_ascii_lowercase().as_str() {
        "b" | "bytes" => 1.0,
        "k" | "kb" => 1024.0,
        "m" | "mb" => 1024.0 * 1024.0,
        "g" | "gb" => 1024.0 * 1024.0 * 1024.0,
        "kib" => 1024.0,
        "mib" => 1024.0 * 1024.0,
        "gib" => 1024.0 * 1024.0 * 1024.0,
        _ => return None,
    };

    Some((value * multiplier) as u64)
}

fn sync_containerfile_sections(
    repo_path: &Path,
    manifest: &SystemPackagesManifest,
) -> Result<bool> {
    let containerfile_path = repo_path.join("Containerfile");
    if !containerfile_path.exists() {
        return Ok(false);
    }

    let mut editor = ContainerfileEditor::load(&containerfile_path)?;
    let mut updated_any = false;

    let has_external_rpms = {
        let manifest_path = repo_path.join("manifests").join("external-repos.json");
        if manifest_path.exists() {
            let content = std::fs::read_to_string(&manifest_path)?;
            let ext: crate::manifest::ExternalReposManifest = serde_json::from_str(&content)?;
            !ext.repos.is_empty()
        } else {
            false
        }
    };

    if editor.has_section(Section::SystemPackages) {
        let new_content = generate_system_packages(&manifest.packages, has_external_rpms);
        editor.update_section(Section::SystemPackages, new_content);
        updated_any = true;
    }

    if editor.has_section(Section::CoprRepos) {
        let repo_names: Vec<String> = manifest
            .copr_repos
            .iter()
            .filter(|c| c.enabled)
            .map(|c| c.name.clone())
            .collect();
        let new_content = generate_copr_repos(&repo_names);
        editor.update_section(Section::CoprRepos, new_content);
        updated_any = true;
    }

    if !updated_any {
        return Ok(false);
    }

    editor.write()?;
    Ok(true)
}

fn commit_try_changes(
    runner: &dyn CommandRunner,
    repo_path: &Path,
    branch: &str,
    files: &[PathBuf],
    title: &str,
    body: &str,
) -> Result<()> {
    let options = CommandOptions::with_cwd(repo_path);

    ensure_branch_checked_out(runner, repo_path, branch)?;

    for file in files {
        let file_str = file.to_string_lossy();
        let status = runner
            .run_status("git", &["add", "--", &file_str], &options)
            .context("Failed to stage changes")?;
        if !status.success() {
            bail!("Failed to stage {}", file.display());
        }
    }

    if !git_has_changes(runner, repo_path)? {
        Output::info("No manifest changes to commit.");
        return Ok(());
    }

    let status = runner
        .run_status("git", &["commit", "-m", title], &options)
        .context("Failed to commit changes")?;
    if !status.success() {
        bail!("git commit failed");
    }

    let status = runner
        .run_status("git", &["push", "-u", "origin", branch], &options)
        .context("Failed to push branch")?;
    if !status.success() {
        bail!("git push failed");
    }

    if !pr_exists(runner, repo_path)? {
        let status = runner
            .run_status(
                "gh",
                &["pr", "create", "--title", title, "--body", body],
                &options,
            )
            .context("Failed to create PR")?;
        if !status.success() {
            bail!("gh pr create failed");
        }
    } else {
        let status = runner
            .run_status(
                "gh",
                &["pr", "edit", "--title", title, "--body", body],
                &options,
            )
            .context("Failed to update PR")?;
        if !status.success() {
            bail!("gh pr edit failed");
        }
    }

    let config = RepoConfig::load()?;
    let _ = runner.run_status("git", &["checkout", &config.default_branch], &options);

    Ok(())
}

fn ensure_branch_checked_out(
    runner: &dyn CommandRunner,
    repo_path: &Path,
    branch: &str,
) -> Result<()> {
    let options = CommandOptions::with_cwd(repo_path);

    let status = runner.run_status(
        "git",
        &[
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{}", branch),
        ],
        &options,
    )?;

    if status.success() {
        let status = runner
            .run_status("git", &["checkout", branch], &options)
            .context("Failed to checkout branch")?;
        if !status.success() {
            bail!("Failed to checkout branch {}", branch);
        }
        return Ok(());
    }

    let status = runner
        .run_status("git", &["checkout", "-b", branch], &options)
        .context("Failed to create branch")?;
    if !status.success() {
        bail!("Failed to create branch {}", branch);
    }

    Ok(())
}

fn pr_exists(runner: &dyn CommandRunner, repo_path: &Path) -> Result<bool> {
    let options = CommandOptions::with_cwd(repo_path);
    let output = runner.run_output("gh", &["pr", "view", "--json", "number"], &options);
    Ok(matches!(output, Ok(o) if o.status.success()))
}

fn git_has_changes(runner: &dyn CommandRunner, repo_path: &Path) -> Result<bool> {
    let options = CommandOptions::with_cwd(repo_path);
    let output = runner
        .run_output("git", &["status", "--porcelain"], &options)
        .context("Failed to check git status")?;
    Ok(output.status.success() && !output.stdout.is_empty())
}

fn fetch_pr_number(runner: &dyn CommandRunner, repo_path: &Path) -> Result<Option<u64>> {
    let options = CommandOptions::with_cwd(repo_path);
    let output = runner.run_output(
        "gh",
        &["pr", "view", "--json", "number", "--jq", ".number"],
        &options,
    );

    match output {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            let value = stdout.trim();
            Ok(value.parse::<u64>().ok())
        }
        _ => Ok(None),
    }
}

fn sanitize_branch_component(value: &str) -> String {
    let sanitized: String = value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect();
    if sanitized.is_empty() {
        "try".to_string()
    } else {
        sanitized
    }
}

fn run_pkexec_status(runner: &dyn CommandRunner, program: &str, args: &[&str]) -> Result<()> {
    let mut argv = vec![program];
    argv.extend(args);

    let status = runner
        .run_status("pkexec", &argv, &CommandOptions::default())
        .with_context(|| format!("Failed to execute pkexec {}", program))?;

    if !status.success() {
        let code = status
            .code()
            .map(|c| c.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        bail!("{} failed with exit code {}", program, code);
    }

    Ok(())
}
