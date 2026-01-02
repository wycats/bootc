//! Doctor command implementation.
//!
//! Runs pre-flight checks and reports system readiness.

use crate::output::Output;
use crate::pr::run_preflight_checks;
use anyhow::Result;
use clap::Args;

#[derive(Debug, Args)]
pub struct DoctorArgs {
    /// Output format (table, json)
    #[arg(short, long, default_value = "table")]
    format: String,
}

pub fn run(args: DoctorArgs) -> Result<()> {
    let results = run_preflight_checks()?;

    if args.format == "json" {
        let json_results: Vec<_> = results
            .iter()
            .map(|r| {
                serde_json::json!({
                    "name": r.name,
                    "passed": r.passed,
                    "message": r.message,
                    "fix_hint": r.fix_hint,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&json_results)?);
        return Ok(());
    }

    Output::header("bkt doctor - checking system readiness");
    Output::blank();

    let mut all_passed = true;
    for result in &results {
        if result.passed {
            Output::success(format!("{}: {}", result.name, result.message));
        } else {
            Output::error(format!("{}: {}", result.name, result.message));
            if let Some(hint) = &result.fix_hint {
                Output::hint(hint);
            }
            all_passed = false;
        }
    }

    Output::blank();
    if all_passed {
        Output::success("All checks passed! Ready to use bkt --pr workflows.");
    } else {
        Output::error("Some checks failed. Fix the issues above to enable --pr workflows.");
    }

    Ok(())
}
