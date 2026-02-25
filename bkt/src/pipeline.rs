//! Execution pipeline for command punning.
//!
//! Provides the unified execution model where commands:
//! 1. Execute locally (unless --pr-only)
//! 2. Update manifests
//! 3. Create PR (if enabled)
//!
//! This is the core infrastructure for Phase 2's command punning philosophy.

use crate::cli::Cli;
use crate::command_runner::{CommandRunner, RealCommandRunner};
use crate::context::{
    CommandDomain, ExecutionContext, PrMode, resolve_context, validate_context_for_domain,
};
use crate::pr::{GitHubBackend, PrBackend, PrChange};
use anyhow::Result;
use std::sync::Arc;

/// Execution plan for a bkt command.
///
/// Captures all the global options that affect how a command executes.
#[derive(Clone)]
pub struct ExecutionPlan {
    /// The resolved execution context
    pub context: ExecutionContext,
    /// PR creation mode
    pub pr_mode: PrMode,
    /// Whether to perform a dry run
    pub dry_run: bool,
    /// Whether to skip preflight checks
    pub skip_preflight: bool,
    /// Backend for PR creation (enables testing)
    pr_backend: Arc<dyn PrBackend>,
    /// Backend for external command execution (enables testing)
    command_runner: Arc<dyn CommandRunner>,
}

impl ExecutionPlan {
    /// Create an execution plan from CLI arguments.
    pub fn from_cli(cli: &Cli) -> Self {
        let context = resolve_context(cli.context);

        let pr_mode = if cli.pr_only {
            PrMode::PrOnly
        } else {
            PrMode::Default
        };

        let command_runner: Arc<dyn CommandRunner> = Arc::new(RealCommandRunner);

        Self {
            context,
            pr_mode,
            dry_run: cli.dry_run,
            skip_preflight: cli.skip_preflight,
            pr_backend: Arc::new(GitHubBackend::new(command_runner.clone())),
            command_runner,
        }
    }

    /// Create a copy of this plan with the specified dry_run value.
    pub fn with_dry_run(&self, dry_run: bool) -> Self {
        Self {
            context: self.context,
            pr_mode: self.pr_mode,
            dry_run,
            skip_preflight: self.skip_preflight,
            pr_backend: self.pr_backend.clone(),
            command_runner: self.command_runner.clone(),
        }
    }

    /// Get the command runner for external command execution.
    pub fn runner(&self) -> &dyn CommandRunner {
        &*self.command_runner
    }

    /// Get a clone of the command runner Arc for downstream ownership.
    pub(crate) fn command_runner_arc(&self) -> Arc<dyn CommandRunner> {
        self.command_runner.clone()
    }

    /// Check if this plan allows local execution.
    pub fn should_execute_locally(&self) -> bool {
        !self.dry_run && self.pr_mode.should_execute_locally()
    }

    /// Check if this plan should create a PR.
    ///
    /// PRs are not created for Dev context since toolbox packages are personal
    /// and not part of the system image.
    pub fn should_create_pr(&self) -> bool {
        // Toolbox changes are personal, not part of system image
        if self.context == ExecutionContext::Dev {
            return false;
        }
        !self.dry_run && self.pr_mode.should_create_pr()
    }

    /// Check if this plan should update manifests.
    ///
    /// Manifests are updated unless we're in pr-only mode.
    pub fn should_update_manifest(&self) -> bool {
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
            self.pr_backend
                .create_pr(&change, manifest_content, self.skip_preflight)?;
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
        let command_runner: Arc<dyn CommandRunner> = Arc::new(RealCommandRunner);
        Self {
            context: ExecutionContext::Host,
            pr_mode: PrMode::Default,
            dry_run: false,
            skip_preflight: false,
            pr_backend: Arc::new(GitHubBackend::new(command_runner.clone())),
            command_runner,
        }
    }
}

/// Builder for creating execution plans in tests or programmatically.
#[derive(Default)]
pub struct ExecutionPlanBuilder {
    context: Option<ExecutionContext>,
    pr_mode: Option<PrMode>,
    dry_run: bool,
    skip_preflight: bool,
    pr_backend: Option<Arc<dyn PrBackend>>,
    command_runner: Option<Arc<dyn CommandRunner>>,
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

    pub fn pr_backend(mut self, backend: Arc<dyn PrBackend>) -> Self {
        self.pr_backend = Some(backend);
        self
    }

    pub fn command_runner(mut self, runner: Arc<dyn CommandRunner>) -> Self {
        self.command_runner = Some(runner);
        self
    }

    pub fn build(self) -> ExecutionPlan {
        let command_runner = self
            .command_runner
            .unwrap_or_else(|| Arc::new(RealCommandRunner));
        let pr_backend = self
            .pr_backend
            .unwrap_or_else(|| Arc::new(GitHubBackend::new(command_runner.clone())));

        ExecutionPlan {
            context: self.context.unwrap_or(ExecutionContext::Host),
            pr_mode: self.pr_mode.unwrap_or(PrMode::Default),
            dry_run: self.dry_run,
            skip_preflight: self.skip_preflight,
            pr_backend,
            command_runner,
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
        assert_eq!(plan.pr_mode, PrMode::Default);
        assert!(!plan.dry_run);
        assert!(plan.should_execute_locally());
        assert!(!plan.should_create_pr());
    }

    #[test]
    fn test_pr_only_mode() {
        let plan = ExecutionPlanBuilder::new().pr_mode(PrMode::PrOnly).build();
        assert!(!plan.should_execute_locally());
        assert!(plan.should_create_pr());
    }

    #[test]
    #[test]
    fn test_dry_run_prevents_all() {
        let plan = ExecutionPlanBuilder::new().dry_run(true).build();
        assert!(!plan.should_execute_locally());
        assert!(!plan.should_create_pr());
        assert!(!plan.should_update_manifest());
    }

    #[test]
    fn test_should_update_manifest() {
        // Default mode: should update manifest
        let default_plan = ExecutionPlanBuilder::new().build();
        assert!(default_plan.should_update_manifest());

        // Pr mode: should update manifest
        let pr_plan = ExecutionPlanBuilder::new().pr_mode(PrMode::Pr).build();
        assert!(pr_plan.should_update_manifest());

        // PrOnly mode: should NOT update manifest
        let pr_only_plan = ExecutionPlanBuilder::new().pr_mode(PrMode::PrOnly).build();
        assert!(!pr_only_plan.should_update_manifest());

        // Dry-run mode: should NOT update manifest
        let dry_run_plan = ExecutionPlanBuilder::new().dry_run(true).build();
        assert!(!dry_run_plan.should_update_manifest());
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

    #[test]
    fn test_dev_context_disables_pr_creation() {
        // Dev context should never create PRs (toolbox packages are personal)
        let dev_plan = ExecutionPlanBuilder::new()
            .context(ExecutionContext::Dev)
            .build();
        assert!(!dev_plan.should_create_pr());

        // Even with PrMode::Pr, Dev context should not create PRs
        let dev_both = ExecutionPlanBuilder::new()
            .context(ExecutionContext::Dev)
            .pr_mode(PrMode::Pr)
            .build();
        assert!(!dev_both.should_create_pr());

        // Host context should still create PRs
        let host_plan = ExecutionPlanBuilder::new()
            .context(ExecutionContext::Host)
            .pr_mode(PrMode::Pr)
            .build();
        assert!(host_plan.should_create_pr());
    }
}
