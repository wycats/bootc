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
use std::path::PathBuf;

/// Returns the path to the daemon socket.
///
/// The socket is placed in `$XDG_RUNTIME_DIR/bkt/host.sock`.
/// This directory is bind-mounted into distrobox containers.
pub fn socket_path() -> Result<PathBuf> {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR").context("XDG_RUNTIME_DIR not set")?;
    Ok(PathBuf::from(runtime_dir).join("bkt").join("host.sock"))
}

/// Check if the daemon socket exists and is connectable.
pub fn daemon_available() -> bool {
    if let Ok(path) = socket_path() {
        path.exists()
    } else {
        false
    }
}
