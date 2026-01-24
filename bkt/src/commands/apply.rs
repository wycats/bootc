//! Apply command - sync all manifests to the running system.
//!
//! The `bkt apply` command composes multiple sync plans into one and executes them.
//! This is the "manifest → system" direction of bidirectional sync.

use anyhow::{Context, Result};
use clap::Args;

use crate::output::Output;
use crate::pipeline::ExecutionPlan;
use crate::plan::{CompositePlan, ExecuteContext, OperationProgress, Plan, PlanContext, Plannable};
use crate::subsystem::SubsystemRegistry;

fn parse_syncable_subsystem(value: &str) -> Result<String, String> {
    let registry = SubsystemRegistry::builtin();
    let normalized = value.replace('-', "");
    if registry.is_valid_syncable(&normalized) {
        Ok(normalized)
    } else {
        let valid = registry.syncable_ids();
        Err(format!(
            "Unknown subsystem '{}'. Valid options: {}",
            value,
            valid.join(", ")
        ))
    }
}

#[derive(Debug, Args)]
pub struct ApplyArgs {
    /// Only sync specific subsystems (comma-separated)
    #[arg(long, short = 's', value_delimiter = ',', value_parser = parse_syncable_subsystem)]
    pub only: Option<Vec<String>>,

    /// Exclude specific subsystems from sync
    #[arg(long, short = 'x', value_delimiter = ',', value_parser = parse_syncable_subsystem)]
    pub exclude: Option<Vec<String>>,

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
    pub include: Option<Vec<String>>,
    /// Subsystems to exclude.
    pub exclude: Vec<String>,

    /// Whether to prune unmanaged AppImages.
    /// Note: This is currently not passed through the registry abstraction.
    /// AppImage sync via registry uses the default (keep_unmanaged=false).
    #[allow(dead_code)]
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

impl Plannable for ApplyCommand {
    type Plan = CompositePlan;

    fn plan(&self, ctx: &PlanContext) -> Result<Self::Plan> {
        let mut composite = CompositePlan::new("Apply");
        let registry = SubsystemRegistry::builtin();

        for subsystem in registry.syncable() {
            if self.should_include_id(subsystem.id())
                && let Some(plan) = subsystem.sync(ctx)?
            {
                composite.add_boxed(plan);
            }
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

        assert!(cmd.should_include_id("shim"));
        assert!(cmd.should_include_id("distrobox"));
        assert!(cmd.should_include_id("gsetting"));
        assert!(cmd.should_include_id("extension"));
        assert!(cmd.should_include_id("flatpak"));
    }

    #[test]
    fn test_apply_command_only_filter() {
        let cmd = ApplyCommand {
            include: Some(vec!["shim".to_string(), "flatpak".to_string()]),
            exclude: vec![],
            prune_appimages: false,
        };

        assert!(cmd.should_include_id("shim"));
        assert!(!cmd.should_include_id("gsetting"));
        assert!(!cmd.should_include_id("extension"));
        assert!(cmd.should_include_id("flatpak"));
    }

    #[test]
    fn test_apply_command_exclude_filter() {
        let cmd = ApplyCommand {
            include: None,
            exclude: vec!["extension".to_string(), "flatpak".to_string()],
            prune_appimages: false,
        };

        assert!(cmd.should_include_id("shim"));
        assert!(cmd.should_include_id("distrobox"));
        assert!(cmd.should_include_id("gsetting"));
        assert!(!cmd.should_include_id("extension"));
        assert!(!cmd.should_include_id("flatpak"));
    }

    #[test]
    fn test_apply_command_exclude_overrides_include() {
        let cmd = ApplyCommand {
            include: Some(vec!["shim".to_string(), "extension".to_string()]),
            exclude: vec!["extension".to_string()],
            prune_appimages: false,
        };

        assert!(cmd.should_include_id("shim"));
        assert!(!cmd.should_include_id("extension")); // excluded wins
    }

    #[test]
    fn test_apply_command_from_args() {
        let args = ApplyArgs {
            only: Some(vec!["shim".to_string()]),
            exclude: Some(vec!["flatpak".to_string()]),
            confirm: true,
            prune_appimages: true,
        };

        let cmd = ApplyCommand::from_args(&args);
        assert_eq!(cmd.include, Some(vec!["shim".to_string()]));
        assert_eq!(cmd.exclude, vec!["flatpak".to_string()]);
        assert!(cmd.prune_appimages);
    }
}
