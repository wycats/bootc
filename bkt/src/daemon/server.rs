//! Daemon server implementation.
//!
//! The server listens on a Unix socket and handles command execution requests.
//! Each request forks a child process to execute the command, passing through
//! the client's stdin/stdout/stderr via fd passing.

use anyhow::{Context, Result, bail};
use nix::sys::wait::{WaitStatus, waitpid};
use nix::unistd::{self, ForkResult};
use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use super::protocol::{self, Request, Response};

/// The daemon server.
pub struct DaemonServer {
    socket_path: PathBuf,
    listener: UnixListener,
    shutdown: Arc<AtomicBool>,
}

impl DaemonServer {
    /// Bind to the socket path and create a new server.
    pub fn bind(socket_path: &Path) -> Result<Self> {
        // Create parent directory if needed
        if let Some(parent) = socket_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
        }

        // Remove stale socket if it exists
        if socket_path.exists() {
            std::fs::remove_file(socket_path).with_context(|| {
                format!("Failed to remove stale socket: {}", socket_path.display())
            })?;
        }

        // Bind the socket
        let listener = UnixListener::bind(socket_path)
            .with_context(|| format!("Failed to bind socket: {}", socket_path.display()))?;

        // Set socket permissions (user-only)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(socket_path, std::fs::Permissions::from_mode(0o600))
                .context("Failed to set socket permissions")?;
        }

        eprintln!("Daemon listening on: {}", socket_path.display());

        Ok(Self {
            socket_path: socket_path.to_path_buf(),
            listener,
            shutdown: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Run the server's main loop.
    pub fn run(&self) -> Result<()> {
        // Set up signal handlers for graceful shutdown
        let shutdown = self.shutdown.clone();
        ctrlc::set_handler(move || {
            eprintln!("\nReceived shutdown signal");
            shutdown.store(true, Ordering::SeqCst);
        })
        .context("Failed to set signal handler")?;

        // Set non-blocking so we can check shutdown flag
        self.listener.set_nonblocking(true)?;

        while !self.shutdown.load(Ordering::SeqCst) {
            match self.listener.accept() {
                Ok((stream, _addr)) => {
                    // Set blocking for the connection
                    stream.set_nonblocking(false)?;

                    if let Err(e) = self.handle_connection(stream) {
                        eprintln!("Connection error: {}", e);
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // No connection ready, sleep briefly and check shutdown
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
                Err(e) => {
                    eprintln!("Accept error: {}", e);
                }
            }
        }

        // Clean up socket on shutdown
        eprintln!("Shutting down...");
        let _ = std::fs::remove_file(&self.socket_path);

        Ok(())
    }

    /// Handle a single client connection.
    fn handle_connection(&self, stream: UnixStream) -> Result<()> {
        // Receive the request with file descriptors
        let (request, fds) = protocol::recv_request(&stream)?;

        eprintln!(
            "Executing: {} (cwd: {})",
            request.argv.join(" "),
            request.cwd.display()
        );

        // Fork and exec
        let wait_status = self.fork_exec(&request, fds)?;

        // Send response
        let response = Response { wait_status };
        protocol::send_response(&stream, &response)?;

        Ok(())
    }

    /// Fork a child process and execute the command.
    fn fork_exec(&self, request: &Request, fds: [OwnedFd; 3]) -> Result<i32> {
        use std::ffi::CString;

        if request.argv.is_empty() {
            bail!("Empty argv");
        }

        // Resolve program path (do PATH lookup before fork)
        let program_name = &request.argv[0];
        let program_path = if program_name.contains('/') {
            // Absolute or relative path - use as-is
            program_name.clone()
        } else {
            // Search PATH
            let path_env = request
                .envp
                .iter()
                .find(|s| s.starts_with("PATH="))
                .map(|s| &s[5..])
                .unwrap_or("/usr/bin:/bin");

            path_env
                .split(':')
                .map(|dir| format!("{}/{}", dir, program_name))
                .find(|p| std::path::Path::new(p).exists())
                .unwrap_or_else(|| program_name.clone())
        };

        // Prepare CStrings BEFORE fork to minimize child work
        let program = CString::new(program_path.as_str()).context("Invalid program name")?;
        let argv: Vec<CString> = request
            .argv
            .iter()
            .map(|s| CString::new(s.as_str()).unwrap())
            .collect();
        let envp: Vec<CString> = request
            .envp
            .iter()
            .map(|s| CString::new(s.as_str()).unwrap())
            .collect();
        let cwd = CString::new(request.cwd.to_string_lossy().as_ref()).context("Invalid cwd")?;

        // SAFETY: We're about to fork. The child will exec immediately.
        match unsafe { unistd::fork() }? {
            ForkResult::Parent { child } => {
                // Parent: wait for child and return status
                // Close the fds in parent (child has them now via fork)
                drop(fds);

                match waitpid(child, None)? {
                    WaitStatus::Exited(_, code) => Ok(code << 8), // Encode as waitpid status
                    WaitStatus::Signaled(_, sig, _) => Ok(sig as i32),
                    other => {
                        eprintln!("Unexpected wait status: {:?}", other);
                        Ok(1 << 8) // Generic failure
                    }
                }
            }
            ForkResult::Child => {
                // Child: set up fds and exec
                // SAFETY: We're in the child after fork, about to exec
                // Minimize work here - all prep was done before fork
                unsafe {
                    // Redirect stdin/stdout/stderr to the passed fds
                    if libc::dup2(fds[0].as_raw_fd(), 0) < 0 {
                        libc::_exit(126);
                    }
                    if libc::dup2(fds[1].as_raw_fd(), 1) < 0 {
                        libc::_exit(126);
                    }
                    if libc::dup2(fds[2].as_raw_fd(), 2) < 0 {
                        libc::_exit(126);
                    }

                    // Close the original fds (now duplicated)
                    drop(fds);

                    // Change to requested working directory
                    if libc::chdir(cwd.as_ptr()) < 0 {
                        libc::_exit(126);
                    }

                    // Build argv and envp pointers for execve
                    let argv_ptrs: Vec<*const libc::c_char> = argv
                        .iter()
                        .map(|s| s.as_ptr())
                        .chain(std::iter::once(std::ptr::null()))
                        .collect();
                    let envp_ptrs: Vec<*const libc::c_char> = envp
                        .iter()
                        .map(|s| s.as_ptr())
                        .chain(std::iter::once(std::ptr::null()))
                        .collect();

                    // Execute!
                    libc::execve(program.as_ptr(), argv_ptrs.as_ptr(), envp_ptrs.as_ptr());

                    // If we get here, exec failed
                    libc::_exit(127);
                }
            }
        }
    }
}

impl Drop for DaemonServer {
    fn drop(&mut self) {
        // Clean up socket on drop
        let _ = std::fs::remove_file(&self.socket_path);
    }
}
