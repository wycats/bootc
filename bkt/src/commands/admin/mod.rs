//! Admin command implementation - privileged operations via polkit.
//!
//! This module provides passwordless privileged operations for wheel group
//! members, enabling seamless management of bootc images and systemd services.
//!
//! # Security Model
//!
//! - **Read-only operations**: Passwordless for wheel group (polkit)
//! - **Mutating operations**: Require explicit `--confirm` flag
//! - **Wheel requirement**: Users must be in wheel group (standard for Fedora/RHEL)
//!
//! # Context Handling
//!
//! Admin commands run on the host. When executed from a distrobox container,
//! D-Bus routes correctly to the host system automatically.
//!
//! # Example Usage
//!
//! ```bash
//! # Bootc operations (via pkexec)
//! bkt admin bootc status
//! bkt admin bootc upgrade --confirm
//!
//! # Systemctl operations (via D-Bus)
//! bkt admin systemctl status docker
//! bkt admin systemctl restart docker --confirm
//! bkt admin systemctl enable docker --confirm
//! ```
//!
//! See [RFC-0009](../../../docs/rfcs/0009-privileged-operations.md) for design details.

mod bootc;
mod kargs;
mod systemctl;
mod systemd;

use anyhow::Result;
use clap::{Args, Subcommand};

use crate::pipeline::ExecutionPlan;

pub use bootc::BootcAction;
pub use kargs::KargsAction;
pub use systemctl::SystemctlAction;
pub use systemd::SystemdAction;

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
    /// Note: `bkt` runs on the host via shim; distrobox D-Bus routing handles
    /// communication with host services automatically.
    Bootc {
        #[command(subcommand)]
        action: BootcAction,
    },

    /// Manage systemd services via D-Bus
    ///
    /// Provides passwordless access to systemd unit management for wheel group members.
    /// Read-only operations (status) execute immediately; mutations require --confirm.
    ///
    /// Uses D-Bus (zbus) rather than shelling out to systemctl, providing proper
    /// structured output and avoiding potential privilege escalation vectors.
    Systemctl {
        #[command(subcommand)]
        action: SystemctlAction,
    },

    /// Manage persistent kernel arguments (image-time configuration)
    Kargs {
        #[command(subcommand)]
        action: KargsAction,
    },

    /// Manage persistent systemd configuration (image-time configuration)
    Systemd {
        #[command(subcommand)]
        action: SystemdAction,
    },
}

/// Execute an admin subcommand.
pub fn run(args: AdminArgs, plan: &ExecutionPlan) -> Result<()> {
    match args.action {
        AdminAction::Bootc { action } => bootc::run(action, plan),
        AdminAction::Systemctl { action } => systemctl::run(action, plan),
        AdminAction::Kargs { action } => action.execute(plan),
        AdminAction::Systemd { action } => action.execute(plan),
    }
}
