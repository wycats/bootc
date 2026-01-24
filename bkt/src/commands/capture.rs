//! Capture command - capture system state to manifests.
//!
//! The `bkt capture` command composes multiple capture plans into one and executes them.
//! This is the "system → manifest" direction of bidirectional sync.

use anyhow::Result;
use clap::Args;

use crate::output::Output;
use crate::pipeline::ExecutionPlan;
use crate::plan::{CompositePlan, ExecuteContext, Plan, PlanContext, Plannable};
use crate::subsystem::SubsystemRegistry;

fn parse_capturable_subsystem(value: &str) -> Result<String, String> {
    let registry = SubsystemRegistry::builtin();
    let normalized = value.replace('-', "");
    if registry.is_valid_capturable(&normalized) {
        Ok(normalized)
    } else {
        let valid = registry.capturable_ids();
        Err(format!(
            "Unknown subsystem '{}'. Valid options: {}",
            value,
            valid.join(", ")
        ))
    }
}

#[derive(Debug, Args)]
pub struct CaptureArgs {
    /// Only capture specific subsystems (comma-separated)
    #[arg(long, short = 's', value_delimiter = ',', value_parser = parse_capturable_subsystem)]
    pub only: Option<Vec<String>>,

    /// Exclude specific subsystems from capture
    #[arg(long, short = 'x', value_delimiter = ',', value_parser = parse_capturable_subsystem)]
    pub exclude: Option<Vec<String>>,

    /// Apply the plan immediately
    #[arg(long)]
    pub apply: bool,
}

/// Command to capture system state to manifests.
pub struct CaptureCommand {
    /// Subsystems to include (None = all).
    pub include: Option<Vec<String>>,
    /// Subsystems to exclude.
    pub exclude: Vec<String>,
}

impl CaptureCommand {
    /// Create from CLI args.
    pub fn from_args(args: &CaptureArgs) -> Self {
        Self {
            include: args.only.clone(),
            exclude: args.exclude.clone().unwrap_or_default(),
        }
    }

    /// Check if a subsystem should be included by ID string.
    fn should_include_id(&self, id: &str) -> bool {
        // If exclude list contains it, skip
        if self.exclude.iter().any(|e| e == id) {
            return false;
        }
        // If include list is specified, only include those
        if let Some(ref include) = self.include {
            return include.iter().any(|i| i == id);
        }
        // Otherwise, include all
        true
    }
}

impl Plannable for CaptureCommand {
    type Plan = CompositePlan;

    fn plan(&self, ctx: &PlanContext) -> Result<Self::Plan> {
        let mut composite = CompositePlan::new("Capture");
        let registry = SubsystemRegistry::builtin();

        for subsystem in registry.capturable() {
            if self.should_include_id(subsystem.id())
                && let Some(plan) = subsystem.capture(ctx)?
            {
                composite.add_boxed(plan);
            }
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

        assert!(cmd.should_include_id("extension"));
        assert!(cmd.should_include_id("distrobox"));
        assert!(cmd.should_include_id("flatpak"));
        assert!(cmd.should_include_id("system"));
    }

    #[test]
    fn test_capture_command_only_filter() {
        let cmd = CaptureCommand {
            include: Some(vec!["extension".to_string()]),
            exclude: vec![],
        };

        assert!(cmd.should_include_id("extension"));
        assert!(!cmd.should_include_id("distrobox"));
        assert!(!cmd.should_include_id("flatpak"));
        assert!(!cmd.should_include_id("system"));
    }

    #[test]
    fn test_capture_command_exclude_filter() {
        let cmd = CaptureCommand {
            include: None,
            exclude: vec!["extension".to_string()],
        };

        assert!(!cmd.should_include_id("extension"));
        assert!(cmd.should_include_id("distrobox"));
        assert!(cmd.should_include_id("flatpak"));
        assert!(cmd.should_include_id("system"));
    }

    #[test]
    fn test_capture_command_exclude_overrides_include() {
        let cmd = CaptureCommand {
            include: Some(vec!["extension".to_string(), "flatpak".to_string()]),
            exclude: vec!["flatpak".to_string()],
        };

        assert!(cmd.should_include_id("extension"));
        assert!(!cmd.should_include_id("flatpak")); // excluded wins
    }

    #[test]
    fn test_capture_command_from_args() {
        let args = CaptureArgs {
            only: Some(vec!["extension".to_string()]),
            exclude: Some(vec!["flatpak".to_string()]),
            apply: false,
        };

        let cmd = CaptureCommand::from_args(&args);
        assert_eq!(cmd.include, Some(vec!["extension".to_string()]));
        assert_eq!(cmd.exclude, vec!["flatpak".to_string()]);
    }
}
