//! Daemon client implementation.
//!
//! The client connects to the daemon socket and sends command execution requests.

use anyhow::{Context, Result};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

use super::protocol::{self, Request};

/// Client for communicating with the daemon.
pub struct DaemonClient {
    socket_path: PathBuf,
}

impl DaemonClient {
    /// Create a new client for the given socket path.
    pub fn new(socket_path: &Path) -> Self {
        Self {
            socket_path: socket_path.to_path_buf(),
        }
    }

    /// Connect to the daemon and execute a command.
    ///
    /// This passes the current process's stdin/stdout/stderr to the daemon,
    /// which will be used by the executed command.
    ///
    /// Returns the exit code of the executed command.
    pub fn execute(&self, argv: &[String], envp: &[String], cwd: &Path) -> Result<i32> {
        // Connect to the daemon
        let stream = UnixStream::connect(&self.socket_path).with_context(|| {
            format!(
                "Failed to connect to daemon at {}",
                self.socket_path.display()
            )
        })?;

        // Build the request
        let request = Request {
            argv: argv.to_vec(),
            envp: envp.to_vec(),
            cwd: cwd.to_path_buf(),
        };

        // Send request with our stdin/stdout/stderr
        protocol::send_request(
            &stream,
            &request,
            std::io::stdin().as_raw_fd(),
            std::io::stdout().as_raw_fd(),
            std::io::stderr().as_raw_fd(),
        )?;

        // Wait for response
        let response = protocol::recv_response(&stream)?;

        // Extract exit code
        Ok(response.exit_code().unwrap_or(1))
    }

    /// Execute a command using the current environment.
    ///
    /// This is a convenience method that captures the current environment
    /// and working directory.
    pub fn execute_current(&self, argv: &[String]) -> Result<i32> {
        let envp: Vec<String> = std::env::vars()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();
        let cwd = std::env::current_dir().context("Failed to get current directory")?;

        self.execute(argv, &envp, &cwd)
    }
}

/// Execute a command via the daemon, falling back to direct execution if unavailable.
///
/// This is the main entry point for daemon-accelerated execution.
pub fn execute_via_daemon(
    socket_path: &Path,
    argv: &[String],
    envp: &[String],
    cwd: &Path,
) -> Result<i32> {
    let client = DaemonClient::new(socket_path);
    client.execute(argv, envp, cwd)
}
