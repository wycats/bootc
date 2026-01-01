//! Execution pipeline for command punning.
//!
//! Provides the unified execution model where commands:
//! 1. Execute locally (unless --pr-only)
//! 2. Update manifests
//! 3. Create PR (unless --local)
//!
//! This is the core infrastructure for Phase 2's command punning philosophy.

use crate::Cli;
use crate::context::{
    CommandDomain, ExecutionContext, PrMode, resolve_context, validate_context_for_domain,
};
use crate::pr::{PrChange, run_pr_workflow};
use anyhow::Result;

/// Execution plan for a bkt command.
///
/// Captures all the global options that affect how a command executes.
#[derive(Debug, Clone)]
pub struct ExecutionPlan {
    /// The resolved execution context
    pub context: ExecutionContext,
    /// PR creation mode
    pub pr_mode: PrMode,
    /// Whether to perform a dry run
    pub dry_run: bool,
    /// Whether to skip preflight checks
    pub skip_preflight: bool,
}

impl ExecutionPlan {
    /// Create an execution plan from CLI arguments.
    pub fn from_cli(cli: &Cli) -> Self {
        let context = resolve_context(cli.context);

        let pr_mode = if cli.pr_only {
            PrMode::PrOnly
        } else if cli.local {
            PrMode::LocalOnly
        } else {
            PrMode::Both
        };

        Self {
            context,
            pr_mode,
            dry_run: cli.dry_run,
            skip_preflight: cli.skip_preflight,
        }
    }

    /// Check if this plan allows local execution.
    pub fn should_execute_locally(&self) -> bool {
        !self.dry_run && self.pr_mode.should_execute_locally()
    }

    /// Check if this plan should create a PR.
    pub fn should_create_pr(&self) -> bool {
        !self.dry_run && self.pr_mode.should_create_pr()
    }

    /// Check if this plan should update local manifests.
    ///
    /// Local manifests are updated unless we're in pr-only mode.
    pub fn should_update_local_manifest(&self) -> bool {
        !self.dry_run && self.pr_mode != PrMode::PrOnly
    }

    /// Validate that the given domain is allowed for this plan's context.
    pub fn validate_domain(&self, domain: CommandDomain) -> Result<()> {
        validate_context_for_domain(domain, self.context)
    }

    /// Create a PR for a manifest change if the plan allows it.
    ///
    /// This is a convenience wrapper around the PR workflow.
    pub fn maybe_create_pr(
        &self,
        manifest_type: &str,
        action: &str,
        name: &str,
        manifest_file: &str,
        manifest_content: &str,
    ) -> Result<()> {
        if self.should_create_pr() {
            let change = PrChange {
                manifest_type: manifest_type.to_string(),
                action: action.to_string(),
                name: name.to_string(),
                manifest_file: manifest_file.to_string(),
            };
            run_pr_workflow(&change, manifest_content, self.skip_preflight)?;
        } else if self.dry_run {
            println!(
                "[dry-run] Would create PR: {} {} {}",
                action, manifest_type, name
            );
        }
        Ok(())
    }
}

impl Default for ExecutionPlan {
    fn default() -> Self {
        Self {
            context: ExecutionContext::Host,
            pr_mode: PrMode::Both,
            dry_run: false,
            skip_preflight: false,
        }
    }
}

/// Builder for creating execution plans in tests or programmatically.
#[derive(Debug, Default)]
pub struct ExecutionPlanBuilder {
    context: Option<ExecutionContext>,
    pr_mode: Option<PrMode>,
    dry_run: bool,
    skip_preflight: bool,
}

impl ExecutionPlanBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn context(mut self, ctx: ExecutionContext) -> Self {
        self.context = Some(ctx);
        self
    }

    pub fn pr_mode(mut self, mode: PrMode) -> Self {
        self.pr_mode = Some(mode);
        self
    }

    pub fn dry_run(mut self, dry_run: bool) -> Self {
        self.dry_run = dry_run;
        self
    }

    pub fn skip_preflight(mut self, skip: bool) -> Self {
        self.skip_preflight = skip;
        self
    }

    pub fn build(self) -> ExecutionPlan {
        ExecutionPlan {
            context: self.context.unwrap_or(ExecutionContext::Host),
            pr_mode: self.pr_mode.unwrap_or(PrMode::Both),
            dry_run: self.dry_run,
            skip_preflight: self.skip_preflight,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_plan() {
        let plan = ExecutionPlan::default();
        assert_eq!(plan.context, ExecutionContext::Host);
        assert_eq!(plan.pr_mode, PrMode::Both);
        assert!(!plan.dry_run);
        assert!(plan.should_execute_locally());
        assert!(plan.should_create_pr());
    }

    #[test]
    fn test_pr_only_mode() {
        let plan = ExecutionPlanBuilder::new().pr_mode(PrMode::PrOnly).build();
        assert!(!plan.should_execute_locally());
        assert!(plan.should_create_pr());
    }

    #[test]
    fn test_local_only_mode() {
        let plan = ExecutionPlanBuilder::new()
            .pr_mode(PrMode::LocalOnly)
            .build();
        assert!(plan.should_execute_locally());
        assert!(!plan.should_create_pr());
    }

    #[test]
    fn test_dry_run_prevents_all() {
        let plan = ExecutionPlanBuilder::new().dry_run(true).build();
        assert!(!plan.should_execute_locally());
        assert!(!plan.should_create_pr());
    }

    #[test]
    fn test_domain_validation() {
        let host_plan = ExecutionPlanBuilder::new()
            .context(ExecutionContext::Host)
            .build();
        let dev_plan = ExecutionPlanBuilder::new()
            .context(ExecutionContext::Dev)
            .build();

        // Flatpak should work on host
        assert!(host_plan.validate_domain(CommandDomain::Flatpak).is_ok());

        // Flatpak should fail on dev
        assert!(dev_plan.validate_domain(CommandDomain::Flatpak).is_err());

        // DNF should work everywhere
        assert!(host_plan.validate_domain(CommandDomain::Dnf).is_ok());
        assert!(dev_plan.validate_domain(CommandDomain::Dnf).is_ok());
    }
}
