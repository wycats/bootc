//! Manifest types for bkt.
//!
//! These structs represent the JSON manifest files used by bootc.

pub mod changelog;
pub mod dnf;
pub mod extension;
pub mod flatpak;
pub mod gsetting;
pub mod shim;
pub mod toolbox;
pub mod upstream;

pub use changelog::*;
pub use dnf::*;
pub use extension::*;
pub use flatpak::*;
pub use gsetting::*;
pub use shim::*;
pub use toolbox::*;
pub use upstream::*;
