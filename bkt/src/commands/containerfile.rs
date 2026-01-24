//! Containerfile management command implementation.
//!
//! Provides commands to sync and check managed sections in the Containerfile.
//! Managed sections are auto-generated from manifests and delimited by
//! special marker comments.

use crate::containerfile::{
    ContainerfileEditor, Section, generate_copr_repos, generate_host_shims,
    generate_kernel_arguments, generate_system_packages, generate_systemd_units,
};
use crate::manifest::system_config::SystemConfigManifest;
use crate::manifest::{ShimsManifest, SystemPackagesManifest};
use crate::output::Output;
use crate::pipeline::ExecutionPlan;
use anyhow::Result;
use clap::{Args, Subcommand};
use std::path::Path;

#[derive(Debug, Args)]
pub struct ContainerfileArgs {
    #[command(subcommand)]
    pub action: ContainerfileAction,
}

#[derive(Debug, Subcommand)]
pub enum ContainerfileAction {
    /// Regenerate all managed sections from manifests
    Sync,
    /// Check for drift between manifests and Containerfile (dry-run)
    Check,
}

/// Result of syncing a single section
#[derive(Debug)]
struct SectionSyncResult {
    section_name: String,
    changed: bool,
    message: String,
}

pub fn run(args: ContainerfileArgs, plan: &ExecutionPlan) -> Result<()> {
    match args.action {
        ContainerfileAction::Sync => {
            if plan.dry_run {
                // In dry-run mode, sync behaves like check
                Output::info("Dry-run mode: checking for drift instead of syncing");
                Output::blank();
                run_check()
            } else {
                run_sync()
            }
        }
        ContainerfileAction::Check => run_check(),
    }
}

/// Sync all managed sections in the Containerfile
fn run_sync() -> Result<()> {
    let containerfile_path = Path::new("Containerfile");
    if !containerfile_path.exists() {
        Output::error("No Containerfile found in current directory");
        return Ok(());
    }

    Output::header("Syncing Containerfile managed sections");
    Output::blank();

    let mut editor = ContainerfileEditor::load(containerfile_path)?;
    let manifest = load_merged_manifest()?;
    let system_config = SystemConfigManifest::load()?;

    let mut any_changes = false;

    // Sync SYSTEM_PACKAGES section
    if editor.has_section(Section::SystemPackages) {
        let new_content = generate_system_packages(&manifest.packages);
        editor.update_section(Section::SystemPackages, new_content);
        Output::success("Synced SYSTEM_PACKAGES section");
        any_changes = true;
    } else {
        Output::warning("No SYSTEM_PACKAGES section found - skipping");
    }

    // Sync KERNEL_ARGUMENTS section
    if editor.has_section(Section::KernelArguments) {
        let new_content = generate_kernel_arguments(&system_config);
        editor.update_section(Section::KernelArguments, new_content);
        Output::success("Synced KERNEL_ARGUMENTS section");
        any_changes = true;
    } else {
        Output::warning("No KERNEL_ARGUMENTS section found - skipping");
    }

    // Sync SYSTEMD_UNITS section
    if editor.has_section(Section::SystemdUnits) {
        let new_content = generate_systemd_units(&system_config);
        editor.update_section(Section::SystemdUnits, new_content);
        Output::success("Synced SYSTEMD_UNITS section");
        any_changes = true;
    } else {
        Output::warning("No SYSTEMD_UNITS section found - skipping");
    }

    // Sync COPR_REPOS section
    if editor.has_section(Section::CoprRepos) {
        let repo_names: Vec<String> = manifest
            .copr_repos
            .iter()
            .filter(|c| c.enabled)
            .map(|c| c.name.clone())
            .collect();
        let new_content = generate_copr_repos(&repo_names);
        editor.update_section(Section::CoprRepos, new_content);
        Output::success("Synced COPR_REPOS section");
        any_changes = true;
    } else {
        Output::warning("No COPR_REPOS section found - skipping");
    }

    // Sync HOST_SHIMS section
    if editor.has_section(Section::HostShims) {
        let shims_manifest = load_merged_shims_manifest()?;
        let new_content = generate_host_shims(&shims_manifest.shims);
        editor.update_section(Section::HostShims, new_content);
        Output::success("Synced HOST_SHIMS section");
        any_changes = true;
    }
    // Note: No warning if HOST_SHIMS section is missing - it's optional

    // Write changes
    if any_changes {
        editor.write()?;
        Output::blank();
        Output::success("Containerfile updated");
    } else {
        Output::blank();
        Output::info("No managed sections found to sync");
    }

    Ok(())
}

/// Check for drift between manifests and Containerfile without modifying
fn run_check() -> Result<()> {
    let containerfile_path = Path::new("Containerfile");
    if !containerfile_path.exists() {
        Output::error("No Containerfile found in current directory");
        return Ok(());
    }

    Output::header("Checking Containerfile managed sections for drift");
    Output::blank();

    let editor = ContainerfileEditor::load(containerfile_path)?;
    let manifest = load_merged_manifest()?;
    let system_config = SystemConfigManifest::load()?;

    let mut has_drift = false;
    let mut results: Vec<SectionSyncResult> = Vec::new();

    // Check COPR_REPOS section
    if editor.has_section(Section::CoprRepos) {
        let repo_names: Vec<String> = manifest
            .copr_repos
            .iter()
            .filter(|c| c.enabled)
            .map(|c| c.name.clone())
            .collect();
        let expected = generate_copr_repos(&repo_names);
        let current = editor.get_section_content(Section::CoprRepos);

        let changed = match current {
            Some(c) => c != expected.as_slice(),
            None => true,
        };

        if changed {
            has_drift = true;
        }

        results.push(SectionSyncResult {
            section_name: Section::CoprRepos.marker_name().to_string(),
            changed,
            message: if changed {
                format!("{} COPR repos in manifest", repo_names.len())
            } else {
                "up to date".to_string()
            },
        });
    }

    // Check SYSTEM_PACKAGES section
    if editor.has_section(Section::SystemPackages) {
        let expected = generate_system_packages(&manifest.packages);
        let current = editor.get_section_content(Section::SystemPackages);

        let changed = match current {
            Some(c) => c != expected.as_slice(),
            None => true,
        };

        if changed {
            has_drift = true;
        }

        results.push(SectionSyncResult {
            section_name: Section::SystemPackages.marker_name().to_string(),
            changed,
            message: if changed {
                format!("{} packages in manifest", manifest.packages.len())
            } else {
                "up to date".to_string()
            },
        });
    }

    // Check KERNEL_ARGUMENTS section
    if editor.has_section(Section::KernelArguments) {
        let expected = generate_kernel_arguments(&system_config);
        let current = editor.get_section_content(Section::KernelArguments);

        let changed = match current {
            Some(c) => c != expected.as_slice(),
            None => true,
        };

        if changed {
            has_drift = true;
        }

        results.push(SectionSyncResult {
            section_name: Section::KernelArguments.marker_name().to_string(),
            changed,
            message: if changed {
                "drift detected".to_string()
            } else {
                "up to date".to_string()
            },
        });
    }

    // Check SYSTEMD_UNITS section
    if editor.has_section(Section::SystemdUnits) {
        let expected = generate_systemd_units(&system_config);
        let current = editor.get_section_content(Section::SystemdUnits);

        let changed = match current {
            Some(c) => c != expected.as_slice(),
            None => true,
        };

        if changed {
            has_drift = true;
        }

        results.push(SectionSyncResult {
            section_name: Section::SystemdUnits.marker_name().to_string(),
            changed,
            message: if changed {
                "drift detected".to_string()
            } else {
                "up to date".to_string()
            },
        });
    }

    // Check HOST_SHIMS section
    if editor.has_section(Section::HostShims) {
        let shims_manifest = load_merged_shims_manifest()?;
        let expected = generate_host_shims(&shims_manifest.shims);
        let current = editor.get_section_content(Section::HostShims);

        let changed = match current {
            Some(c) => c != expected.as_slice(),
            None => true,
        };

        if changed {
            has_drift = true;
        }

        results.push(SectionSyncResult {
            section_name: Section::HostShims.marker_name().to_string(),
            changed,
            message: if changed {
                format!("{} shims in manifest", shims_manifest.shims.len())
            } else {
                "up to date".to_string()
            },
        });
    }

    // Display results
    for result in &results {
        if result.changed {
            Output::warning(format!(
                "{}: DRIFT DETECTED ({})",
                result.section_name, result.message
            ));
        } else {
            Output::success(format!("{}: {}", result.section_name, result.message));
        }
    }

    Output::blank();
    if has_drift {
        Output::warning("Drift detected. Run `bkt containerfile sync` to update.");
    } else {
        Output::success("All managed sections are in sync with manifests.");
    }

    Ok(())
}

/// Load the merged system packages manifest (repo + user)
///
/// For containerfile generation, we load from the repo's manifests directory
/// rather than the system path, since we're generating the Containerfile
/// that will install the packages into the image.
fn load_merged_manifest() -> Result<SystemPackagesManifest> {
    let repo = SystemPackagesManifest::load_repo()?;
    let user = SystemPackagesManifest::load_user()?;
    Ok(SystemPackagesManifest::merged(&repo, &user))
}

/// Load the merged shims manifest (repo + user)
///
/// For containerfile generation, we load from the repo's manifests directory
/// rather than the system path, since we're generating the Containerfile
/// that will install the shims into the image.
fn load_merged_shims_manifest() -> Result<ShimsManifest> {
    let repo = ShimsManifest::load_repo()?;
    let user = ShimsManifest::load_user()?;
    Ok(ShimsManifest::merged(&repo, &user))
}
