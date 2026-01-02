//! Plan-centric command infrastructure.
//!
//! This module implements a pattern where commands are split into two phases:
//! 1. **Planning**: Analyze current state and produce an immutable plan (no side effects)
//! 2. **Execution**: Apply the plan's operations (all side effects happen here)
//!
//! This separation enables:
//! - Free dry-run support (just skip the execute phase)
//! - Plan composition (combine multiple plans into one)
//! - Plan inspection (serialize, diff, log plans)
//! - Consistent output formatting across all commands
//!
//! # Example
//!
//! ```rust,ignore
//! let plan = ShimSyncCommand.plan(&ctx)?;
//!
//! // Always show what will happen
//! println!("{}", plan.describe());
//!
//! // Only execute if not dry-run
//! if !dry_run {
//!     let report = plan.execute(&mut exec_ctx)?;
//!     println!("{}", report);
//! }
//! ```

use anyhow::Result;
use owo_colors::OwoColorize;
use std::fmt;
use std::path::PathBuf;

use crate::effects::Executor;
use crate::pipeline::ExecutionPlan;

// ============================================================================
// Core Traits
// ============================================================================

/// A command that can produce a plan without side effects.
///
/// The `plan()` method analyzes current state and returns a typed plan
/// describing what operations would be performed. This method must NOT
/// have any side effects.
pub trait Plannable {
    /// The plan type this command produces.
    type Plan: Plan;

    /// Analyze the current state and produce a plan.
    ///
    /// This method MUST NOT have side effects. It may read files,
    /// query system state, etc., but must not modify anything.
    fn plan(&self, ctx: &PlanContext) -> Result<Self::Plan>;
}

/// An immutable description of operations to perform.
///
/// Plans are the core abstraction for separating "what to do" from "doing it".
/// They can be inspected, composed, serialized, and compared before execution.
pub trait Plan: Sized {
    /// Get a structured description of this plan for display.
    fn describe(&self) -> PlanSummary;

    /// Execute the plan, performing all side effects.
    ///
    /// Consumes the plan since execution is a one-time operation.
    fn execute(self, ctx: &mut ExecuteContext) -> Result<ExecutionReport>;

    /// Returns true if this plan has no operations to perform.
    fn is_empty(&self) -> bool;
}

// ============================================================================
// Context Types
// ============================================================================

/// Context for the planning phase.
///
/// Provides read-only access to configuration and state needed to create plans.
pub struct PlanContext {
    /// Directory containing manifests.
    manifest_dir: PathBuf,
    /// Execution plan with PR/local mode settings.
    execution_plan: ExecutionPlan,
}

impl PlanContext {
    /// Create a new plan context.
    pub fn new(manifest_dir: PathBuf, execution_plan: ExecutionPlan) -> Self {
        Self {
            manifest_dir,
            execution_plan,
        }
    }

    /// Get the manifest directory.
    pub fn manifest_dir(&self) -> &PathBuf {
        &self.manifest_dir
    }

    /// Get the execution plan.
    pub fn execution_plan(&self) -> &ExecutionPlan {
        &self.execution_plan
    }

    /// Check if this is a dry run.
    pub fn is_dry_run(&self) -> bool {
        self.execution_plan.dry_run
    }
}

/// Context for the execution phase.
///
/// Provides controlled access to side effects via the `Executor`.
pub struct ExecuteContext {
    /// The underlying executor for side effects.
    executor: Executor,
    /// Execution plan with mode settings.
    execution_plan: ExecutionPlan,
}

impl ExecuteContext {
    /// Create a new execution context.
    pub fn new(execution_plan: ExecutionPlan) -> Self {
        Self {
            executor: Executor::new(execution_plan.dry_run),
            execution_plan,
        }
    }

    /// Get mutable access to the executor.
    pub fn executor(&mut self) -> &mut Executor {
        &mut self.executor
    }

    /// Get the execution plan.
    pub fn execution_plan(&self) -> &ExecutionPlan {
        &self.execution_plan
    }

    /// Check if local execution should happen.
    pub fn should_execute_locally(&self) -> bool {
        self.execution_plan.should_execute_locally()
    }
}

// ============================================================================
// Operation Types
// ============================================================================

/// A verb describing an operation type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verb {
    /// Install something (flatpak, package)
    Install,
    /// Remove something
    Remove,
    /// Enable something (extension)
    Enable,
    /// Disable something
    Disable,
    /// Set a value (gsetting)
    Set,
    /// Create a file/resource
    Create,
    /// Delete a file/resource
    Delete,
    /// Update something in place
    Update,
    /// Capture state to manifest
    Capture,
    /// Skip (already in desired state)
    Skip,
}

impl Verb {
    /// Get a display string for this verb.
    pub fn as_str(&self) -> &'static str {
        match self {
            Verb::Install => "Install",
            Verb::Remove => "Remove",
            Verb::Enable => "Enable",
            Verb::Disable => "Disable",
            Verb::Set => "Set",
            Verb::Create => "Create",
            Verb::Delete => "Delete",
            Verb::Update => "Update",
            Verb::Capture => "Capture",
            Verb::Skip => "Skip",
        }
    }

    /// Get a colored display for this verb.
    pub fn colored(&self) -> String {
        match self {
            Verb::Install | Verb::Enable | Verb::Create => self.as_str().green().to_string(),
            Verb::Remove | Verb::Disable | Verb::Delete => self.as_str().red().to_string(),
            Verb::Set | Verb::Update => self.as_str().yellow().to_string(),
            Verb::Capture => self.as_str().cyan().to_string(),
            Verb::Skip => self.as_str().dimmed().to_string(),
        }
    }
}

impl fmt::Display for Verb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// A single operation in a plan.
#[derive(Debug, Clone)]
pub struct Operation {
    /// The verb/action type.
    pub verb: Verb,
    /// The target of the operation (e.g., "flatpak:org.gnome.Boxes").
    pub target: String,
    /// Optional additional details.
    pub details: Option<String>,
}

impl Operation {
    /// Create a new operation.
    pub fn new(verb: Verb, target: impl Into<String>) -> Self {
        Self {
            verb,
            target: target.into(),
            details: None,
        }
    }

    /// Create a new operation with details.
    pub fn with_details(verb: Verb, target: impl Into<String>, details: impl Into<String>) -> Self {
        Self {
            verb,
            target: target.into(),
            details: Some(details.into()),
        }
    }
}

impl fmt::Display for Operation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.verb.colored(), self.target)?;
        if let Some(ref details) = self.details {
            write!(f, " ({})", details.dimmed())?;
        }
        Ok(())
    }
}

// ============================================================================
// Plan Summary
// ============================================================================

/// Structured description of a plan for display.
#[derive(Debug, Clone)]
pub struct PlanSummary {
    /// Brief summary of the plan.
    pub summary: String,
    /// List of operations to perform.
    pub operations: Vec<Operation>,
}

impl PlanSummary {
    /// Create a new plan summary.
    pub fn new(summary: impl Into<String>) -> Self {
        Self {
            summary: summary.into(),
            operations: Vec::new(),
        }
    }

    /// Add an operation to the summary.
    pub fn add_operation(&mut self, op: Operation) {
        self.operations.push(op);
    }

    /// Add multiple operations.
    pub fn add_operations(&mut self, ops: impl IntoIterator<Item = Operation>) {
        self.operations.extend(ops);
    }

    /// Get count of non-skip operations.
    pub fn action_count(&self) -> usize {
        self.operations
            .iter()
            .filter(|o| o.verb != Verb::Skip)
            .count()
    }

    /// Check if there are any real operations (not just skips).
    pub fn has_actions(&self) -> bool {
        self.action_count() > 0
    }
}

impl fmt::Display for PlanSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{}", self.summary.bold())?;

        if self.operations.is_empty() {
            writeln!(f, "  {}", "No operations".dimmed())?;
        } else {
            for op in &self.operations {
                writeln!(f, "  ▸ {}", op)?;
            }
        }

        let action_count = self.action_count();
        if action_count > 0 {
            writeln!(f, "\n{} operation(s) to perform", action_count)?;
        }

        Ok(())
    }
}

// ============================================================================
// Execution Report
// ============================================================================

/// Result of a single operation execution.
#[derive(Debug, Clone)]
pub struct OperationResult {
    /// The operation that was attempted.
    pub operation: Operation,
    /// Whether it succeeded.
    pub success: bool,
    /// Optional error message if failed.
    pub error: Option<String>,
}

impl OperationResult {
    /// Create a successful result.
    pub fn success(operation: Operation) -> Self {
        Self {
            operation,
            success: true,
            error: None,
        }
    }

    /// Create a failed result.
    pub fn failure(operation: Operation, error: impl Into<String>) -> Self {
        Self {
            operation,
            success: false,
            error: Some(error.into()),
        }
    }
}

/// Report of plan execution.
#[derive(Debug, Clone, Default)]
pub struct ExecutionReport {
    /// Results of each operation.
    pub results: Vec<OperationResult>,
}

impl ExecutionReport {
    /// Create a new empty report.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a successful operation.
    pub fn record_success(&mut self, verb: Verb, target: impl Into<String>) {
        self.results
            .push(OperationResult::success(Operation::new(verb, target)));
    }

    /// Record a successful operation with details.
    pub fn record_success_with_details(
        &mut self,
        verb: Verb,
        target: impl Into<String>,
        details: impl Into<String>,
    ) {
        self.results
            .push(OperationResult::success(Operation::with_details(
                verb, target, details,
            )));
    }

    /// Record a failed operation.
    pub fn record_failure(
        &mut self,
        verb: Verb,
        target: impl Into<String>,
        error: impl Into<String>,
    ) {
        self.results.push(OperationResult::failure(
            Operation::new(verb, target),
            error,
        ));
    }

    /// Merge another report into this one.
    pub fn merge(&mut self, other: ExecutionReport) {
        self.results.extend(other.results);
    }

    /// Count successful operations.
    pub fn success_count(&self) -> usize {
        self.results.iter().filter(|r| r.success).count()
    }

    /// Count failed operations.
    pub fn failure_count(&self) -> usize {
        self.results.iter().filter(|r| !r.success).count()
    }

    /// Check if all operations succeeded.
    pub fn all_succeeded(&self) -> bool {
        self.results.iter().all(|r| r.success)
    }

    /// Check if any operations failed.
    pub fn has_failures(&self) -> bool {
        self.results.iter().any(|r| !r.success)
    }
}

impl fmt::Display for ExecutionReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let success = self.success_count();
        let failed = self.failure_count();

        if failed == 0 {
            writeln!(
                f,
                "{}",
                format!("✓ {} operation(s) completed", success).green()
            )?;
        } else {
            writeln!(
                f,
                "{}",
                format!("⚠ {} succeeded, {} failed", success, failed).yellow()
            )?;
            writeln!(f)?;
            writeln!(f, "Failures:")?;
            for result in &self.results {
                if !result.success {
                    writeln!(
                        f,
                        "  {} {}: {}",
                        "✗".red(),
                        result.operation.target,
                        result.error.as_deref().unwrap_or("Unknown error")
                    )?;
                }
            }
        }

        Ok(())
    }
}

// ============================================================================
// Composite Plans
// ============================================================================

/// A plan that combines multiple boxed plans.
///
/// Used for heterogeneous composition (different plan types).
pub struct CompositePlan {
    /// The name of this composite plan (e.g., "Apply").
    name: String,
    /// The sub-plans to execute.
    plans: Vec<Box<dyn DynPlan>>,
}

impl CompositePlan {
    /// Create a new composite plan.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            plans: Vec::new(),
        }
    }

    /// Add a plan to this composite.
    pub fn add<P: Plan + 'static>(&mut self, plan: P) {
        if !plan.is_empty() {
            self.plans.push(Box::new(plan));
        }
    }
}

impl Plan for CompositePlan {
    fn describe(&self) -> PlanSummary {
        let mut summary = PlanSummary::new(format!("{} Plan", self.name));

        for plan in &self.plans {
            let sub = plan.describe_dyn();
            summary.add_operations(sub.operations);
        }

        summary
    }

    fn execute(self, ctx: &mut ExecuteContext) -> Result<ExecutionReport> {
        let mut report = ExecutionReport::new();

        for plan in self.plans {
            let sub_report = plan.execute_dyn(ctx)?;
            report.merge(sub_report);
        }

        Ok(report)
    }

    fn is_empty(&self) -> bool {
        self.plans.is_empty() || self.plans.iter().all(|p| p.is_empty_dyn())
    }
}

/// Object-safe trait for dynamic plan dispatch.
///
/// This enables heterogeneous plan composition via `Box<dyn DynPlan>`.
///
/// The `*_dyn` method names use a suffix to distinguish them from the [`Plan`]
/// trait methods they mirror. This suffix makes explicit that these are the
/// object-safe adapters used for dynamic dispatch, not the primary API.
trait DynPlan {
    /// Object-safe adapter for [`Plan::describe`].
    fn describe_dyn(&self) -> PlanSummary;
    /// Object-safe adapter for [`Plan::execute`], taking ownership via `Box<Self>`.
    fn execute_dyn(self: Box<Self>, ctx: &mut ExecuteContext) -> Result<ExecutionReport>;
    /// Object-safe adapter for [`Plan::is_empty`].
    fn is_empty_dyn(&self) -> bool;
}

impl<P: Plan> DynPlan for P {
    fn describe_dyn(&self) -> PlanSummary {
        self.describe()
    }

    fn execute_dyn(self: Box<Self>, ctx: &mut ExecuteContext) -> Result<ExecutionReport> {
        (*self).execute(ctx)
    }

    fn is_empty_dyn(&self) -> bool {
        self.is_empty()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_verb_display() {
        assert_eq!(Verb::Install.as_str(), "Install");
        assert_eq!(Verb::Remove.as_str(), "Remove");
        assert_eq!(Verb::Create.as_str(), "Create");
    }

    #[test]
    fn test_operation_display() {
        let op = Operation::new(Verb::Create, "shim:git");
        let display = format!("{}", op);
        assert!(display.contains("git"));
    }

    #[test]
    fn test_operation_with_details() {
        let op = Operation::with_details(Verb::Set, "gsetting:theme", "dark");
        assert_eq!(op.details, Some("dark".to_string()));
    }

    #[test]
    fn test_plan_summary_action_count() {
        let mut summary = PlanSummary::new("Test");
        summary.add_operation(Operation::new(Verb::Create, "a"));
        summary.add_operation(Operation::new(Verb::Skip, "b"));
        summary.add_operation(Operation::new(Verb::Create, "c"));

        assert_eq!(summary.action_count(), 2);
        assert!(summary.has_actions());
    }

    #[test]
    fn test_execution_report() {
        let mut report = ExecutionReport::new();
        report.record_success(Verb::Create, "shim:git");
        report.record_success(Verb::Create, "shim:gh");
        report.record_failure(Verb::Create, "shim:bad", "Permission denied");

        assert_eq!(report.success_count(), 2);
        assert_eq!(report.failure_count(), 1);
        assert!(report.has_failures());
        assert!(!report.all_succeeded());
    }

    #[test]
    fn test_execution_report_merge() {
        let mut report1 = ExecutionReport::new();
        report1.record_success(Verb::Create, "a");

        let mut report2 = ExecutionReport::new();
        report2.record_success(Verb::Create, "b");

        report1.merge(report2);
        assert_eq!(report1.results.len(), 2);
    }

    // Helper struct for testing CompositePlan
    struct TestPlan {
        operations: Vec<Operation>,
        execute_fn: fn() -> Result<ExecutionReport>,
    }

    impl TestPlan {
        fn new(ops: Vec<Operation>) -> Self {
            Self {
                operations: ops,
                execute_fn: || Ok(ExecutionReport::new()),
            }
        }

        fn empty() -> Self {
            Self {
                operations: vec![],
                execute_fn: || Ok(ExecutionReport::new()),
            }
        }
    }

    impl Plan for TestPlan {
        fn describe(&self) -> PlanSummary {
            let mut summary = PlanSummary::new("TestPlan");
            for op in &self.operations {
                summary.add_operation(op.clone());
            }
            summary
        }

        fn execute(self, _ctx: &mut ExecuteContext) -> Result<ExecutionReport> {
            (self.execute_fn)()
        }

        fn is_empty(&self) -> bool {
            self.operations.is_empty()
        }
    }

    #[test]
    fn test_composite_plan_filters_empty_plans() {
        let mut composite = CompositePlan::new("Test");
        composite.add(TestPlan::empty());
        composite.add(TestPlan::new(vec![Operation::new(Verb::Create, "a")]));
        composite.add(TestPlan::empty());

        // Only the non-empty plan should be added
        assert_eq!(composite.plans.len(), 1);
        assert!(!composite.is_empty());
    }

    #[test]
    fn test_composite_plan_is_empty_when_all_empty() {
        let mut composite = CompositePlan::new("Test");
        composite.add(TestPlan::empty());
        composite.add(TestPlan::empty());

        assert!(composite.is_empty());
    }

    #[test]
    fn test_composite_plan_describe_aggregates_operations() {
        let mut composite = CompositePlan::new("Test");
        composite.add(TestPlan::new(vec![
            Operation::new(Verb::Create, "a"),
            Operation::new(Verb::Create, "b"),
        ]));
        composite.add(TestPlan::new(vec![Operation::new(Verb::Install, "c")]));

        let summary = composite.describe();
        assert_eq!(summary.operations.len(), 3);
        assert_eq!(summary.action_count(), 3);
    }

    #[test]
    fn test_composite_plan_execute_merges_reports() {
        let mut composite = CompositePlan::new("Test");
        composite.add(TestPlan::new(vec![Operation::new(Verb::Create, "a")]));
        composite.add(TestPlan::new(vec![Operation::new(Verb::Create, "b")]));

        let mut ctx = ExecuteContext::new(ExecutionPlan::default());
        let report = composite.execute(&mut ctx).unwrap();

        // Both sub-plans executed (though TestPlan returns empty reports)
        assert!(report.all_succeeded());
    }
}
