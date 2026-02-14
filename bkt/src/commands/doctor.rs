//! Doctor command implementation.
//!
//! Runs pre-flight checks and reports system readiness.

use crate::command_runner::RealCommandRunner;
use crate::output::Output;
use crate::pr::run_preflight_checks;
use anyhow::Result;
use clap::Args;
use std::path::{Path, PathBuf};

#[derive(Debug, Args)]
pub struct DoctorArgs {
    /// Output format (table, json)
    #[arg(short, long, default_value = "table")]
    format: String,
}

pub fn run(args: DoctorArgs) -> Result<()> {
    let runner = RealCommandRunner;
    let mut results = run_preflight_checks(&runner)?;

    // Additional environment readiness checks (not specific to PR workflows).
    results.push(check_distrobox_shims_path());
    results.push(check_distrobox_wrappers());
    results.push(check_devtools_resolve_to_distrobox("cargo"));
    results.push(check_devtools_resolve_to_distrobox("node"));
    results.push(check_devtools_resolve_to_distrobox("pnpm"));

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

fn home_dir() -> Option<PathBuf> {
    directories::BaseDirs::new()
        .map(|d| d.home_dir().to_path_buf())
        .or_else(|| std::env::var("HOME").ok().map(PathBuf::from))
}

fn pass(name: &str, message: &str) -> crate::pr::PreflightResult {
    crate::pr::PreflightResult {
        name: name.to_string(),
        passed: true,
        message: message.to_string(),
        fix_hint: None,
    }
}

fn fail(name: &str, message: &str, fix_hint: &str) -> crate::pr::PreflightResult {
    crate::pr::PreflightResult {
        name: name.to_string(),
        passed: false,
        message: message.to_string(),
        fix_hint: if fix_hint.trim().is_empty() {
            None
        } else {
            Some(fix_hint.to_string())
        },
    }
}

fn split_path_var() -> Vec<PathBuf> {
    let raw = std::env::var("PATH").unwrap_or_default();
    raw.split(':')
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .collect()
}

fn find_first_executable(cmd: &str, path: &[PathBuf]) -> Option<PathBuf> {
    for dir in path {
        let candidate = dir.join(cmd);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn index_of_path(path: &[PathBuf], needle: &Path) -> Option<usize> {
    path.iter().position(|p| p == needle)
}

fn check_distrobox_shims_path() -> crate::pr::PreflightResult {
    let Some(home) = home_dir() else {
        return fail(
            "PATH (distrobox shims)",
            "Cannot determine $HOME",
            "Ensure HOME is set",
        );
    };

    let distrobox_dir = home.join(".local/bin/distrobox");
    let cargo_dir = home.join(".cargo/bin");
    let proto_shims = home.join(".proto/shims");

    let path = split_path_var();
    let distrobox_idx = index_of_path(&path, &distrobox_dir);

    if distrobox_idx.is_none() {
        return fail(
            "PATH (distrobox shims)",
            "~/.local/bin/distrobox is not on PATH",
            "Run: bkt skel sync --force\nThen relogin (or restart user session) so environment.d is picked up",
        );
    }

    let distrobox_idx = distrobox_idx.unwrap();

    // If host toolchain paths are ahead of distrobox shims, you can accidentally
    // build/run on the host and accumulate state in $HOME.
    let mut offenders: Vec<String> = Vec::new();
    if let Some(i) = index_of_path(&path, &proto_shims)
        && i < distrobox_idx
    {
        offenders.push("~/.proto/shims".to_string());
    }
    if let Some(i) = index_of_path(&path, &cargo_dir)
        && i < distrobox_idx
    {
        offenders.push("~/.cargo/bin".to_string());
    }

    if !offenders.is_empty() {
        return fail(
            "PATH (devtools precedence)",
            &format!(
                "Host toolchain paths precede distrobox shims: {}",
                offenders.join(", ")
            ),
            "Move ~/.local/bin/distrobox earlier in PATH (preferably via environment.d)\nOptionally prune host toolchains: scripts/prune-host-devtools",
        );
    }

    pass(
        "PATH (distrobox shims)",
        "~/.local/bin/distrobox is present and has precedence",
    )
}

fn check_devtools_resolve_to_distrobox(cmd: &str) -> crate::pr::PreflightResult {
    let Some(home) = home_dir() else {
        return fail(
            &format!("{} resolution", cmd),
            "Cannot determine $HOME",
            "Ensure HOME is set",
        );
    };

    let distrobox_dir = home.join(".local/bin/distrobox");
    let expected = distrobox_dir.join(cmd);
    if !expected.exists() {
        // If the shim doesn't exist, don't hard-fail: some hosts won't need every tool.
        return pass(
            &format!("{} resolution", cmd),
            &format!("No distrobox shim for {} (ok if unused)", cmd),
        );
    }

    let path = split_path_var();
    let resolved = find_first_executable(cmd, &path);
    let Some(resolved) = resolved else {
        return fail(
            &format!("{} resolution", cmd),
            &format!("{} not found on PATH", cmd),
            "Ensure distrobox shims are exported and PATH includes ~/.local/bin/distrobox",
        );
    };

    // Compare by string form to avoid surprises with non-normalized paths.
    let resolved_s = resolved.to_string_lossy();
    let expected_s = expected.to_string_lossy();
    if resolved_s != expected_s {
        return fail(
            &format!("{} resolution", cmd),
            &format!("{} resolves to {}", cmd, resolved_s),
            &format!(
                "Expected {} (distrobox shim)\nFix PATH precedence or remove host toolchains that shadow it",
                expected_s
            ),
        );
    }

    pass(
        &format!("{} resolution", cmd),
        &format!("{} resolves to distrobox shim", cmd),
    )
}

/// Check that all files in ~/.local/bin/distrobox are valid wrapper scripts.
fn check_distrobox_wrappers() -> crate::pr::PreflightResult {
    let Some(home) = home_dir() else {
        return fail(
            "distrobox wrappers",
            "Cannot determine $HOME",
            "Ensure HOME is set",
        );
    };

    let distrobox_dir = home.join(".local/bin/distrobox");
    if !distrobox_dir.exists() {
        return pass(
            "distrobox wrappers",
            "~/.local/bin/distrobox does not exist (ok if unused)",
        );
    }

    let entries = match std::fs::read_dir(&distrobox_dir) {
        Ok(e) => e,
        Err(e) => {
            return fail(
                "distrobox wrappers",
                &format!("Cannot read ~/.local/bin/distrobox: {}", e),
                "",
            );
        }
    };

    let mut non_wrappers: Vec<String> = Vec::new();
    let mut unreadable: Vec<String> = Vec::new();

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                unreadable.push(format!("(entry error: {})", e));
                continue;
            }
        };

        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.display().to_string());

        // Read first 512 bytes to check for marker (avoids loading large binaries)
        let check_result = std::fs::File::open(&path).and_then(|mut file| {
            use std::io::Read;
            let mut buffer = [0u8; 512];
            let n = file.read(&mut buffer)?;
            let content = String::from_utf8_lossy(&buffer[..n]);
            Ok(content.contains("distrobox_binary"))
        });

        match check_result {
            Ok(true) => {} // Valid wrapper
            Ok(false) => non_wrappers.push(file_name),
            Err(_) => unreadable.push(file_name),
        }
    }

    if non_wrappers.is_empty() && unreadable.is_empty() {
        pass(
            "distrobox wrappers",
            "All files in ~/.local/bin/distrobox are valid wrappers",
        )
    } else {
        let mut problems = Vec::new();
        if !non_wrappers.is_empty() {
            problems.push(format!("non-wrapper files: {}", non_wrappers.join(", ")));
        }
        if !unreadable.is_empty() {
            problems.push(format!("unreadable files: {}", unreadable.join(", ")));
        }
        fail(
            "distrobox wrappers",
            &problems.join("; "),
            "Remove these files and re-export from distrobox:\n  rm ~/.local/bin/distrobox/<file>\n  bkt distrobox apply",
        )
    }
}
