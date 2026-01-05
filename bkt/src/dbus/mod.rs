//! D-Bus integration for privileged operations.
//!
//! This module provides D-Bus access to system services, primarily systemd.
//! Uses zbus for type-safe D-Bus communication.
//!
//! ## Architecture
//!
//! - **From host**: Direct connection to system bus
//! - **From toolbox**: System bus routes to host automatically via flatpak-portal
//!
//! Polkit handles authorization - wheel group members get passwordless access.

pub mod systemd;

pub use systemd::SystemdManager;
