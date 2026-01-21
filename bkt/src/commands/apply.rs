//! Apply command - sync all manifests to the running system.
//!
//! The `bkt apply` command composes multiple sync plans into one and executes them.
//! This is the "manifest → system" direction of bidirectional sync.

use anyhow::{Context, Result};
use clap::Args;

use crate::output::Output;
use crate::pipeline::ExecutionPlan;
use crate::plan::{CompositePlan, ExecuteContext, OperationProgress, Plan, PlanContext, Plannable};

use super::appimage::{AppImageSyncCommand, AppImageSyncPlan};
use super::distrobox::{DistroboxSyncCommand, DistroboxSyncPlan};
use super::dnf::{DnfSyncCommand, DnfSyncPlan};
use super::extension::{ExtensionSyncCommand, ExtensionSyncPlan};
use super::flatpak::{FlatpakSyncCommand, FlatpakSyncPlan};
use super::gsetting::{GsettingApplyCommand, GsettingApplyPlan};
use super::shim::{ShimSyncCommand, ShimSyncPlan};

/// The subsystems that can be synced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Subsystem {
    /// Host shims for toolbox commands
    Shim,
    /// Distrobox configuration
    Distrobox,
    /// GSettings values
    Gsetting,
    /// GNOME Shell extensions
    Extension,
    /// Flatpak applications
    Flatpak,
    /// DNF/rpm-ostree packages
    Dnf,
    /// AppImages via GearLever
    AppImage,
}

impl std::fmt::Display for Subsystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Subsystem::Shim => write!(f, "shim"),
            Subsystem::Distrobox => write!(f, "distrobox"),
            Subsystem::Gsetting => write!(f, "gsetting"),
            Subsystem::Extension => write!(f, "extension"),
            Subsystem::Flatpak => write!(f, "flatpak"),
            Subsystem::Dnf => write!(f, "dnf"),
            Subsystem::AppImage => write!(f, "appimage"),
        }
    }
}

#[derive(Debug, Args)]
pub struct ApplyArgs {
    /// Only sync specific subsystems (comma-separated)
    #[arg(long, short = 's', value_delimiter = ',')]
    pub only: Option<Vec<Subsystem>>,

    /// Exclude specific subsystems from sync
    #[arg(long, short = 'x', value_delimiter = ',')]
    pub exclude: Option<Vec<Subsystem>>,

    /// Apply changes without prompting for confirmation
    #[arg(long)]
    pub confirm: bool,

    /// Prune unmanaged AppImages (default is to keep them)
    #[arg(long)]
    pub prune_appimages: bool,
}

/// Command to apply all manifests to the system.
pub struct ApplyCommand {
    /// Subsystems to include (None = all).
    pub include: Option<Vec<Subsystem>>,
    /// Subsystems to exclude.
    pub exclude: Vec<Subsystem>,

    /// Whether to prune unmanaged AppImages.
    pub prune_appimages: bool,
}

impl ApplyCommand {
    /// Create from CLI args.
    pub fn from_args(args: &ApplyArgs) -> Self {
        Self {
            include: args.only.clone(),
            exclude: args.exclude.clone().unwrap_or_default(),
            prune_appimages: args.prune_appimages,
        }
    }

    /// Check if a subsystem should be included.
    fn should_include(&self, subsystem: Subsystem) -> bool {
        // If exclude list contains it, skip
        if self.exclude.contains(&subsystem) {
            return false;
        }
        // If include list is specified, only include those
        if let Some(ref include) = self.include {
            return include.contains(&subsystem);
        }
        // Otherwise, include all
        true
    }
}

impl Plannable for ApplyCommand {
    type Plan = CompositePlan;

    fn plan(&self, ctx: &PlanContext) -> Result<Self::Plan> {
        let mut composite = CompositePlan::new("Apply");

        // Shim sync
        if self.should_include(Subsystem::Shim) {
            let shim_plan: ShimSyncPlan = ShimSyncCommand.plan(ctx)?;
            composite.add(shim_plan);
        }

        // Distrobox sync
        if self.should_include(Subsystem::Distrobox) {
            let distrobox_plan: DistroboxSyncPlan = DistroboxSyncCommand.plan(ctx)?;
            composite.add(distrobox_plan);
        }

        // GSettings apply
        if self.should_include(Subsystem::Gsetting) {
            let gsetting_plan: GsettingApplyPlan = GsettingApplyCommand.plan(ctx)?;
            composite.add(gsetting_plan);
        }

        // Extension sync
        if self.should_include(Subsystem::Extension) {
            let extension_plan: ExtensionSyncPlan = ExtensionSyncCommand.plan(ctx)?;
            composite.add(extension_plan);
        }

        // Flatpak sync
        if self.should_include(Subsystem::Flatpak) {
            let flatpak_plan: FlatpakSyncPlan = FlatpakSyncCommand.plan(ctx)?;
            composite.add(flatpak_plan);
        }

        // DNF sync
        if self.should_include(Subsystem::Dnf) {
            let dnf_plan: DnfSyncPlan = DnfSyncCommand {
                now: false, // Default to reboot-required mode for apply
                context: ctx.execution_plan().context,
            }
            .plan(ctx)?;
            composite.add(dnf_plan);
        }

        // AppImage sync via GearLever
        if self.should_include(Subsystem::AppImage) {
            let appimage_plan: AppImageSyncPlan = AppImageSyncCommand {
                keep_unmanaged: !self.prune_appimages,
            }
            .plan(ctx)?;
            composite.add(appimage_plan);
        }

        Ok(composite)
    }
}

pub fn run(args: ApplyArgs, exec_plan: &ExecutionPlan) -> Result<()> {
    let cmd = ApplyCommand::from_args(&args);

    let cwd = std::env::current_dir()?;
    let plan_ctx = PlanContext::new(cwd, exec_plan.clone());

    let plan = cmd.plan(&plan_ctx)?;

    if plan.is_empty() {
        Output::success("Nothing to apply. System is in sync with manifests.");
        return Ok(());
    }

    // Always show the plan
    let summary = plan.describe();
    print!("{}", summary);

    if exec_plan.dry_run {
        Output::info("Run without --dry-run to apply these changes.");
        return Ok(());
    }

    if !args.confirm {
        let confirmed = cliclack::confirm("Apply these changes?")
            .initial_value(false)
            .interact()
            .context("Failed to read confirmation")?;
        if !confirmed {
            Output::info("Cancelled.");
            return Ok(());
        }
    }

    // Print hint that we're executing
    Output::info("Applying changes...");
    println!();

    // Execute the plan with progress tracking
    let total_ops = summary.action_count();
    let mut exec_ctx = ExecuteContext::new(exec_plan.clone());
    exec_ctx.set_total_ops(total_ops);
    exec_ctx.set_progress_callback(print_progress);

    let report = plan.execute(&mut exec_ctx)?;

    // Print final summary (only failures, since progress showed successes)
    println!();
    print!("{}", report);

    Ok(())
}

/// Print progress for a single operation.
pub(crate) fn print_progress(progress: &OperationProgress) {
    use owo_colors::OwoColorize;

    let index_str = format!("[{}/{}]", progress.current, progress.total);
    let result = &progress.result;

    if result.success {
        println!(
            "{} {} {}",
            index_str.dimmed(),
            "✓".green().bold(),
            result.operation
        );
    } else {
        println!(
            "{} {} {}",
            index_str.dimmed(),
            "✗".red().bold(),
            result.operation
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apply_command_should_include_all_by_default() {
        let cmd = ApplyCommand {
            include: None,
            exclude: vec![],
            prune_appimages: false,
        };

        assert!(cmd.should_include(Subsystem::Shim));
        assert!(cmd.should_include(Subsystem::Distrobox));
        assert!(cmd.should_include(Subsystem::Gsetting));
        assert!(cmd.should_include(Subsystem::Extension));
        assert!(cmd.should_include(Subsystem::Flatpak));
        assert!(cmd.should_include(Subsystem::Dnf));
    }

    #[test]
    fn test_apply_command_only_filter() {
        let cmd = ApplyCommand {
            include: Some(vec![Subsystem::Shim, Subsystem::Dnf]),
            exclude: vec![],
            prune_appimages: false,
        };

        assert!(cmd.should_include(Subsystem::Shim));
        assert!(!cmd.should_include(Subsystem::Gsetting));
        assert!(!cmd.should_include(Subsystem::Extension));
        assert!(!cmd.should_include(Subsystem::Flatpak));
        assert!(cmd.should_include(Subsystem::Dnf));
    }

    #[test]
    fn test_apply_command_exclude_filter() {
        let cmd = ApplyCommand {
            include: None,
            exclude: vec![Subsystem::Extension, Subsystem::Flatpak],
            prune_appimages: false,
        };

        assert!(cmd.should_include(Subsystem::Shim));
        assert!(cmd.should_include(Subsystem::Distrobox));
        assert!(cmd.should_include(Subsystem::Gsetting));
        assert!(!cmd.should_include(Subsystem::Extension));
        assert!(!cmd.should_include(Subsystem::Flatpak));
        assert!(cmd.should_include(Subsystem::Dnf));
    }

    #[test]
    fn test_apply_command_exclude_overrides_include() {
        let cmd = ApplyCommand {
            include: Some(vec![Subsystem::Shim, Subsystem::Extension]),
            exclude: vec![Subsystem::Extension],
            prune_appimages: false,
        };

        assert!(cmd.should_include(Subsystem::Shim));
        assert!(!cmd.should_include(Subsystem::Extension)); // excluded wins
    }

    #[test]
    fn test_subsystem_display() {
        assert_eq!(format!("{}", Subsystem::Shim), "shim");
        assert_eq!(format!("{}", Subsystem::Distrobox), "distrobox");
        assert_eq!(format!("{}", Subsystem::Gsetting), "gsetting");
        assert_eq!(format!("{}", Subsystem::Extension), "extension");
        assert_eq!(format!("{}", Subsystem::Flatpak), "flatpak");
        assert_eq!(format!("{}", Subsystem::Dnf), "dnf");
    }

    #[test]
    fn test_apply_command_from_args() {
        let args = ApplyArgs {
            only: Some(vec![Subsystem::Shim]),
            exclude: Some(vec![Subsystem::Dnf]),
            confirm: true,
            prune_appimages: true,
        };

        let cmd = ApplyCommand::from_args(&args);
        assert_eq!(cmd.include, Some(vec![Subsystem::Shim]));
        assert_eq!(cmd.exclude, vec![Subsystem::Dnf]);
        assert!(cmd.prune_appimages);
    }
}
