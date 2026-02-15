use anyhow::{Context, Result};
use clap::Parser;
use tracing_subscriber::EnvFilter;

use bkt::commands;
use bkt::context;
use bkt::output;
use bkt::pipeline;
use bkt::{Cli, Commands};

/// Delegate to the appropriate context if needed.
///
/// This is called early in main(), after parsing but before command dispatch.
/// If we're in the wrong environment for the command's target, we re-exec
/// via distrobox-host-exec (toolbox→host) or distrobox enter (host→toolbox).
fn maybe_delegate(cli: &Cli) -> Result<()> {
    // Skip if explicitly disabled
    if cli.no_delegate {
        return Ok(());
    }

    // Skip if already delegated (prevent infinite recursion)
    if std::env::var("BKT_DELEGATED").is_ok() {
        return Ok(());
    }

    let runtime = context::detect_environment();
    let target = cli.command.target();

    match (runtime, target) {
        // In toolbox, command wants host → delegate to host
        (context::RuntimeEnvironment::Toolbox, context::CommandTarget::Host) => {
            if cli.dry_run {
                output::Output::dry_run("Would delegate to host: distrobox-host-exec bkt ...");
                return Ok(());
            }
            delegate_to_host()?;
        }

        // On host, command wants dev → delegate to toolbox
        (context::RuntimeEnvironment::Host, context::CommandTarget::Dev) => {
            if cli.dry_run {
                output::Output::dry_run(
                    "Would delegate to toolbox: distrobox enter bootc-dev -- bkt ...",
                );
                return Ok(());
            }
            delegate_to_toolbox()?;
        }

        // Generic container, command wants host → error (no delegation path)
        (context::RuntimeEnvironment::Container, context::CommandTarget::Host) => {
            anyhow::bail!(
                "Cannot run host commands from a generic container\n\n\
                 This command requires the host system, but you're in a container\n\
                 without distrobox-host-exec access.\n\n\
                 Options:\n  \
                 • Exit this container and run on the host\n  \
                 • Use a distrobox instead: distrobox create && distrobox enter"
            );
        }

        // All other cases: run locally
        _ => {}
    }

    Ok(())
}

/// Delegate the current command to the host via distrobox-host-exec.
fn delegate_to_host() -> Result<()> {
    output::Output::info("Delegating to host...");

    let args: Vec<String> = std::env::args().collect();
    let status = std::process::Command::new("distrobox-host-exec")
        .arg("bkt")
        .args(&args[1..]) // Skip argv[0] (the current binary path)
        .env("BKT_DELEGATED", "1") // Prevent recursion
        .status()
        .context("Failed to execute distrobox-host-exec")?;

    // Exit with the same code as the delegated command
    std::process::exit(status.code().unwrap_or(1));
}

/// Delegate the current command to the default toolbox.
fn delegate_to_toolbox() -> Result<()> {
    output::Output::info("Delegating to toolbox...");

    let args: Vec<String> = std::env::args().collect();
    let status = std::process::Command::new("distrobox")
        .arg("enter")
        .arg("bootc-dev")
        .arg("--")
        .arg("bkt")
        .args(&args[1..])
        .env("BKT_DELEGATED", "1")
        .status()
        .context("Failed to execute distrobox enter")?;

    std::process::exit(status.code().unwrap_or(1));
}

fn main() -> Result<()> {
    // Initialize tracing with RUST_LOG env filter
    // e.g., RUST_LOG=bkt=debug
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();

    // Check if we need to delegate to a different context (RFC-0010)
    maybe_delegate(&cli)?;

    // Create execution plan from global options
    let plan = pipeline::ExecutionPlan::from_cli(&cli);

    // Log detected context
    tracing::debug!(
        context = %plan.context,
        pr_mode = ?plan.pr_mode,
        dry_run = plan.dry_run,
        "Execution plan created"
    );

    match cli.command {
        Commands::Admin(args) => commands::admin::run(args, &plan),
        Commands::Apply(args) => commands::apply::run(args, &plan),
        Commands::Capture(args) => commands::capture::run(args, &plan),
        Commands::System(args) => commands::system::run(args, &plan),
        Commands::Dev(args) => commands::dev::run(args, &plan),
        Commands::Flatpak(args) => commands::flatpak::run(args, &plan),
        Commands::Distrobox(args) => commands::distrobox::run(args, &plan),
        Commands::AppImage(args) => commands::appimage::run(args, &plan),
        Commands::Fetchbin(args) => commands::fetchbin::run(args, &plan),
        Commands::Shim(args) => commands::shim::run(args, &plan),
        Commands::Extension(args) => commands::extension::run(args, &plan),
        Commands::Gsetting(args) => commands::gsetting::run(args, &plan),
        Commands::Homebrew(args) => commands::homebrew::run(args, &plan),
        Commands::Skel(args) => commands::skel::run(args, &plan),
        Commands::Profile(args) => commands::profile::run(args, plan.runner()),
        Commands::Repo(args) => commands::repo::run(args),
        Commands::Schema(args) => commands::schema::run(args),
        Commands::Completions(args) => commands::completions::run(args),
        Commands::Doctor(args) => commands::doctor::run(args),
        Commands::Status(args) => commands::status::run(args),
        Commands::Upstream(args) => commands::upstream::run(args, plan.runner()),
        Commands::Changelog(args) => commands::changelog::run(args),
        Commands::Drift(args) => commands::drift::run(args),
        Commands::Base(args) => commands::base::run(args, plan.runner()),
        Commands::BuildInfo(args) => commands::build_info::run(args, plan.runner()),
        Commands::Containerfile(args) => commands::containerfile::run(args, &plan),
        Commands::Local(args) => commands::local::run(args, &plan),
        Commands::Wrap(args) => commands::wrap::execute(args),
    }
}
