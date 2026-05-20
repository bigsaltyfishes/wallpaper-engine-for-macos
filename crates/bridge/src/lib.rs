#![deny(clippy::correctness)]
#![deny(clippy::single_call_fn)]
#![deny(clippy::complexity)]
#![warn(clippy::pedantic)]

mod actor;
mod api;
pub mod config;
pub mod display;
pub mod engine;
pub mod library;
pub mod logging;
pub mod login;
pub mod paths;
pub mod project;
pub mod state;

#[cfg(test)]
mod tests;

pub use api::{
    BridgeAppSnapshot, BridgeDisplayConfigRow, BridgeDisplayMode, BridgeDisplayMutationBundle,
    BridgeDisplaySettingsRow, BridgeError, BridgeErrorKind, BridgeLibraryScanStatus,
    BridgeLibrarySnapshot, BridgeLogLevel, BridgeLogStatus, BridgeMonitorInfoRow,
    BridgeMonitorInformationSnapshot, BridgePlaybackState, BridgePropertyDescriptor,
    BridgePropertyKind, BridgePropertyValue, BridgeScalingMode, BridgeSettingsSnapshot,
    BridgeSliderMetadata, BridgeSnapshotBundle, BridgeStorageStatus, BridgeWallpaperEntry,
    BridgeWallpaperKind, BridgeWallpaperMutationBundle, BridgeWallpaperOptionsSnapshot,
    WallpaperBridge,
};

mod build {
    include!(concat!(env!("OUT_DIR"), "/shadow.rs"));
}

uniffi::setup_scaffolding!();
