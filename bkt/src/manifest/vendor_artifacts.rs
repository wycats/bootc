//! Vendor artifacts manifest types.
//!
//! Re-exports from `bkt-common` so the types are shared between `bkt` and `bkt-build`.
//! The manifest captures intent ("follow latest stable VS Code"); resolved
//! versions are a build artifact, not tracked in git.

pub use bkt_common::manifest::{
    ArtifactKind, VendorArtifact, VendorArtifactsManifest, VendorSource,
};
