//! Admin command implementation - privileged operations via polkit.
//!
//! This module provides passwordless privileged operations for wheel group
//! members, enabling seamless management of bootc images and systemd services
//! from both host and toolbox contexts.
//!
//! # Security Model
//!
//! - **Read-only operations**: Passwordless for wheel group (polkit)
//! - **Mutating operations**: Require explicit `--confirm` flag
//! - **Wheel requirement**: Users must be in wheel group (standard for Fedora/RHEL)
//!
//! # Context Handling
//!
//! Commands work identically from host or toolbox:
//! - **Host**: Direct `pkexec bootc <args>`
//! - **Toolbox**: `flatpak-spawn --host pkexec bootc <args>`
//!
//! Context is detected automatically via `RuntimeEnvironment::Toolbox`.
//!
//! # Example Usage
//!
//! ```bash
//! # Read-only (passwordless)
//! bkt admin bootc status
//!
//! # Mutating (requires --confirm)
//! bkt admin bootc upgrade --confirm
//! bkt admin bootc switch ghcr.io/user/image:latest --confirm
//! bkt admin bootc rollback --confirm
//! ```
//!
//! See [RFC-0009](../../../docs/rfcs/0009-privileged-operations.md) for design details.

mod bootc;

use anyhow::Result;
use clap::{Args, Subcommand};

use crate::pipeline::ExecutionPlan;

pub use bootc::BootcAction;

/// Arguments for the `admin` command.
#[derive(Debug, Args)]
pub struct AdminArgs {
    #[command(subcommand)]
    pub action: AdminAction,
}

/// Subcommands for privileged administration.
#[derive(Debug, Subcommand)]
pub enum AdminAction {
    /// Manage bootc images (upgrade, switch, rollback, status)
    ///
    /// Provides passwordless access to bootc commands for wheel group members.
    /// Read-only operations (status) execute immediately; mutations require --confirm.
    ///
    /// From toolbox, commands automatically delegate to host via flatpak-spawn.
    Bootc {
        #[command(subcommand)]
        action: BootcAction,
    },

    // Future: Systemctl subcommand will be added in Phase 3-4
    // Systemctl {
    //     #[command(subcommand)]
    //     action: SystemctlAction,
    // },
}

/// Execute an admin subcommand.
pub fn run(args: AdminArgs, plan: &ExecutionPlan) -> Result<()> {
    match args.action {
        AdminAction::Bootc { action } => bootc::run(action, plan),
    }
}
