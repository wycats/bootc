//! GSettings command implementation.

use crate::component::{DriftReport, SystemComponent};
use crate::context::PrMode;
use crate::manifest::ephemeral::{ChangeAction, ChangeDomain, EphemeralChange, EphemeralManifest};
use crate::manifest::{GSetting, GSettingsManifest};
use crate::output::Output;
use crate::pipeline::ExecutionPlan;
use crate::plan::{
    ExecuteContext, ExecutionReport, Operation, Plan, PlanContext, PlanSummary, Plannable, Verb,
};
use crate::validation::{validate_gsettings_key, validate_gsettings_schema};
use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use owo_colors::OwoColorize;
use std::process::Command;

#[derive(Debug, Args)]
pub struct GSettingArgs {
    #[command(subcommand)]
    pub action: GSettingAction,
}

#[derive(Debug, Subcommand)]
pub enum GSettingAction {
    /// Set a GSettings value in the manifest
    Set {
        /// Schema name (e.g., org.gnome.desktop.interface)
        schema: String,
        /// Key name
        key: String,
        /// Value (as GVariant string)
        value: String,
        /// Optional comment
        #[arg(short, long)]
        comment: Option<String>,
        /// Skip schema/key validation
        #[arg(long)]
        force: bool,
    },
    /// Remove a GSettings entry from the manifest
    Unset {
        /// Schema name
        schema: String,
        /// Key name
        key: String,
    },
    /// List all GSettings in the manifest
    List {
        /// Output format (table, json)
        #[arg(short, long, default_value = "table")]
        format: String,
    },
    /// Apply all GSettings from the manifest
    Apply,
    /// Capture current GSettings values to manifest
    Capture {
        /// Schema name to capture (required - captures all keys from this schema)
        schema: String,
        /// Specific key to capture (optional - defaults to all keys in schema)
        #[arg(short, long)]
        key: Option<String>,
        /// Apply the plan immediately (default is preview only)
        #[arg(long)]
        apply: bool,
    },
}

/// Get current value of a gsetting.
fn get_current_value(schema: &str, key: &str) -> Option<String> {
    Command::new("gsettings")
        .args(["get", schema, key])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

/// Set a gsetting value.
fn set_gsetting(schema: &str, key: &str, value: &str) -> Result<bool> {
    let status = Command::new("gsettings")
        .args(["set", schema, key, value])
        .status()
        .context("Failed to run gsettings set")?;
    Ok(status.success())
}

pub fn run(args: GSettingArgs, plan: &ExecutionPlan) -> Result<()> {
    match args.action {
        GSettingAction::Set {
            schema,
            key,
            value,
            comment,
            force,
        } => {
            // Validate schema and key exist before modifying manifest
            if !force {
                validate_gsettings_schema(&schema)?;
                validate_gsettings_key(&schema, &key)?;
            }

            let system = GSettingsManifest::load_system()?;
            let mut user = GSettingsManifest::load_user()?;

            // Check if already set to same value
            let existing = system
                .find(&schema, &key)
                .or_else(|| user.find(&schema, &key));

            if plan.should_update_local_manifest() {
                if let Some(e) = existing {
                    if e.value == value {
                        Output::info(format!(
                            "Already in manifest: {}.{} = {}",
                            schema, key, value
                        ));
                    } else {
                        // Update in user manifest
                        let setting = GSetting {
                            schema: schema.clone(),
                            key: key.clone(),
                            value: value.clone(),
                            comment,
                        };
                        user.upsert(setting);
                        user.save_user()?;
                        Output::success(format!(
                            "Updated in user manifest: {}.{} = {}",
                            schema, key, value
                        ));
                    }
                } else {
                    let setting = GSetting {
                        schema: schema.clone(),
                        key: key.clone(),
                        value: value.clone(),
                        comment,
                    };
                    user.upsert(setting);
                    user.save_user()?;
                    Output::success(format!(
                        "Added to user manifest: {}.{} = {}",
                        schema, key, value
                    ));
                }
            } else if plan.dry_run {
                Output::dry_run(format!(
                    "Would set in manifest: {}.{} = {}",
                    schema, key, value
                ));
            }

            // Record ephemeral change if using --local (not in dry-run mode)
            if plan.pr_mode == PrMode::LocalOnly && !plan.dry_run {
                let mut ephemeral = EphemeralManifest::load_validated()?;
                ephemeral.record(
                    EphemeralChange::new(
                        ChangeDomain::Gsetting,
                        ChangeAction::Update,
                        format!("{}.{}", schema, key),
                    )
                    .with_metadata("value", &value),
                );
                ephemeral.save()?;
            }

            // Apply immediately
            if plan.should_execute_locally() {
                let spinner =
                    Output::spinner(format!("Applying {}.{} = {}...", schema, key, value));
                if set_gsetting(&schema, &key, &value)? {
                    spinner.finish_success(format!("Applied {}.{}", schema, key));
                } else {
                    spinner.finish_error(format!("Failed to apply {}.{}", schema, key));
                }
            } else if plan.dry_run {
                Output::dry_run(format!(
                    "Would apply gsetting: {}.{} = {}",
                    schema, key, value
                ));
            }

            if plan.should_create_pr() {
                let mut system_manifest = GSettingsManifest::load_system()?;
                let setting_for_pr = GSetting {
                    schema: schema.clone(),
                    key: key.clone(),
                    value: value.clone(),
                    comment: None,
                };
                system_manifest.upsert(setting_for_pr);
                let manifest_content = serde_json::to_string_pretty(&system_manifest)?;

                plan.maybe_create_pr(
                    "gsetting",
                    "set",
                    &format!("{}.{}", schema, key),
                    "gsettings.json",
                    &manifest_content,
                )?;
            }
        }
        GSettingAction::Unset { schema, key } => {
            let mut user = GSettingsManifest::load_user()?;
            let system = GSettingsManifest::load_system()?;

            let in_system = system.find(&schema, &key).is_some();
            if in_system && user.find(&schema, &key).is_none() && !plan.should_create_pr() {
                Output::info(format!(
                    "'{}.{}' is in the system manifest; use --pr or --pr-only to remove from source",
                    schema, key
                ));
            }

            if plan.should_update_local_manifest() {
                if user.remove(&schema, &key) {
                    user.save_user()?;
                    Output::success(format!("Removed from user manifest: {}.{}", schema, key));
                } else if !in_system {
                    Output::warning(format!("Setting not found in manifest: {}.{}", schema, key));
                }
            } else if plan.dry_run {
                Output::dry_run(format!("Would remove from manifest: {}.{}", schema, key));
            }

            // Record ephemeral change if using --local (not in dry-run mode)
            if plan.pr_mode == PrMode::LocalOnly && !plan.dry_run {
                let mut ephemeral = EphemeralManifest::load_validated()?;
                ephemeral.record(EphemeralChange::new(
                    ChangeDomain::Gsetting,
                    ChangeAction::Remove,
                    format!("{}.{}", schema, key),
                ));
                ephemeral.save()?;
            }

            // Reset to default
            if plan.should_execute_locally() {
                let spinner =
                    Output::spinner(format!("Resetting {}.{} to default...", schema, key));
                let status = Command::new("gsettings")
                    .args(["reset", &schema, &key])
                    .status()
                    .context("Failed to run gsettings reset")?;
                if status.success() {
                    spinner.finish_success(format!("Reset {}.{}", schema, key));
                } else {
                    spinner.finish_error(format!("Failed to reset {}.{}", schema, key));
                }
            } else if plan.dry_run {
                Output::dry_run(format!(
                    "Would reset gsetting to default: {}.{}",
                    schema, key
                ));
            }

            if plan.should_create_pr() {
                let mut system_manifest = GSettingsManifest::load_system()?;
                if system_manifest.remove(&schema, &key) {
                    let manifest_content = serde_json::to_string_pretty(&system_manifest)?;

                    plan.maybe_create_pr(
                        "gsetting",
                        "unset",
                        &format!("{}.{}", schema, key),
                        "gsettings.json",
                        &manifest_content,
                    )?;
                } else {
                    Output::info(format!(
                        "'{}.{}' not in system manifest, no PR needed",
                        schema, key
                    ));
                }
            }
        }
        GSettingAction::List { format } => {
            let system = GSettingsManifest::load_system()?;
            let user = GSettingsManifest::load_user()?;
            let merged = GSettingsManifest::merged(&system, &user);

            if format == "json" {
                println!("{}", serde_json::to_string_pretty(&merged)?);
            } else {
                if merged.settings.is_empty() {
                    Output::info("No gsettings in manifest.");
                    return Ok(());
                }

                Output::subheader("GSETTINGS:");
                println!(
                    "{:<45} {:<20} {:<10} {}",
                    "SCHEMA.KEY".cyan(),
                    "VALUE".cyan(),
                    "SOURCE".cyan(),
                    "CURRENT".cyan()
                );
                Output::separator();

                for setting in &merged.settings {
                    let source = if user.find(&setting.schema, &setting.key).is_some() {
                        "user".yellow().to_string()
                    } else {
                        "system".dimmed().to_string()
                    };
                    let current = get_current_value(&setting.schema, &setting.key)
                        .unwrap_or_else(|| "(unset)".to_string());
                    let matches = if current == setting.value {
                        "✓".green().to_string()
                    } else {
                        "≠".yellow().to_string()
                    };

                    println!(
                        "{:<45} {:<20} {:<10} {} {}",
                        format!("{}.{}", setting.schema, setting.key),
                        truncate(&setting.value, 18),
                        source,
                        matches,
                        truncate(&current, 15)
                    );
                }

                Output::blank();
                Output::info(format!(
                    "{} settings ({} system, {} user)",
                    merged.settings.len(),
                    system.settings.len(),
                    user.settings.len()
                ));
            }
        }
        GSettingAction::Apply => {
            // Use the new Plan-based implementation
            let cwd = std::env::current_dir()?;
            let plan_ctx = PlanContext::new(cwd, plan.clone());

            let apply_plan = GsettingApplyCommand.plan(&plan_ctx)?;

            if apply_plan.is_empty() {
                Output::success("All settings are already applied.");
                return Ok(());
            }

            // Always show the plan
            print!("{}", apply_plan.describe());

            if plan.dry_run {
                return Ok(());
            }

            // Execute the plan
            let mut exec_ctx = ExecuteContext::new(plan.clone());
            let report = apply_plan.execute(&mut exec_ctx)?;
            print!("{}", report);
        }
        GSettingAction::Capture { schema, key, apply } => {
            // Validate schema exists
            validate_gsettings_schema(&schema)?;

            // Use the Plan-based capture implementation
            let cwd = std::env::current_dir()?;
            let plan_ctx = PlanContext::new(cwd, plan.clone());

            let capture_plan = GsettingCaptureCommand {
                schema: schema.clone(),
                key: key.clone(),
            }
            .plan(&plan_ctx)?;

            if capture_plan.is_empty() {
                Output::success("All settings are already in the manifest.");
                return Ok(());
            }

            // Always show the plan
            print!("{}", capture_plan.describe());

            if plan.dry_run || !apply {
                if !apply && !plan.dry_run {
                    Output::hint("Use --apply to execute this plan.");
                }
                return Ok(());
            }

            // Execute the plan
            let mut exec_ctx = ExecuteContext::new(plan.clone());
            let report = capture_plan.execute(&mut exec_ctx)?;
            print!("{}", report);
        }
    }
    Ok(())
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else if max == 0 {
        String::new()
    } else {
        let truncated: String = s.chars().take(max - 1).collect();
        format!("{}…", truncated)
    }
}

// ============================================================================
// Plan-based GSettings Apply Implementation
// ============================================================================

/// A setting that needs to be applied.
#[derive(Debug, Clone)]
pub struct SettingToApply {
    /// The gsetting entry.
    pub setting: GSetting,
    /// Current value on the system.
    pub current: Option<String>,
}

/// Command to apply all GSettings from manifests.
pub struct GsettingApplyCommand;

/// Plan for applying GSettings.
pub struct GsettingApplyPlan {
    /// Settings to apply (value differs from current).
    pub to_apply: Vec<SettingToApply>,
    /// Settings already in sync.
    pub already_set: usize,
}

impl Plannable for GsettingApplyCommand {
    type Plan = GsettingApplyPlan;

    fn plan(&self, _ctx: &PlanContext) -> Result<Self::Plan> {
        // Load and merge manifests (read-only, no side effects)
        let system = GSettingsManifest::load_system()?;
        let user = GSettingsManifest::load_user()?;
        let merged = GSettingsManifest::merged(&system, &user);

        let mut to_apply = Vec::new();
        let mut already_set = 0;

        for setting in merged.settings {
            let current = get_current_value(&setting.schema, &setting.key);

            if current.as_deref() == Some(&setting.value) {
                already_set += 1;
            } else {
                to_apply.push(SettingToApply { setting, current });
            }
        }

        Ok(GsettingApplyPlan {
            to_apply,
            already_set,
        })
    }
}

impl Plan for GsettingApplyPlan {
    fn describe(&self) -> PlanSummary {
        let mut summary = PlanSummary::new(format!(
            "GSettings Apply: {} to set, {} already in sync",
            self.to_apply.len(),
            self.already_set
        ));

        for item in &self.to_apply {
            let current_display = item.current.as_deref().unwrap_or("(unset)");
            summary.add_operation(Operation::with_details(
                Verb::Set,
                format!("gsetting:{}.{}", item.setting.schema, item.setting.key),
                format!("{} → {}", current_display, item.setting.value),
            ));
        }

        summary
    }

    fn execute(self, _ctx: &mut ExecuteContext) -> Result<ExecutionReport> {
        let mut report = ExecutionReport::new();

        for item in self.to_apply {
            let target = format!("{}.{}", item.setting.schema, item.setting.key);

            match set_gsetting(&item.setting.schema, &item.setting.key, &item.setting.value) {
                Ok(true) => {
                    report.record_success(Verb::Set, format!("gsetting:{}", target));
                }
                Ok(false) => {
                    report.record_failure(
                        Verb::Set,
                        format!("gsetting:{}", target),
                        "gsettings command failed",
                    );
                }
                Err(e) => {
                    report.record_failure(Verb::Set, format!("gsetting:{}", target), e.to_string());
                }
            }
        }

        Ok(report)
    }

    fn is_empty(&self) -> bool {
        self.to_apply.is_empty()
    }
}

// ============================================================================
// Plan-based GSettings Capture Implementation
// ============================================================================

/// Get all keys for a schema.
fn get_schema_keys(schema: &str) -> Vec<String> {
    let output = Command::new("gsettings")
        .args(["list-keys", schema])
        .output();

    match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .lines()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

/// A setting to capture (add to manifest).
#[derive(Debug, Clone)]
pub struct SettingToCapture {
    /// The gsetting entry.
    pub setting: GSetting,
}

/// Command to capture GSettings to manifest.
pub struct GsettingCaptureCommand {
    /// Schema to capture from.
    pub schema: String,
    /// Specific key (or all keys if None).
    pub key: Option<String>,
}

/// Plan for capturing GSettings.
pub struct GsettingCapturePlan {
    /// Settings to add to manifest.
    pub to_capture: Vec<SettingToCapture>,
    /// Settings already in manifest.
    pub already_in_manifest: usize,
}

impl Plannable for GsettingCaptureCommand {
    type Plan = GsettingCapturePlan;

    fn plan(&self, _ctx: &PlanContext) -> Result<Self::Plan> {
        // Get keys to capture
        let keys = if let Some(ref key) = self.key {
            vec![key.clone()]
        } else {
            get_schema_keys(&self.schema)
        };

        // Load manifests to see what's already tracked
        let system = GSettingsManifest::load_system()?;
        let user = GSettingsManifest::load_user()?;
        let merged = GSettingsManifest::merged(&system, &user);

        let mut to_capture = Vec::new();
        let mut already_in_manifest = 0;

        for key in keys {
            if merged.find(&self.schema, &key).is_some() {
                already_in_manifest += 1;
            } else if let Some(value) = get_current_value(&self.schema, &key) {
                to_capture.push(SettingToCapture {
                    setting: GSetting {
                        schema: self.schema.clone(),
                        key,
                        value,
                        comment: None,
                    },
                });
            }
        }

        // Sort for consistent output
        to_capture.sort_by(|a, b| a.setting.key.cmp(&b.setting.key));

        Ok(GsettingCapturePlan {
            to_capture,
            already_in_manifest,
        })
    }
}

impl Plan for GsettingCapturePlan {
    fn describe(&self) -> PlanSummary {
        let mut summary = PlanSummary::new(format!(
            "GSettings Capture: {} to add, {} already in manifest",
            self.to_capture.len(),
            self.already_in_manifest
        ));

        for item in &self.to_capture {
            summary.add_operation(Operation::with_details(
                Verb::Capture,
                format!("gsetting:{}.{}", item.setting.schema, item.setting.key),
                truncate(&item.setting.value, 30),
            ));
        }

        summary
    }

    fn execute(self, _ctx: &mut ExecuteContext) -> Result<ExecutionReport> {
        let mut report = ExecutionReport::new();

        // Load user manifest (we add captured settings there)
        let mut user = GSettingsManifest::load_user()?;

        for item in self.to_capture {
            let target = format!("{}.{}", item.setting.schema, item.setting.key);
            user.upsert(item.setting);
            report.record_success(Verb::Capture, format!("gsetting:{}", target));
        }

        // Save the updated manifest
        user.save_user()?;

        Ok(report)
    }

    fn is_empty(&self) -> bool {
        self.to_capture.is_empty()
    }
}

// ============================================================================
// SystemComponent Implementation
// ============================================================================

/// The GSettings system component.
///
/// GSettings is unique in that we cannot enumerate "all settings" on the system.
/// The `scan_system()` method returns current values for settings in the manifest.
/// Capture requires explicit schema filtering (handled via `CaptureFilter`).
#[derive(Debug, Clone, Default)]
pub struct GSettingComponent;

impl GSettingComponent {
    /// Create a new GSettingComponent.
    pub fn new() -> Self {
        Self
    }

    /// Get current value of a gsetting from the system.
    #[allow(dead_code)]
    pub fn get_current_value(schema: &str, key: &str) -> Option<String> {
        get_current_value(schema, key)
    }
}

impl SystemComponent for GSettingComponent {
    type Item = GSetting;
    type Manifest = GSettingsManifest;
    /// Capture filter: list of schema names to capture. Empty = capture nothing.
    type CaptureFilter = Vec<String>;

    fn name(&self) -> &'static str {
        "GSettings"
    }

    fn scan_system(&self) -> Result<Vec<Self::Item>> {
        // For GSettings, we scan the system values for items in our manifest
        // (we can't enumerate all possible settings)
        let manifest = self.load_manifest()?;

        let mut system_values = Vec::new();
        for setting in &manifest.settings {
            if let Some(value) = get_current_value(&setting.schema, &setting.key) {
                system_values.push(GSetting {
                    schema: setting.schema.clone(),
                    key: setting.key.clone(),
                    value,
                    comment: None,
                });
            }
        }

        Ok(system_values)
    }

    fn load_manifest(&self) -> Result<Self::Manifest> {
        let system = GSettingsManifest::load_system()?;
        let user = GSettingsManifest::load_user()?;
        Ok(GSettingsManifest::merged(&system, &user))
    }

    fn manifest_items(&self, manifest: &Self::Manifest) -> Vec<Self::Item> {
        manifest.settings.clone()
    }

    fn diff(&self, system: &[Self::Item], manifest: &Self::Manifest) -> DriftReport<Self::Item> {
        let manifest_items = self.manifest_items(manifest);

        // Build lookup map for system values
        let system_map: std::collections::HashMap<String, &GSetting> =
            system.iter().map(|s| (s.unique_key(), s)).collect();

        let mut to_install = Vec::new(); // Settings with missing schema/key
        let mut to_update = Vec::new(); // Settings with different values
        let mut synced_count = 0;

        for manifest_setting in &manifest_items {
            match system_map.get(&manifest_setting.unique_key()) {
                None => {
                    // Schema/key not found on system
                    to_install.push(manifest_setting.clone());
                }
                Some(sys_setting) => {
                    if sys_setting.value != manifest_setting.value {
                        to_update.push(((*sys_setting).clone(), manifest_setting.clone()));
                    } else {
                        synced_count += 1;
                    }
                }
            }
        }

        // GSettings never has "untracked" items - we can't enumerate all settings
        DriftReport {
            to_install,
            untracked: Vec::new(),
            to_update,
            synced_count,
        }
    }

    fn supports_capture(&self) -> bool {
        true
    }

    fn capture(
        &self,
        _system: &[Self::Item],
        filter: Self::CaptureFilter,
    ) -> Option<Result<Self::Manifest>> {
        if filter.is_empty() {
            return None; // Capture requires explicit schemas
        }

        // Capture all keys from specified schemas
        let mut settings = Vec::new();
        for schema in &filter {
            let keys = get_schema_keys(schema);
            for key in keys {
                if let Some(value) = get_current_value(schema, &key) {
                    settings.push(GSetting {
                        schema: schema.clone(),
                        key,
                        value,
                        comment: None,
                    });
                }
            }
        }

        Some(Ok(GSettingsManifest {
            schema: None,
            settings,
        }))
    }
}
