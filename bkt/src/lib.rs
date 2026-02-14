//! bkt - Bucket: manage your bootc manifests
//!
//! A library for managing system manifests including Flatpaks, GNOME extensions,
//! GSettings, host shims, skel files, and system profiles.
//!
//! # Command Punning
//!
//! `bkt` implements "command punning": commands that execute immediately AND
//! propagate changes to the distribution via Git PRs. This is the core philosophy
//! of Phase 2.
//!
//! ## Execution Contexts
//!
//! - **Host** (default): Execute on the immutable host system
//! - **Dev** (`bkt dev ...`): Execute in the development toolbox
//! - **Image** (`--pr-only`): Only update manifests, no local execution
//!
//! ## PR Modes
//!
//! - Default: Execute locally AND create PR
//! - `--local`: Execute locally only, skip PR
//! - `--pr-only`: Create PR only, skip local execution

pub mod cli;
pub mod command_runner;
pub mod commands;
pub mod containerfile;
pub mod context;
pub mod dbus;
pub mod effects;
pub mod manifest;
pub mod output;
pub mod pipeline;
pub mod plan;
pub mod pr;
pub mod repo;
pub mod subsystem;
pub mod validation;

pub use cli::{Cli, Commands};
pub use context::{CommandDomain, ExecutionContext, PrMode};
