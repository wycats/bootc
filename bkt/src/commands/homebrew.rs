//! Homebrew command implementation.
//!
//! Manages Linuxbrew/Homebrew packages on the host system.

use crate::command_runner::{CommandOptions, CommandRunner};
use crate::context::CommandDomain;
use crate::manifest::homebrew::HomebrewManifest;
use crate::output::Output;
use crate::pipeline::ExecutionPlan;
use crate::plan::{
    ExecuteContext, ExecutionReport, Operation, Plan, PlanContext, PlanSummary, Plannable, Verb,
};
use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use std::collections::HashSet;

#[derive(Debug, Args)]
pub struct HomebrewArgs {
    #[command(subcommand)]
    pub action: HomebrewAction,
}

#[derive(Debug, Subcommand)]
pub enum HomebrewAction {
    /// Add a formula to the manifest
    Add {
        /// Formula name (e.g., "lefthook" or "user/tap/formula")
        formula: String,
    },
    /// Remove a formula from the manifest
    Remove {
        /// Formula name to remove
        formula: String,
    },
    /// List formulae in the manifest
    List {
        /// Output format (table, json)
        #[arg(short, long, default_value = "table")]
        format: String,
    },
    /// Sync: install formulae from manifest
    Sync,
    /// Capture installed formulae to manifest
    Capture,
}

pub fn run(args: HomebrewArgs, plan: &ExecutionPlan) -> Result<()> {
    plan.validate_domain(CommandDomain::Homebrew)?;

    let cwd = std::env::current_dir()?;
    let plan_ctx = PlanContext::new(cwd, plan.clone());

    match args.action {
        HomebrewAction::Add { formula } => handle_add(&formula, &plan_ctx),
        HomebrewAction::Remove { formula } => handle_remove(&formula, &plan_ctx),
        HomebrewAction::List { format } => handle_list(&format),
        HomebrewAction::Sync => handle_sync(&plan_ctx),
        HomebrewAction::Capture => handle_capture(&plan_ctx),
    }
}

// =============================================================================
// Add Command
// =============================================================================

fn handle_add(formula: &str, ctx: &PlanContext) -> Result<()> {
    let mut user = HomebrewManifest::load_user()?;

    if user.contains(formula) {
        Output::warning(format!("Formula '{}' is already in manifest", formula));
        return Ok(());
    }

    if ctx.is_dry_run() {
        Output::dry_run(format!("Would add formula '{}' to manifest", formula));
        return Ok(());
    }

    user.add(formula);
    user.save_user()?;
    Output::success(format!("Added '{}' to homebrew manifest", formula));

    Ok(())
}

// =============================================================================
// Remove Command
// =============================================================================

fn handle_remove(formula: &str, ctx: &PlanContext) -> Result<()> {
    let mut user = HomebrewManifest::load_user()?;

    if !user.contains(formula) {
        Output::warning(format!("Formula '{}' is not in manifest", formula));
        return Ok(());
    }

    if ctx.is_dry_run() {
        Output::dry_run(format!("Would remove formula '{}' from manifest", formula));
        return Ok(());
    }

    user.remove(formula);
    user.save_user()?;
    Output::success(format!("Removed '{}' from homebrew manifest", formula));

    Ok(())
}

// =============================================================================
// List Command
// =============================================================================

fn handle_list(format: &str) -> Result<()> {
    let system = HomebrewManifest::load_system()?;
    let user = HomebrewManifest::load_user()?;
    let merged = HomebrewManifest::merged(&system, &user);

    if format == "json" {
        println!("{}", serde_json::to_string_pretty(&merged)?);
        return Ok(());
    }

    if merged.formulae.is_empty() {
        Output::info("No formulae in manifest");
        return Ok(());
    }

    Output::header("Homebrew Formulae");
    for formula in &merged.formulae {
        if let Some(tap) = formula.tap() {
            println!("  {} (tap: {})", formula.formula_name(), tap);
        } else {
            println!("  {}", formula.name());
        }
    }

    if !merged.taps.is_empty() {
        Output::blank();
        Output::header("Taps");
        for tap in &merged.taps {
            println!("  {}", tap);
        }
    }

    Ok(())
}

// =============================================================================
// Sync Command
// =============================================================================

fn handle_sync(ctx: &PlanContext) -> Result<()> {
    let cmd = HomebrewSyncCommand;
    let plan = cmd.plan(ctx)?;

    if plan.is_empty() {
        Output::success("All formulae from manifest are installed.");
        return Ok(());
    }

    let summary = plan.describe();
    print!("{}", summary);

    if ctx.is_dry_run() {
        Output::info("Run without --dry-run to apply these changes.");
        return Ok(());
    }

    let total_ops = summary.action_count();
    let mut exec_ctx = ExecuteContext::new(ctx.execution_plan().clone());
    exec_ctx.set_total_ops(total_ops);

    let report = plan.execute(&mut exec_ctx)?;
    println!();
    print!("{}", report);

    Ok(())
}

// =============================================================================
// Capture Command
// =============================================================================

fn handle_capture(ctx: &PlanContext) -> Result<()> {
    let cmd = HomebrewCaptureCommand;
    let plan = cmd.plan(ctx)?;

    if plan.is_empty() {
        Output::success("Nothing to capture. All installed formulae are in manifest.");
        return Ok(());
    }

    let summary = plan.describe();
    print!("{}", summary);

    if ctx.is_dry_run() {
        Output::info("Run without --dry-run to capture these formulae.");
        return Ok(());
    }

    let mut exec_ctx = ExecuteContext::new(ctx.execution_plan().clone());
    let report = plan.execute(&mut exec_ctx)?;
    print!("{}", report);

    Ok(())
}

// =============================================================================
// Sync Plan
// =============================================================================

pub struct HomebrewSyncCommand;

pub struct HomebrewSyncPlan {
    /// Formulae to install.
    pub to_install: Vec<String>,
    /// Taps to add.
    pub taps_to_add: Vec<String>,
    /// Already installed count.
    pub already_installed: usize,
}

impl Plannable for HomebrewSyncCommand {
    type Plan = HomebrewSyncPlan;

    fn plan(&self, ctx: &PlanContext) -> Result<Self::Plan> {
        let runner = ctx.execution_plan().runner();

        let system = HomebrewManifest::load_system()?;
        let user = HomebrewManifest::load_user()?;
        let merged = HomebrewManifest::merged(&system, &user);

        let installed = get_installed_formulae(runner);
        let installed_taps = get_installed_taps(runner);

        let mut to_install = Vec::new();
        let mut already_installed = 0;

        for formula in &merged.formulae {
            let name = formula.formula_name();
            if installed.contains(name) {
                already_installed += 1;
            } else {
                to_install.push(formula.name().to_string());
            }
        }

        let taps_to_add: Vec<String> = merged
            .taps
            .iter()
            .filter(|t| !installed_taps.contains(t.as_str()))
            .cloned()
            .collect();

        Ok(HomebrewSyncPlan {
            to_install,
            taps_to_add,
            already_installed,
        })
    }
}

impl Plan for HomebrewSyncPlan {
    fn is_empty(&self) -> bool {
        self.to_install.is_empty() && self.taps_to_add.is_empty()
    }

    fn describe(&self) -> PlanSummary {
        let mut summary = PlanSummary::new(format!(
            "Homebrew Sync: {} to install, {} already installed",
            self.to_install.len(),
            self.already_installed
        ));

        for tap in &self.taps_to_add {
            summary.add_operation(Operation::new(Verb::Install, format!("tap:{}", tap)));
        }

        for formula in &self.to_install {
            summary.add_operation(Operation::new(
                Verb::Install,
                format!("formula:{}", formula),
            ));
        }

        summary
    }

    fn execute(self, ctx: &mut ExecuteContext) -> Result<ExecutionReport> {
        let mut report = ExecutionReport::new();
        let runner = ctx.execution_plan().runner();

        // Add taps first
        for tap in self.taps_to_add {
            if install_tap(&tap, runner)? {
                report.record_success(Verb::Install, format!("tap:{}", tap));
            } else {
                report.record_failure(Verb::Install, format!("tap:{}", tap), "failed to add tap");
            }
        }

        // Install formulae
        for formula in self.to_install {
            if install_formula(&formula, runner)? {
                report.record_success(Verb::Install, format!("formula:{}", formula));
            } else {
                report.record_failure(
                    Verb::Install,
                    format!("formula:{}", formula),
                    "failed to install",
                );
            }
        }

        Ok(report)
    }
}

// =============================================================================
// Capture Plan
// =============================================================================

pub struct HomebrewCaptureCommand;

pub struct HomebrewCapturePlan {
    /// Formulae to add to manifest.
    pub to_capture: Vec<String>,
    /// Already in manifest count.
    pub already_in_manifest: usize,
}

impl Plannable for HomebrewCaptureCommand {
    type Plan = HomebrewCapturePlan;

    fn plan(&self, ctx: &PlanContext) -> Result<Self::Plan> {
        let runner = ctx.execution_plan().runner();

        let system = HomebrewManifest::load_system()?;
        let user = HomebrewManifest::load_user()?;
        let merged = HomebrewManifest::merged(&system, &user);

        // Get explicitly installed formulae (leaves that were requested)
        let installed = get_explicitly_installed_formulae(runner);

        let mut to_capture = Vec::new();
        let mut already_in_manifest = 0;

        for formula in installed {
            if merged.contains(&formula) {
                already_in_manifest += 1;
            } else {
                to_capture.push(formula);
            }
        }

        to_capture.sort();

        Ok(HomebrewCapturePlan {
            to_capture,
            already_in_manifest,
        })
    }
}

impl Plan for HomebrewCapturePlan {
    fn is_empty(&self) -> bool {
        self.to_capture.is_empty()
    }

    fn describe(&self) -> PlanSummary {
        let mut summary = PlanSummary::new(format!(
            "Homebrew Capture: {} to add, {} already in manifest",
            self.to_capture.len(),
            self.already_in_manifest
        ));

        for formula in &self.to_capture {
            summary.add_operation(Operation::new(
                Verb::Capture,
                format!("formula:{}", formula),
            ));
        }

        summary
    }

    fn execute(self, _ctx: &mut ExecuteContext) -> Result<ExecutionReport> {
        let mut report = ExecutionReport::new();
        let mut user = HomebrewManifest::load_user()?;

        for formula in self.to_capture {
            if user.add(formula.clone()) {
                report.record_success(Verb::Capture, format!("formula:{}", formula));
            }
        }

        user.save_user()?;

        Ok(report)
    }
}

// =============================================================================
// Helpers
// =============================================================================

/// Get set of installed formula names.
fn get_installed_formulae(runner: &dyn CommandRunner) -> HashSet<String> {
    let output = runner.run_output(
        "brew",
        &["list", "--formula", "-1"],
        &CommandOptions::default(),
    );

    match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .lines()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
        _ => HashSet::new(),
    }
}

/// Get explicitly installed formulae (not dependencies).
fn get_explicitly_installed_formulae(runner: &dyn CommandRunner) -> Vec<String> {
    let output = runner.run_output("brew", &["leaves", "-r"], &CommandOptions::default());

    match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .lines()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

/// Get installed taps.
fn get_installed_taps(runner: &dyn CommandRunner) -> HashSet<String> {
    let output = runner.run_output("brew", &["tap"], &CommandOptions::default());

    match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .lines()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
        _ => HashSet::new(),
    }
}

/// Install a formula.
fn install_formula(formula: &str, runner: &dyn CommandRunner) -> Result<bool> {
    let status = runner
        .run_status("brew", &["install", formula], &CommandOptions::default())
        .context("Failed to run brew install")?;

    Ok(status.success())
}

/// Add a tap.
fn install_tap(tap: &str, runner: &dyn CommandRunner) -> Result<bool> {
    let status = runner
        .run_status("brew", &["tap", tap], &CommandOptions::default())
        .context("Failed to run brew tap")?;

    Ok(status.success())
}
