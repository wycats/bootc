//! Doctor command implementation.
//!
//! Runs pre-flight checks and reports system readiness.

use crate::command_runner::RealCommandRunner;
use crate::daemon;
use crate::manifest::DistroboxManifest;
use crate::output::Output;
use crate::pr::run_preflight_checks;
use crate::repo::find_repo_path;
use anyhow::{Context, Result};
use clap::Args;
use std::path::{Path, PathBuf};

#[derive(Debug, Args)]
pub struct DoctorArgs {
    /// Output format (table, json)
    #[arg(short, long, default_value = "table")]
    format: String,

    /// Attempt to automatically fix known issues
    #[arg(long)]
    fix: bool,
}

pub fn run(args: DoctorArgs) -> Result<()> {
    if args.fix && args.format == "json" {
        anyhow::bail!("--fix cannot be used with --format json");
    }

    let runner = RealCommandRunner;
    let mut results = collect_results(&runner)?;

    if args.fix {
        apply_known_fixes(&results)?;
        results = collect_results(&runner)?;
    }

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

fn collect_results(runner: &RealCommandRunner) -> Result<Vec<crate::pr::PreflightResult>> {
    let mut results = run_preflight_checks(runner)?;

    // Additional environment readiness checks (not specific to PR workflows).
    results.push(check_distrobox_shims_path());
    results.push(check_distrobox_wrappers());
    results.push(check_cargo_bin_exports());
    results.push(check_devtools_resolve_to_distrobox("cargo"));
    results.push(check_devtools_resolve_to_distrobox("node"));
    results.push(check_devtools_resolve_to_distrobox("pnpm"));
    results.push(check_daemon_status());

    Ok(results)
}

fn apply_known_fixes(results: &[crate::pr::PreflightResult]) -> Result<()> {
    let mut has_wrapper_issue = false;
    let mut has_cargo_export_issue = false;

    for result in results {
        if result.passed {
            continue;
        }

        if result.name == "distrobox wrappers" {
            has_wrapper_issue = true;
        }
        if result.name == "cargo bin exports" {
            has_cargo_export_issue = true;
        }
    }

    if !has_wrapper_issue && !has_cargo_export_issue {
        Output::info("No auto-fixable doctor issues detected.");
        return Ok(());
    }

    if has_wrapper_issue {
        remove_non_wrapper_files()?;
    }

    if has_wrapper_issue || has_cargo_export_issue {
        Output::info("Running `bkt distrobox apply` to regenerate shims...");
        let current_exe = std::env::current_exe().context("Failed to locate current bkt binary")?;
        let status = std::process::Command::new(current_exe)
            .arg("distrobox")
            .arg("apply")
            .status()
            .context("Failed to run `bkt distrobox apply`")?;

        if !status.success() {
            anyhow::bail!("`bkt distrobox apply` failed while running doctor --fix");
        }
    }

    Ok(())
}

fn remove_non_wrapper_files() -> Result<()> {
    let Some(home) = home_dir() else {
        anyhow::bail!("Cannot determine $HOME");
    };

    let distrobox_dir = home.join(".local/bin/distrobox");
    if !distrobox_dir.exists() {
        return Ok(());
    }

    let files = list_non_wrapper_paths(&distrobox_dir)?;
    if files.is_empty() {
        return Ok(());
    }

    Output::info("Removing non-wrapper files from ~/.local/bin/distrobox...");
    for path in files {
        Output::step(format!("Removing {}", path.display()));
        std::fs::remove_file(&path)
            .with_context(|| format!("Failed to remove {}", path.display()))?;
    }

    Ok(())
}

fn list_non_wrapper_paths(distrobox_dir: &Path) -> Result<Vec<PathBuf>> {
    let entries = std::fs::read_dir(distrobox_dir)
        .with_context(|| format!("Cannot read {}", distrobox_dir.display()))?;

    let mut non_wrappers = Vec::new();

    for entry in entries {
        let entry =
            entry.with_context(|| format!("Cannot read entry in {}", distrobox_dir.display()))?;

        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        if !path_has_wrapper_marker(&path) {
            non_wrappers.push(path);
        }
    }

    Ok(non_wrappers)
}

fn path_has_wrapper_marker(path: &Path) -> bool {
    let check_result = std::fs::File::open(path).and_then(|mut file| {
        use std::io::Read;
        let mut buffer = [0u8; 512];
        let n = file.read(&mut buffer)?;
        let content = String::from_utf8_lossy(&buffer[..n]);
        Ok(content.contains("distrobox_binary"))
    });

    check_result.unwrap_or(false)
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

        match std::fs::File::open(&path) {
            Ok(_) => {
                if !path_has_wrapper_marker(&path) {
                    non_wrappers.push(file_name);
                }
            }
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

/// Check that cargo-installed binaries have corresponding distrobox shims.
///
/// When you run `cargo install --path <crate>` (via the distrobox cargo shim),
/// the binary lands in ~/.cargo/bin but isn't accessible on the host until
/// `bkt distrobox apply` creates a shim for it.
fn check_cargo_bin_exports() -> crate::pr::PreflightResult {
    let missing_shims = match list_missing_cargo_shims() {
        Ok(missing) => missing,
        Err(e) => {
            return fail("cargo bin exports", &e.to_string(), "");
        }
    };

    if missing_shims.is_empty() {
        pass(
            "cargo bin exports",
            "All cargo-installed binaries have distrobox shims",
        )
    } else {
        fail(
            "cargo bin exports",
            &format!(
                "Binaries in ~/.cargo/bin without shims: {}",
                missing_shims.join(", ")
            ),
            "Run `bkt distrobox apply` to create shims for these binaries",
        )
    }
}

/// Load the distrobox manifest's `bins.exclude` list for a given container.
///
/// Returns an empty vec if the manifest can't be loaded (e.g., not in a repo).
fn load_bins_exclude() -> Vec<String> {
    let repo_path = match find_repo_path() {
        Ok(p) => p,
        Err(_) => return Vec::new(),
    };

    let manifest = match DistroboxManifest::load_from_dir(&repo_path) {
        Ok(m) => m,
        Err(_) => return Vec::new(),
    };

    // Use the first container's exclude list (typically "bootc-dev")
    manifest
        .containers
        .values()
        .next()
        .map(|c| c.bins.exclude.clone())
        .unwrap_or_default()
}

fn list_missing_cargo_shims() -> Result<Vec<String>> {
    let Some(home) = home_dir() else {
        anyhow::bail!("Cannot determine $HOME");
    };

    let cargo_bin = home.join(".cargo/bin");
    let shims_dir = home.join(".local/bin/distrobox");

    if !cargo_bin.exists() {
        return Ok(Vec::new());
    }

    let entries = std::fs::read_dir(&cargo_bin)
        .with_context(|| format!("Cannot read {}", cargo_bin.display()))?;

    // Rustup-managed binaries are proxies, not real user-installed binaries
    let rustup_managed = [
        "cargo",
        "rustc",
        "rustdoc",
        "rust-gdb",
        "rust-gdbgui",
        "rust-lldb",
        "rustfmt",
        "cargo-fmt",
        "clippy-driver",
        "cargo-clippy",
        "rust-analyzer",
        "cargo-miri",
        "rls",
        "rustup",
    ];

    // Load manifest exclusions (e.g., bkt ships with the OS image)
    let excluded = load_bins_exclude();

    let mut missing_shims: Vec<String> = Vec::new();

    for entry in entries {
        let entry =
            entry.with_context(|| format!("Cannot read entry in {}", cargo_bin.display()))?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };

        // Skip rustup-managed binaries
        if rustup_managed.contains(&name) {
            continue;
        }

        // Skip manifest-excluded binaries (e.g., bkt — ships with the OS image)
        if excluded.iter().any(|e| e == name) {
            continue;
        }

        // Check if a shim exists
        let shim_path = shims_dir.join(name);
        if !shim_path.exists() {
            missing_shims.push(name.to_string());
        }
    }

    Ok(missing_shims)
}

/// Check if the bkt daemon is running and connectable.
fn check_daemon_status() -> crate::pr::PreflightResult {
    if daemon::daemon_available() {
        pass("bkt daemon", "Daemon is running and connectable")
    } else if daemon::daemon_socket_exists() {
        fail(
            "bkt daemon",
            "Daemon socket exists but is not connectable (stale?)",
            "Restart the daemon: systemctl --user restart bkt-daemon.service",
        )
    } else {
        fail(
            "bkt daemon",
            "Daemon is not running (~30x slower container→host delegation)",
            "Start the daemon: systemctl --user enable --now bkt-daemon.service",
        )
    }
}
