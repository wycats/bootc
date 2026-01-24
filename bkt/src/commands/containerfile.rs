//! Containerfile management command implementation.
//!
//! Provides commands to sync and check managed sections in the Containerfile.
//! Managed sections are auto-generated from manifests and delimited by
//! special marker comments.
//!
//! This implementation follows the Plan/Execute pattern from RFC-0008:
//! - Planning phase: Analyze current state, detect drift, produce immutable plan
//! - Execution phase: Apply updates to the Containerfile

use crate::containerfile::{
    ContainerfileEditor, Section, generate_copr_repos, generate_host_shims,
    generate_kernel_arguments, generate_system_packages, generate_systemd_units,
};
use crate::manifest::system_config::SystemConfigManifest;
use crate::manifest::{ShimsManifest, SystemPackagesManifest};
use crate::output::Output;
use crate::pipeline::ExecutionPlan;
use crate::plan::{
    ExecuteContext, ExecutionReport, Operation, Plan, PlanContext, PlanSummary, PlanWarning,
    Plannable, Verb,
};
use anyhow::Result;
use clap::{Args, Subcommand};
use std::path::{Path, PathBuf};

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

// ============================================================================
// Plan-based Implementation
// ============================================================================

/// Command to sync all managed sections in the Containerfile.
pub struct ContainerfileSyncCommand;

/// Description of a section update operation.
#[derive(Debug, Clone)]
pub struct SectionUpdate {
    /// The section being updated.
    pub section: Section,
    /// The new content to write.
    pub new_content: Vec<String>,
    /// The old content (retained for future diff display).
    #[allow(dead_code)]
    pub old_content: Option<Vec<String>>,
    /// Whether this represents a drift from current state.
    pub is_drift: bool,
}

/// A warning about a missing or problematic section.
#[derive(Debug, Clone)]
pub struct SectionWarning {
    /// The section this warning relates to.
    pub section: Section,
    /// The warning message.
    pub message: String,
}

impl std::fmt::Display for SectionWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.section.marker_name(), self.message)
    }
}

/// Plan for syncing managed sections in the Containerfile.
pub struct ContainerfileSyncPlan {
    /// Path to the Containerfile.
    pub containerfile_path: PathBuf,
    /// Sections that need to be updated.
    pub section_updates: Vec<SectionUpdate>,
    /// Warnings for missing or problematic sections.
    pub warnings: Vec<SectionWarning>,
}

impl Plannable for ContainerfileSyncCommand {
    type Plan = ContainerfileSyncPlan;

    fn plan(&self, _ctx: &PlanContext) -> Result<Self::Plan> {
        let containerfile_path = Path::new("Containerfile").to_path_buf();

        if !containerfile_path.exists() {
            // Return an empty plan with a warning if no Containerfile exists
            return Ok(ContainerfileSyncPlan {
                containerfile_path,
                section_updates: Vec::new(),
                warnings: vec![SectionWarning {
                    section: Section::SystemPackages,
                    message: "No Containerfile found in current directory".to_string(),
                }],
            });
        }

        // Load the Containerfile (read-only)
        let editor = ContainerfileEditor::load(&containerfile_path)?;

        // Load manifests (read-only)
        let manifest = load_merged_manifest()?;
        let system_config = SystemConfigManifest::load()?;
        let shims_manifest = load_merged_shims_manifest()?;

        let mut section_updates = Vec::new();
        let mut warnings = Vec::new();

        // Check all sections and determine what needs updating
        check_section(
            &editor,
            Section::SystemPackages,
            generate_system_packages(&manifest.packages),
            true,
            &mut section_updates,
            &mut warnings,
        );

        check_section(
            &editor,
            Section::KernelArguments,
            generate_kernel_arguments(&system_config),
            true,
            &mut section_updates,
            &mut warnings,
        );

        check_section(
            &editor,
            Section::SystemdUnits,
            generate_systemd_units(&system_config),
            true,
            &mut section_updates,
            &mut warnings,
        );

        let repo_names: Vec<String> = manifest
            .copr_repos
            .iter()
            .filter(|c| c.enabled)
            .map(|c| c.name.clone())
            .collect();
        check_section(
            &editor,
            Section::CoprRepos,
            generate_copr_repos(&repo_names),
            true,
            &mut section_updates,
            &mut warnings,
        );

        // HOST_SHIMS is optional - don't warn if missing
        check_section(
            &editor,
            Section::HostShims,
            generate_host_shims(&shims_manifest.shims),
            false, // Don't warn if missing
            &mut section_updates,
            &mut warnings,
        );

        Ok(ContainerfileSyncPlan {
            containerfile_path,
            section_updates,
            warnings,
        })
    }
}

/// Helper to check a section and add updates/warnings as needed.
fn check_section(
    editor: &ContainerfileEditor,
    section: Section,
    new_content: Vec<String>,
    warn_if_missing: bool,
    updates: &mut Vec<SectionUpdate>,
    warnings: &mut Vec<SectionWarning>,
) {
    if editor.has_section(section) {
        let old_content = editor.get_section_content(section).map(|c| c.to_vec());
        let is_drift = match &old_content {
            Some(old) => old.as_slice() != new_content.as_slice(),
            None => true,
        };

        updates.push(SectionUpdate {
            section,
            new_content,
            old_content,
            is_drift,
        });
    } else if warn_if_missing {
        warnings.push(SectionWarning {
            section,
            message: format!(
                "Section {} not found in Containerfile",
                section.marker_name()
            ),
        });
    }
}

impl Plan for ContainerfileSyncPlan {
    fn describe(&self) -> PlanSummary {
        let drift_count = self.section_updates.iter().filter(|u| u.is_drift).count();
        let summary_text = if drift_count > 0 {
            format!("Containerfile Sync: {} section(s) with drift", drift_count)
        } else {
            "Containerfile Sync: all sections up to date".to_string()
        };

        let mut summary = PlanSummary::new(summary_text);

        // Add operations for each section
        for update in &self.section_updates {
            if update.is_drift {
                summary.add_operation(Operation::new(
                    Verb::Update,
                    format!("section:{}", update.section.marker_name()),
                ));
            } else {
                summary.add_operation(Operation::new(
                    Verb::Skip,
                    format!("section:{}", update.section.marker_name()),
                ));
            }
        }

        // Add warnings for missing sections
        for warning in &self.warnings {
            summary.add_warning(PlanWarning::new(
                format!("section:{}", warning.section.marker_name()),
                warning.message.clone(),
            ));
        }

        summary
    }

    fn execute(self, ctx: &mut ExecuteContext) -> Result<ExecutionReport> {
        let mut report = ExecutionReport::new();

        // If there's a warning about missing Containerfile, don't proceed
        if self
            .warnings
            .iter()
            .any(|w| w.message.contains("No Containerfile found"))
        {
            report.record_failure_and_notify(
                ctx,
                Verb::Update,
                "Containerfile",
                "No Containerfile found in current directory",
            );
            return Ok(report);
        }

        // Load the editor for writing
        let mut editor = ContainerfileEditor::load(&self.containerfile_path)?;

        let mut any_updates = false;

        // Apply all updates
        for update in &self.section_updates {
            if update.is_drift {
                editor.update_section(update.section, update.new_content.clone());
                report.record_success_and_notify(
                    ctx,
                    Verb::Update,
                    format!("section:{}", update.section.marker_name()),
                );
                any_updates = true;
            }
        }

        // Write changes if any were made
        if any_updates {
            editor.write()?;
        }

        Ok(report)
    }

    fn is_empty(&self) -> bool {
        !self.section_updates.iter().any(|update| update.is_drift)
    }
}

impl ContainerfileSyncPlan {
    /// Check if any section has drift.
    pub fn has_drift(&self) -> bool {
        self.section_updates.iter().any(|u| u.is_drift)
    }
}

// ============================================================================
// Command Entry Point
// ============================================================================

pub fn run(args: ContainerfileArgs, plan: &ExecutionPlan) -> Result<()> {
    match args.action {
        ContainerfileAction::Sync => {
            let cwd = std::env::current_dir()?;
            let plan_ctx = PlanContext::new(cwd, plan.clone());

            let sync_plan = ContainerfileSyncCommand.plan(&plan_ctx)?;

            // Check for fatal errors (no Containerfile)
            if sync_plan
                .warnings
                .iter()
                .any(|w| w.message.contains("No Containerfile found"))
            {
                Output::error("No Containerfile found in current directory");
                return Ok(());
            }

            if sync_plan.is_empty() && sync_plan.warnings.is_empty() {
                Output::success("All managed sections are already in sync.");
                return Ok(());
            }

            // Always show the plan
            Output::header("Containerfile Sync");
            Output::blank();
            for op in &sync_plan.describe().operations {
                println!("  {op}");
            }
            for warning in &sync_plan.warnings {
                Output::warning(format!("  {warning}"));
            }

            if plan.dry_run {
                Output::blank();
                Output::info("Dry-run mode: no changes will be made");
                return Ok(());
            }

            if !sync_plan.has_drift() {
                Output::blank();
                Output::success("All managed sections are already in sync.");
                return Ok(());
            }

            // Execute the plan
            let mut exec_ctx = ExecuteContext::new(plan.clone());
            let _report = sync_plan.execute(&mut exec_ctx)?;

            Output::blank();
            Output::success("Containerfile updated");

            Ok(())
        }
        ContainerfileAction::Check => {
            // Check is just planning without execution
            let cwd = std::env::current_dir()?;
            let plan_ctx = PlanContext::new(cwd, plan.clone());

            let sync_plan = ContainerfileSyncCommand.plan(&plan_ctx)?;

            // Check for fatal errors
            if sync_plan
                .warnings
                .iter()
                .any(|w| w.message.contains("No Containerfile found"))
            {
                Output::error("No Containerfile found in current directory");
                return Ok(());
            }

            Output::header("Checking Containerfile managed sections for drift");
            Output::blank();

            // Show each section status
            for update in &sync_plan.section_updates {
                if update.is_drift {
                    Output::warning(format!("{}: DRIFT DETECTED", update.section.marker_name()));
                } else {
                    Output::success(format!("{}: up to date", update.section.marker_name()));
                }
            }

            // Show warnings for missing sections
            for warning in &sync_plan.warnings {
                Output::warning(format!(
                    "{}: {}",
                    warning.section.marker_name(),
                    warning.message
                ));
            }

            Output::blank();
            if sync_plan.has_drift() {
                Output::warning("Drift detected. Run `bkt containerfile sync` to update.");
            } else {
                Output::success("All managed sections are in sync with manifests.");
            }

            Ok(())
        }
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Load the merged system packages manifest (repo + user)
fn load_merged_manifest() -> Result<SystemPackagesManifest> {
    let repo = SystemPackagesManifest::load_repo()?;
    let user = SystemPackagesManifest::load_user()?;
    Ok(SystemPackagesManifest::merged(&repo, &user))
}

/// Load the merged shims manifest (repo + user)
fn load_merged_shims_manifest() -> Result<ShimsManifest> {
    let repo = ShimsManifest::load_repo()?;
    let user = ShimsManifest::load_user()?;
    Ok(ShimsManifest::merged(&repo, &user))
}
