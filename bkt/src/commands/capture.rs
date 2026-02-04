//! Capture command - capture system state to manifests.
//!
//! The `bkt capture` command composes multiple capture plans into one and executes them.
//! This is the "system → manifest" direction of bidirectional sync.

use anyhow::Result;
use clap::Args;

use crate::output::Output;
use crate::pipeline::ExecutionPlan;
use crate::plan::{CompositePlan, ExecuteContext, Plan, PlanContext, Plannable};

use super::appimage::{AppImageCaptureCommand, AppImageCapturePlan};
use super::distrobox::{DistroboxCaptureCommand, DistroboxCapturePlan};
use super::extension::{ExtensionCaptureCommand, ExtensionCapturePlan};
use super::flatpak::{FlatpakCaptureCommand, FlatpakCapturePlan};
use super::homebrew::{HomebrewCaptureCommand, HomebrewCapturePlan};
use super::system::{SystemCaptureCommand, SystemCapturePlan};

/// The subsystems that can be captured.
/// Note: gsetting capture requires a schema filter, so it's excluded from the meta-command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum CaptureSubsystem {
    /// GNOME Shell extensions
    Extension,
    /// Distrobox configuration
    Distrobox,
    /// Flatpak applications
    Flatpak,
    /// System packages (rpm-ostree layered)
    System,
    /// AppImage applications (via GearLever)
    AppImage,
    /// Homebrew/Linuxbrew formulae
    Homebrew,
}

impl std::fmt::Display for CaptureSubsystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CaptureSubsystem::Extension => write!(f, "extension"),
            CaptureSubsystem::Distrobox => write!(f, "distrobox"),
            CaptureSubsystem::Flatpak => write!(f, "flatpak"),
            CaptureSubsystem::System => write!(f, "system"),
            CaptureSubsystem::AppImage => write!(f, "appimage"),
            CaptureSubsystem::Homebrew => write!(f, "homebrew"),
        }
    }
}

#[derive(Debug, Args)]
pub struct CaptureArgs {
    /// Only capture specific subsystems (comma-separated)
    #[arg(long, short = 's', value_delimiter = ',')]
    pub only: Option<Vec<CaptureSubsystem>>,

    /// Exclude specific subsystems from capture
    #[arg(long, short = 'x', value_delimiter = ',')]
    pub exclude: Option<Vec<CaptureSubsystem>>,

    /// Apply the plan immediately
    #[arg(long)]
    pub apply: bool,
}

/// Command to capture system state to manifests.
pub struct CaptureCommand {
    /// Subsystems to include (None = all).
    pub include: Option<Vec<CaptureSubsystem>>,
    /// Subsystems to exclude.
    pub exclude: Vec<CaptureSubsystem>,
}

impl CaptureCommand {
    pub fn from_args(args: &CaptureArgs) -> Self {
        Self {
            include: args.only.clone(),
            exclude: args.exclude.clone().unwrap_or_default(),
        }
    }

    fn should_include(&self, subsystem: CaptureSubsystem) -> bool {
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

impl Plannable for CaptureCommand {
    type Plan = CompositePlan;

    fn plan(&self, ctx: &PlanContext) -> Result<Self::Plan> {
        let mut composite = CompositePlan::new("Capture");

        // Extension capture
        if self.should_include(CaptureSubsystem::Extension) {
            let extension_plan: ExtensionCapturePlan = ExtensionCaptureCommand.plan(ctx)?;
            composite.add(extension_plan);
        }

        // Distrobox capture
        if self.should_include(CaptureSubsystem::Distrobox) {
            let distrobox_plan: DistroboxCapturePlan = DistroboxCaptureCommand.plan(ctx)?;
            composite.add(distrobox_plan);
        }

        // Flatpak capture
        if self.should_include(CaptureSubsystem::Flatpak) {
            let flatpak_plan: FlatpakCapturePlan = FlatpakCaptureCommand.plan(ctx)?;
            composite.add(flatpak_plan);
        }

        // System capture (rpm-ostree layered packages)
        if self.should_include(CaptureSubsystem::System) {
            let system_plan: SystemCapturePlan = SystemCaptureCommand.plan(ctx)?;
            composite.add(system_plan);
        }

        // AppImage capture (via GearLever)
        if self.should_include(CaptureSubsystem::AppImage) {
            let appimage_plan: AppImageCapturePlan = AppImageCaptureCommand.plan(ctx)?;
            composite.add(appimage_plan);
        }

        // Homebrew capture
        if self.should_include(CaptureSubsystem::Homebrew) {
            let homebrew_plan: HomebrewCapturePlan = HomebrewCaptureCommand.plan(ctx)?;
            composite.add(homebrew_plan);
        }

        Ok(composite)
    }
}

pub fn run(args: CaptureArgs, exec_plan: &ExecutionPlan) -> Result<()> {
    let cmd = CaptureCommand::from_args(&args);
    let apply = args.apply;

    let cwd = std::env::current_dir()?;
    let plan_ctx = PlanContext::new(cwd, exec_plan.clone());

    let plan = cmd.plan(&plan_ctx)?;

    if plan.is_empty() {
        Output::success("Nothing to capture. All system state is already in manifests.");
        return Ok(());
    }

    // Always show the plan
    print!("{}", plan.describe());

    if exec_plan.dry_run || !apply {
        if !apply {
            Output::hint("Use --apply to execute this plan.");
        }
        return Ok(());
    }

    // Execute the plan
    let mut exec_ctx = ExecuteContext::new(exec_plan.clone());
    let report = plan.execute(&mut exec_ctx)?;
    print!("{}", report);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capture_command_should_include_all_by_default() {
        let cmd = CaptureCommand {
            include: None,
            exclude: vec![],
        };

        assert!(cmd.should_include(CaptureSubsystem::Extension));
        assert!(cmd.should_include(CaptureSubsystem::Distrobox));
        assert!(cmd.should_include(CaptureSubsystem::Flatpak));
        assert!(cmd.should_include(CaptureSubsystem::System));
    }

    #[test]
    fn test_capture_command_only_filter() {
        let cmd = CaptureCommand {
            include: Some(vec![CaptureSubsystem::Extension]),
            exclude: vec![],
        };

        assert!(cmd.should_include(CaptureSubsystem::Extension));
        assert!(!cmd.should_include(CaptureSubsystem::Distrobox));
        assert!(!cmd.should_include(CaptureSubsystem::Flatpak));
        assert!(!cmd.should_include(CaptureSubsystem::System));
    }

    #[test]
    fn test_capture_command_exclude_filter() {
        let cmd = CaptureCommand {
            include: None,
            exclude: vec![CaptureSubsystem::Extension],
        };

        assert!(!cmd.should_include(CaptureSubsystem::Extension));
        assert!(cmd.should_include(CaptureSubsystem::Distrobox));
        assert!(cmd.should_include(CaptureSubsystem::Flatpak));
        assert!(cmd.should_include(CaptureSubsystem::System));
    }

    #[test]
    fn test_capture_command_exclude_overrides_include() {
        let cmd = CaptureCommand {
            include: Some(vec![CaptureSubsystem::Extension, CaptureSubsystem::Flatpak]),
            exclude: vec![CaptureSubsystem::Flatpak],
        };

        assert!(cmd.should_include(CaptureSubsystem::Extension));
        assert!(!cmd.should_include(CaptureSubsystem::Flatpak)); // excluded wins
    }

    #[test]
    fn test_capture_subsystem_display() {
        assert_eq!(format!("{}", CaptureSubsystem::Extension), "extension");
        assert_eq!(format!("{}", CaptureSubsystem::Distrobox), "distrobox");
        assert_eq!(format!("{}", CaptureSubsystem::Flatpak), "flatpak");
        assert_eq!(format!("{}", CaptureSubsystem::System), "system");
    }

    #[test]
    fn test_capture_command_from_args() {
        let args = CaptureArgs {
            only: Some(vec![CaptureSubsystem::Extension]),
            exclude: Some(vec![CaptureSubsystem::Flatpak]),
            apply: false,
        };

        let cmd = CaptureCommand::from_args(&args);
        assert_eq!(cmd.include, Some(vec![CaptureSubsystem::Extension]));
        assert_eq!(cmd.exclude, vec![CaptureSubsystem::Flatpak]);
    }
}
