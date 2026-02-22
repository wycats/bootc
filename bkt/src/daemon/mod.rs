//! Host command daemon for fast cross-boundary execution.
//!
//! This module provides a Unix socket-based daemon that runs on the host
//! and accepts command execution requests from distrobox containers.
//! It bypasses the D-Bus/host-spawn overhead (~120ms) with direct socket
//! communication (~3ms).
//!
//! # Architecture
//!
//! ```text
//! Host                          Container
//! ┌─────────────┐              ┌─────────────┐
//! │ bkt-hostd   │◄─────────────│ bkt client  │
//! │ (daemon)    │  Unix socket │             │
//! └─────────────┘              └─────────────┘
//! ```
//!
//! The socket is placed in `$XDG_RUNTIME_DIR/bkt/host.sock`, which is
//! bind-mounted into distrobox containers, allowing direct access.
//!
//! # Usage
//!
//! ```bash
//! # Start daemon on host
//! bkt admin daemon run
//!
//! # From container, commands automatically use daemon if available
//! bkt status  # Uses daemon socket instead of distrobox-host-exec
//! ```
//!
//! See [RFC-0048](../../../docs/rfcs/0048-persistent-host-command-helper.md) for design details.

mod client;
mod protocol;
mod server;

pub use client::DaemonClient;
pub use protocol::{Request, Response};
pub use server::DaemonServer;

use anyhow::{Context, Result};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

/// Default connection timeout for daemon operations.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

/// Returns the path to the daemon socket.
///
/// The socket is placed in `$XDG_RUNTIME_DIR/bkt/host.sock`.
/// This directory is bind-mounted into distrobox containers.
pub fn socket_path() -> Result<PathBuf> {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR").context("XDG_RUNTIME_DIR not set")?;
    Ok(PathBuf::from(runtime_dir).join("bkt").join("host.sock"))
}

/// Check if the daemon socket exists and is connectable.
///
/// This performs an actual connection attempt to detect stale sockets
/// (e.g., from a crashed daemon that didn't clean up).
pub fn daemon_available() -> bool {
    let Ok(path) = socket_path() else {
        return false;
    };

    if !path.exists() {
        return false;
    }

    // Try to connect with a short timeout to detect stale sockets
    match UnixStream::connect(&path) {
        Ok(stream) => {
            // Set a read timeout to avoid blocking forever
            let _ = stream.set_read_timeout(Some(Duration::from_millis(100)));
            drop(stream);
            true
        }
        Err(_) => {
            // Socket exists but can't connect - it's stale
            tracing::debug!("Daemon socket exists but is not connectable (stale?)");
            false
        }
    }
}

/// Check if the daemon socket exists (without attempting connection).
///
/// This is faster than `daemon_available()` but won't detect stale sockets.
pub fn daemon_socket_exists() -> bool {
    socket_path().map(|p| p.exists()).unwrap_or(false)
}
