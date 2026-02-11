# RFC-0008: Command Infrastructure

## Summary

Introduce a `Plan`-centric command infrastructure where all operations are first computed as immutable plans, then optionally executed. This enables dry-run for free, composability, and clean separation between analysis and mutation.

## Motivation

Current command implementations mix analysis and execution:

```rust
// Current pattern (problematic)
fn handle_sync(dry_run: bool) -> Result<()> {
    let missing = find_missing_packages()?;
    for pkg in missing {
        if dry_run {
            println!("Would install: {}", pkg);
        } else {
            install_package(&pkg)?;
        }
    }
    Ok(())
}
```

This has several issues:

1. **Ad-hoc branching** - Every command reimplements dry-run logic
2. **No composability** - Can't combine multiple commands into one plan
3. **Testing difficulty** - Must mock side effects or run real commands
4. **No preview** - Can't inspect what will happen before deciding

## Design

### Core Trait: `Plannable`

```rust
/// A command that can produce a plan without side effects.
pub trait Plannable {
    /// The plan type this command produces.
    type Plan: Plan;

    /// Analyze the current state and produce a plan.
    /// This method MUST NOT have side effects.
    fn plan(&self, ctx: &PlanContext) -> Result<Self::Plan>;
}
```

### Core Trait: `Plan`

```rust
/// An immutable description of operations to perform.
pub trait Plan: Sized {
    /// Human-readable description for dry-run output.
    fn describe(&self) -> PlanSummary;

    /// Execute the plan, performing all side effects.
    fn execute(self, ctx: &mut ExecuteContext) -> Result<ExecutionReport>;

    /// Returns true if this plan has no operations.
    fn is_empty(&self) -> bool;
}

/// Structured summary of a plan for display.
pub struct PlanSummary {
    pub summary: String,
    pub operations: Vec<Operation>,
}

pub struct Operation {
    pub verb: Verb,        // Install, Remove, Enable, Disable, Set, Create, etc.
    pub target: String,    // "flatpak:org.gnome.Boxes", "extension:dash-to-dock", etc.
    pub details: Option<String>,
}

pub enum Verb {
    Install,
    Remove,
    Enable,
    Disable,
    Set,
    Create,
    Delete,
    Update,
    Capture,
    Skip,
}
```

### Composite Plans

Plans can be combined. The actual implementation uses type erasure via `Box<dyn DynPlan>`
to enable heterogeneous plan composition (different plan types in the same composite):

```rust
/// Internal trait for object-safe plan operations.
/// Enables heterogeneous plan composition via Box<dyn DynPlan>.
trait DynPlan {
    fn describe_dyn(&self) -> PlanSummary;
    fn execute_dyn(self: Box<Self>, ctx: &mut ExecuteContext) -> Result<ExecutionReport>;
    fn is_empty_dyn(&self) -> bool;
}

/// A plan that combines multiple sub-plans of different types.
pub struct CompositePlan {
    name: String,
    plans: Vec<Box<dyn DynPlan>>,
}

impl CompositePlan {
    pub fn new(name: impl Into<String>) -> Self { ... }
    
    /// Add a plan to the composite. Empty plans are filtered out.
    pub fn add<P: Plan + 'static>(&mut self, plan: P) { ... }
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
            report.merge(plan.execute_dyn(ctx)?);
        }
        Ok(report)
    }

    fn is_empty(&self) -> bool {
        self.plans.is_empty()
    }
}
}
```

### Type-Erased Plans

For heterogeneous composition (different plan types):

```rust
/// Type-erased plan for dynamic composition.
pub struct BoxedPlan(Box<dyn DynPlan>);

trait DynPlan {
    fn describe(&self) -> PlanDescription;
    fn execute_boxed(self: Box<Self>, ctx: &mut ExecuteContext) -> Result<ExecutionReport>;
    fn is_empty(&self) -> bool;
}

impl<P: Plan + 'static> DynPlan for P {
    fn describe(&self) -> PlanDescription { self.describe() }
    fn execute_boxed(self: Box<Self>, ctx: &mut ExecuteContext) -> Result<ExecutionReport> {
        (*self).execute(ctx)
    }
    fn is_empty(&self) -> bool { self.is_empty() }
}

impl BoxedPlan {
    pub fn new<P: Plan + 'static>(plan: P) -> Self {
        BoxedPlan(Box::new(plan))
    }
}
```

## Example: Flatpak Sync

```rust
pub struct FlatpakSyncCommand {
    pub remote: String,
}

pub struct FlatpakSyncPlan {
    pub to_install: Vec<FlatpakApp>,
    pub to_remove: Vec<FlatpakApp>,
    pub already_synced: Vec<FlatpakApp>,
}

impl Plannable for FlatpakSyncCommand {
    type Plan = FlatpakSyncPlan;

    fn plan(&self, ctx: &PlanContext) -> Result<Self::Plan> {
        let manifest = FlatpakManifest::load_merged(ctx.manifest_dir())?;
        let installed = query_installed_flatpaks()?;

        let to_install: Vec<_> = manifest.apps()
            .filter(|app| !installed.contains(&app.id))
            .cloned()
            .collect();

        let to_remove: Vec<_> = installed.iter()
            .filter(|app| !manifest.contains(&app.id))
            .cloned()
            .collect();

        Ok(FlatpakSyncPlan {
            to_install,
            to_remove,
            already_synced: /* ... */,
        })
    }
}

impl Plan for FlatpakSyncPlan {
    fn describe(&self) -> PlanDescription {
        let mut ops = Vec::new();

        for app in &self.to_install {
            ops.push(Operation {
                verb: Verb::Install,
                target: format!("flatpak:{}", app.id),
                details: app.name.clone(),
            });
        }

        for app in &self.to_remove {
            ops.push(Operation {
                verb: Verb::Remove,
                target: format!("flatpak:{}", app.id),
                details: None,
            });
        }

        PlanDescription {
            summary: format!(
                "Flatpak: {} to install, {} to remove, {} in sync",
                self.to_install.len(),
                self.to_remove.len(),
                self.already_synced.len(),
            ),
            operations: ops,
        }
    }

    fn execute(self, ctx: &mut ExecuteContext) -> Result<ExecutionReport> {
        let mut report = ExecutionReport::default();

        for app in self.to_install {
            ctx.run(&["flatpak", "install", "-y", &app.id])?;
            report.record_success(Verb::Install, &app.id);
        }

        for app in self.to_remove {
            ctx.run(&["flatpak", "uninstall", "-y", &app.id])?;
            report.record_success(Verb::Remove, &app.id);
        }

        Ok(report)
    }

    fn is_empty(&self) -> bool {
        self.to_install.is_empty() && self.to_remove.is_empty()
    }
}
```

## Example: `bkt apply`

```rust
pub struct ApplyCommand {
    pub subset: Option<Vec<Subsystem>>,
}

pub struct ApplyPlan {
    plans: Vec<BoxedPlan>,
}

impl Plannable for ApplyCommand {
    type Plan = ApplyPlan;

    fn plan(&self, ctx: &PlanContext) -> Result<Self::Plan> {
        let mut plans = Vec::new();

        let subsystems = self.subset.as_ref()
            .cloned()
            .unwrap_or_else(|| vec![
                Subsystem::Flatpak,
                Subsystem::Extension,
                Subsystem::GSetting,
                Subsystem::Dnf,
                Subsystem::Shim,
            ]);

        for subsystem in subsystems {
            let plan: BoxedPlan = match subsystem {
                Subsystem::Flatpak => {
                    BoxedPlan::new(FlatpakSyncCommand::default().plan(ctx)?)
                }
                Subsystem::Extension => {
                    BoxedPlan::new(ExtensionSyncCommand::default().plan(ctx)?)
                }
                Subsystem::GSetting => {
                    BoxedPlan::new(GSettingApplyCommand::default().plan(ctx)?)
                }
                Subsystem::Dnf => {
                    BoxedPlan::new(DnfSyncCommand::default().plan(ctx)?)
                }
                Subsystem::Shim => {
                    BoxedPlan::new(ShimSyncCommand::default().plan(ctx)?)
                }
            };
            plans.push(plan);
        }

        Ok(ApplyPlan { plans })
    }
}
```

## Example: Capture (System → Manifest)

```rust
pub struct FlatpakCaptureCommand;

pub struct FlatpakCapturePlan {
    pub to_capture: Vec<FlatpakApp>,
    pub already_tracked: Vec<FlatpakApp>,
}

impl Plannable for FlatpakCaptureCommand {
    type Plan = FlatpakCapturePlan;

    fn plan(&self, ctx: &PlanContext) -> Result<Self::Plan> {
        let manifest = FlatpakManifest::load_user(ctx.manifest_dir())?;
        let installed = query_installed_flatpaks()?;

        let to_capture: Vec<_> = installed.into_iter()
            .filter(|app| !manifest.contains(&app.id))
            .collect();

        Ok(FlatpakCapturePlan {
            to_capture,
            already_tracked: manifest.apps().cloned().collect(),
        })
    }
}

impl Plan for FlatpakCapturePlan {
    fn describe(&self) -> PlanDescription {
        PlanDescription {
            summary: format!(
                "Flatpak capture: {} to add to manifest",
                self.to_capture.len()
            ),
            operations: self.to_capture.iter().map(|app| Operation {
                verb: Verb::Capture,
                target: format!("flatpak:{}", app.id),
                details: app.name.clone(),
            }).collect(),
        }
    }

    fn execute(self, ctx: &mut ExecuteContext) -> Result<ExecutionReport> {
        let mut manifest = FlatpakManifest::load_user(ctx.manifest_dir())?;
        let mut report = ExecutionReport::default();

        for app in self.to_capture {
            manifest.add(app.clone());
            report.record_success(Verb::Capture, &app.id);
        }

        manifest.save()?;
        Ok(report)
    }

    fn is_empty(&self) -> bool {
        self.to_capture.is_empty()
    }
}
```

## CLI Integration

```rust
fn main() -> Result<()> {
    let args = Cli::parse();
    let ctx = PlanContext::new(&args)?;

    match args.command {
        Command::Apply { dry_run, subset } => {
            let cmd = ApplyCommand { subset };
            let plan = cmd.plan(&ctx)?;

            if plan.is_empty() {
                println!("Nothing to do. System is in sync with manifests.");
                return Ok(());
            }

            // Always show the plan
            print_plan(&plan.describe());

            if dry_run {
                return Ok(());
            }

            // Confirm and execute
            if !confirm("Proceed with these changes?")? {
                return Ok(());
            }

            let mut exec_ctx = ExecuteContext::new(&args)?;
            let report = plan.execute(&mut exec_ctx)?;
            print_report(&report);
        }

        Command::Capture { dry_run } => {
            let cmd = CaptureCommand::default();
            let plan = cmd.plan(&ctx)?;

            if plan.is_empty() {
                println!("Nothing to capture. Manifests match system state.");
                return Ok(());
            }

            print_plan(&plan.describe());

            if dry_run {
                return Ok(());
            }

            // ... execute
        }

        // ... other commands
    }
}
```

## Plan Display Format

```
$ bkt apply --dry-run

╭─ Apply Plan ─────────────────────────────────────────────────────╮
│                                                                  │
│  Flatpak: 2 to install, 0 to remove                              │
│    ▸ Install flatpak:org.gnome.Boxes (GNOME Boxes)               │
│    ▸ Install flatpak:org.gnome.Calculator (Calculator)           │
│                                                                  │
│  Extensions: 1 to enable                                         │
│    ▸ Enable extension:dash-to-dock@micxgx.gmail.com              │
│                                                                  │
│  GSettings: 3 to set                                             │
│    ▸ Set gsetting:org.gnome.desktop.interface.color-scheme       │
│    ▸ Set gsetting:org.gnome.desktop.interface.gtk-theme          │
│    ▸ Set gsetting:org.gnome.desktop.wm.preferences.button-layout │
│                                                                  │
╰──────────────────────────────────────────── 6 operations total ──╯

Run without --dry-run to apply these changes.
```

## Benefits

1. **Dry-run is free**: Just don't call `execute()`
2. **Composability**: `bkt apply` collects plans without custom logic
3. **Inspectability**: Plans can be serialized, logged, diffed
4. **Testability**: Assert on plan contents without side effects
5. **Transactionality**: Could extend to rollback on failure
6. **Reporting**: Unified execution reports across all commands

## Future Extensions

### Plan Serialization

```rust
impl Serialize for PlanDescription {
    // JSON/YAML output for automation
}

// bkt apply --dry-run --output=json
```

### Plan Diffing

```rust
// Compare two plans
let diff = old_plan.diff(&new_plan);
// Shows what changed between runs
```

### Confirmation Levels

```rust
pub enum ConfirmLevel {
    Never,           // --yes
    OnDestructive,   // Default: confirm removes
    Always,          // --confirm
}
```

### Parallel Execution

```rust
impl Plan for CompositePlan {
    fn execute(self, ctx: &mut ExecuteContext) -> Result<ExecutionReport> {
        // Plans that don't conflict can run in parallel
        self.plans.par_iter().map(|p| p.execute(ctx)).collect()
    }
}
```
