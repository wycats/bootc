//! Daemon subcommand implementation for `bkt admin daemon`.
//!
//! Manages the host command daemon for fast cross-boundary execution.

use anyhow::Result;
use clap::Subcommand;

use crate::daemon::{self, DaemonServer};
use crate::pipeline::ExecutionPlan;

/// Daemon operations available via `bkt admin daemon`.
#[derive(Debug, Subcommand)]
pub enum DaemonAction {
    /// Run the daemon in foreground mode
    ///
    /// Starts the host command daemon, listening for requests from
    /// distrobox containers. Press Ctrl+C to stop.
    ///
    /// The daemon listens on $XDG_RUNTIME_DIR/bkt/host.sock.
    Run,

    /// Show daemon status
    ///
    /// Checks if the daemon is running and shows socket information.
    Status,

    /// Test the daemon by executing a command
    ///
    /// Connects to the running daemon and executes the given command.
    /// This is useful for testing the daemon protocol.
    Test {
        /// Command and arguments to execute
        #[arg(trailing_var_arg = true, required = true)]
        command: Vec<String>,
    },
}

/// Execute a daemon subcommand.
pub fn run(action: DaemonAction, _plan: &ExecutionPlan) -> Result<()> {
    match action {
        DaemonAction::Run => run_foreground(),
        DaemonAction::Status => show_status(),
        DaemonAction::Test { command } => test_execute(command),
    }
}

/// Run the daemon in foreground mode.
fn run_foreground() -> Result<()> {
    let socket_path = daemon::socket_path()?;

    eprintln!("Starting bkt host daemon...");
    eprintln!("Socket: {}", socket_path.display());
    eprintln!("Press Ctrl+C to stop.\n");

    let server = DaemonServer::bind(&socket_path)?;
    server.run()?;

    Ok(())
}

/// Test executing a command via the daemon.
fn test_execute(command: Vec<String>) -> Result<()> {
    use crate::daemon::DaemonClient;

    let socket_path = daemon::socket_path()?;
    let client = DaemonClient::new(&socket_path);
    let exit_code = client.execute_current(&command)?;

    std::process::exit(exit_code);
}

/// Show daemon status.
fn show_status() -> Result<()> {
    let socket_path = daemon::socket_path()?;

    if socket_path.exists() {
        // Try to connect to verify it's alive
        match std::os::unix::net::UnixStream::connect(&socket_path) {
            Ok(_) => {
                println!("Daemon Status: running");
                println!("  Socket: {}", socket_path.display());
            }
            Err(_) => {
                println!("Daemon Status: stale socket (not responding)");
                println!("  Socket: {}", socket_path.display());
                println!("\nThe socket file exists but the daemon is not responding.");
                println!("You may need to remove the stale socket and restart:");
                println!("  rm {}", socket_path.display());
                println!("  bkt admin daemon run");
            }
        }
    } else {
        println!("Daemon Status: not running");
        println!("  Socket: {} (not found)", socket_path.display());
        println!("\nTo start the daemon:");
        println!("  bkt admin daemon run");
    }

    Ok(())
}
