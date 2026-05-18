#![deny(clippy::correctness)]
#![deny(clippy::single_call_fn)]
#![deny(clippy::complexity)]
#![warn(clippy::pedantic)]

mod actor;
mod api;
pub mod config;
pub mod display;
pub mod engine;
pub mod login;
pub mod library;
pub mod paths;
pub mod project;
pub mod state;

#[cfg(test)]
mod tests;

pub use api::{
    BridgeAppSnapshot, BridgeDisplayConfigRow, BridgeDisplayMode, BridgeDisplayMutationBundle,
    BridgeDisplaySettingsRow, BridgeError, BridgeErrorKind, BridgeLibraryScanStatus,
    BridgeLibrarySnapshot, BridgeMonitorInfoRow, BridgeMonitorInformationSnapshot,
    BridgePlaybackState, BridgePropertyDescriptor, BridgePropertyKind, BridgePropertyValue,
    BridgeScalingMode, BridgeSettingsSnapshot, BridgeSliderMetadata, BridgeSnapshotBundle,
    BridgeWallpaperEntry, BridgeWallpaperKind, BridgeWallpaperMutationBundle,
    BridgeWallpaperOptionsSnapshot, WallpaperBridge,
};

uniffi::setup_scaffolding!();
