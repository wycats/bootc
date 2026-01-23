//! Manifest types for bkt.
//!
//! These structs represent the JSON manifest files used by bootc.

pub mod appimage;
pub mod base;
pub mod base_image;
pub mod build_info;
pub mod changelog;
pub mod diff;
pub mod distrobox;
pub mod dnf;
pub mod ephemeral;
pub mod extension;
pub mod flatpak;
pub mod gsetting;
pub mod homebrew;
pub mod parsers;
pub mod shim;
pub mod system_config;
pub mod toolbox;
pub mod upstream;

pub use appimage::*;
pub use base::*;
pub use changelog::*;
pub use distrobox::*;
pub use dnf::*;
pub use extension::*;
pub use flatpak::*;
pub use gsetting::*;
pub use homebrew::*;
pub use shim::*;
pub use toolbox::*;
pub use upstream::*;
