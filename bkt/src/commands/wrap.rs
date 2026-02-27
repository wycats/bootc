//! Application wrapper generation command.
//!
//! Generates Rust binaries that launch applications under systemd resource controls.
//! This replaces shell wrapper scripts with compiled binaries that have proper
//! error handling, argument passing, and VS Code remote-cli detection.

use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use std::fs;
use std::path::Path;
use std::process::Command;

use crate::output::Output;
use crate::repo::find_repo_path;

#[derive(Debug, Args)]
pub struct WrapArgs {
    #[command(subcommand)]
    pub action: WrapAction,
}

#[derive(Debug, Subcommand)]
pub enum WrapAction {
    /// Generate wrapper source files from manifest config
    ///
    /// Reads wrapper definitions from image-config.json and writes
    /// the corresponding Rust source files to wrappers/*/src/main.rs.
    /// Does not compile — the Containerfile handles compilation via rustc.
    Generate,

    /// Build all wrappers locally via cargo (dev convenience)
    Build,

    /// Check that committed wrapper source matches manifest config
    ///
    /// Exits with code 1 if any wrapper source file is out of sync.
    /// Used by CI to enforce that generated source is always current.
    Check,

    /// List configured wrappers
    List,
}

/// Configuration for a wrapper, as stored in the manifest
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct WrapperConfig {
    pub name: String,
    pub target: String,
    pub slice: String,
    pub output: String,
    #[serde(default)]
    pub remote_cli: bool,
    #[serde(default)]
    pub description: Option<String>,
}

impl WrapperConfig {
    /// Generate the Rust source code for this wrapper
    pub fn generate_source(&self) -> String {
        let description = self
            .description
            .clone()
            .unwrap_or_else(|| format!("{} (managed)", self.name));

        let remote_cli_code = if self.remote_cli {
            r#"
    // VS Code remote-cli passthrough
    if std::env::var("VSCODE_IPC_HOOK_CLI").is_ok() {
        if let Some(remote_cli) = find_remote_cli() {
            let err = std::process::Command::new(&remote_cli)
                .args(std::env::args().skip(1))
                .exec();
            eprintln!("Failed to exec remote-cli: {}", err);
            std::process::exit(1);
        }
    }
"#
        } else {
            ""
        };

        let remote_cli_fn = if self.remote_cli {
            r#"
fn find_remote_cli() -> Option<String> {
    let path = std::env::var("PATH").ok()?;
    for dir in path.split(':') {
        let candidate = format!("{}/code", dir);
        if candidate.contains("/remote-cli/") && std::path::Path::new(&candidate).exists() {
            return Some(candidate);
        }
    }
    None
}
"#
        } else {
            ""
        };

        format!(
            r#"//! Auto-generated wrapper by bkt wrap
//! Target: {target}
//! Slice: {slice}

use std::os::unix::process::CommandExt;

fn already_in_slice(slice: &str) -> bool {{
    std::fs::read_to_string("/proc/self/cgroup")
        .map(|s| s.contains(slice))
        .unwrap_or(false)
}}

fn main() {{

    // Re-entry guard: if already running inside our target slice, exec directly.
    // Without this, child processes that re-invoke the wrapper binary
    // would each create a new systemd-run scope, causing an infinite loop.
    if already_in_slice("{slice}") {{
        let err = std::process::Command::new("{target}")
            .args(std::env::args().skip(1))
            .exec();
        eprintln!("Failed to exec target: {{}}", err);
        std::process::exit(1);
    }}
{remote_cli_code}
    // Validate target exists
    let target = "{target}";
    if !std::path::Path::new(target).exists() {{
        eprintln!("Error: {{}} not found", target);
        std::process::exit(127);
    }}

    // Generate unique unit name
    let unit_name = format!("{name}-{{}}-{{}}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0)
    );

    // Launch via systemd-run
    let err = std::process::Command::new("systemd-run")
        .args([
            "--user",
            "--quiet",
            "--slice={slice}",
            "--scope",
            &format!("--unit={{}}", unit_name),
            "--description={description}",
            "--property=OOMPolicy=kill",
            "--",
            target,
        ])
        .args(std::env::args().skip(1))
        .exec();

    eprintln!("Failed to exec systemd-run: {{}}", err);
    std::process::exit(1);
}}
{remote_cli_fn}
"#,
            target = self.target,
            slice = self.slice,
            name = self.name,
            description = description,
            remote_cli_code = remote_cli_code,
            remote_cli_fn = remote_cli_fn,
        )
    }

    /// Build the wrapper binary
    pub fn build(&self, wrappers_dir: &Path) -> Result<()> {
        let wrapper_name = &self.name;
        let wrapper_dir = wrappers_dir.join(wrapper_name);

        // Create wrapper crate directory
        fs::create_dir_all(&wrapper_dir)?;

        // Write Cargo.toml
        let cargo_toml = format!(
            r#"[package]
name = "{name}-wrapper"
version = "0.1.0"
edition = "2021"

[profile.release]
opt-level = "z"
lto = true
codegen-units = 1
panic = "abort"
strip = true
"#,
            name = wrapper_name
        );
        fs::write(wrapper_dir.join("Cargo.toml"), cargo_toml)?;

        // Create src directory and write main.rs
        let src_dir = wrapper_dir.join("src");
        fs::create_dir_all(&src_dir)?;
        fs::write(src_dir.join("main.rs"), self.generate_source())?;

        // Build the wrapper
        Output::info(format!("Building wrapper: {}", wrapper_name));
        let status = Command::new("cargo")
            .args(["build", "--release"])
            .current_dir(&wrapper_dir)
            .status()
            .context("Failed to run cargo build")?;

        if !status.success() {
            anyhow::bail!("Failed to build wrapper: {}", wrapper_name);
        }

        // Copy the binary to the output location
        let binary_path = wrapper_dir
            .join("target")
            .join("release")
            .join(format!("{}-wrapper", wrapper_name));

        let output_path = wrappers_dir.join("bin").join(&self.name);
        fs::create_dir_all(output_path.parent().unwrap())?;
        fs::copy(&binary_path, &output_path)?;

        Output::success(format!(
            "Built wrapper: {} -> {}",
            wrapper_name,
            output_path.display()
        ));

        Ok(())
    }
}

/// Execute the wrap command
pub fn execute(args: WrapArgs) -> Result<()> {
    match args.action {
        WrapAction::Generate => execute_generate(),
        WrapAction::Build => execute_build(),
        WrapAction::Check => execute_check(),
        WrapAction::List => execute_list(),
    }
}

fn execute_generate() -> Result<()> {
    let repo_root = find_repo_path()?;
    let wrappers_dir = repo_root.join("wrappers");
    let configs = load_wrapper_configs(&repo_root)?;

    if configs.is_empty() {
        Output::info("No wrappers configured in image-config.json");
        return Ok(());
    }

    for config in &configs {
        let source = config.generate_source();
        let source_path = wrappers_dir.join(&config.name).join("src").join("main.rs");
        fs::create_dir_all(source_path.parent().unwrap())?;
        fs::write(&source_path, &source)
            .with_context(|| format!("Failed to write {}", source_path.display()))?;
        Output::info(format!("Generated wrappers/{}/src/main.rs", config.name));
    }

    Output::success(format!("Generated source for {} wrapper(s)", configs.len()));
    Ok(())
}

fn execute_check() -> Result<()> {
    let repo_root = find_repo_path()?;
    let wrappers_dir = repo_root.join("wrappers");
    let configs = load_wrapper_configs(&repo_root)?;

    if configs.is_empty() {
        Output::info("No wrappers configured in image-config.json");
        return Ok(());
    }

    let mut all_match = true;

    for config in &configs {
        let expected = config.generate_source();
        let source_path = wrappers_dir.join(&config.name).join("src").join("main.rs");

        let current = match fs::read_to_string(&source_path) {
            Ok(content) => content,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Output::error(format!("Missing: wrappers/{}/src/main.rs", config.name));
                all_match = false;
                continue;
            }
            Err(e) => {
                return Err(e).context(format!(
                    "Failed to read wrappers/{}/src/main.rs",
                    config.name
                ));
            }
        };

        if expected != current {
            Output::error(format!("Out of sync: wrappers/{}/src/main.rs", config.name));
            let exp_lines: Vec<&str> = expected.lines().collect();
            let cur_lines: Vec<&str> = current.lines().collect();
            let diff_count = exp_lines
                .iter()
                .zip(cur_lines.iter())
                .filter(|(a, b)| a != b)
                .count();
            let len_diff = (exp_lines.len() as i64 - cur_lines.len() as i64).abs();
            Output::info(format!(
                "  {} line(s) differ, {} line(s) length difference",
                diff_count, len_diff
            ));
            all_match = false;
        }
    }

    if all_match {
        Output::success("All wrapper source files are in sync with manifests.");
        Ok(())
    } else {
        Output::error("Wrapper source has drifted from manifests.");
        Output::info("Run `bkt wrap generate` to regenerate.");
        std::process::exit(1);
    }
}

fn execute_build() -> Result<()> {
    let repo_root = find_repo_path()?;
    let wrappers_dir = repo_root.join("wrappers");

    // Load wrapper configs from image-config.json
    let configs = load_wrapper_configs(&repo_root)?;

    if configs.is_empty() {
        Output::info("No wrappers configured in image-config.json");
        return Ok(());
    }

    for config in &configs {
        config.build(&wrappers_dir)?;
    }

    Output::success(format!("Built {} wrapper(s)", configs.len()));
    Ok(())
}

fn execute_list() -> Result<()> {
    let repo_root = find_repo_path()?;
    let configs = load_wrapper_configs(&repo_root)?;

    if configs.is_empty() {
        Output::info("No wrappers configured");
        return Ok(());
    }

    println!("{:<15} {:<35} {:<25}", "NAME", "TARGET", "SLICE");
    println!("{}", "-".repeat(75));
    for config in configs {
        println!(
            "{:<15} {:<35} {:<25}",
            config.name, config.target, config.slice
        );
    }

    Ok(())
}

/// Load wrapper configurations from image-config.json
fn load_wrapper_configs(repo_root: &Path) -> Result<Vec<WrapperConfig>> {
    use crate::manifest::image_config::ImageConfigManifest;

    let manifest = ImageConfigManifest::load_from_repo(repo_root)?;
    Ok(manifest.wrappers())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_source_basic() {
        let config = WrapperConfig {
            name: "test".to_string(),
            target: "/usr/bin/test".to_string(),
            slice: "app-test.slice".to_string(),
            output: "/usr/bin/test".to_string(),
            remote_cli: false,
            description: None,
        };

        let source = config.generate_source();
        assert!(source.contains("systemd-run"));
        assert!(source.contains("app-test.slice"));
        assert!(source.contains("/usr/bin/test"));
        assert!(source.contains("\"--quiet\""));
        assert!(!source.contains("VSCODE_IPC_HOOK_CLI"));
    }

    #[test]
    fn test_generate_source_with_remote_cli() {
        let config = WrapperConfig {
            name: "code".to_string(),
            target: "/usr/share/code/bin/code".to_string(),
            slice: "app-vscode.slice".to_string(),
            output: "/usr/bin/code".to_string(),
            remote_cli: true,
            description: Some("VS Code (managed)".to_string()),
        };

        let source = config.generate_source();
        assert!(source.contains("VSCODE_IPC_HOOK_CLI"));
        assert!(source.contains("find_remote_cli"));
        assert!(source.contains("/remote-cli/"));
    }
}
