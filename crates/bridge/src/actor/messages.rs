use std::collections::BTreeMap;

use crate::{
    actor::state::ApplyCandidates,
    api::{
        BridgeAppSnapshot, BridgeDisplayMode, BridgeDisplayMutationBundle,
        BridgeDisplaySettingsRow, BridgeError, BridgeLibrarySnapshot,
        BridgeMonitorInformationSnapshot, BridgePlaybackState, BridgePropertyValue,
        BridgeScalingMode, BridgeSettingsSnapshot, BridgeSnapshotBundle, BridgeWallpaperEntry,
        BridgeWallpaperKind, BridgeWallpaperMutationBundle, BridgeWallpaperOptionsSnapshot,
    },
    config::{AppConfig, WallpaperConfig},
    power::PowerSource,
};

pub struct Bootstrap;

pub struct GetAllSnapshots;

pub struct GetAppSnapshot;

pub struct GetLibrarySnapshot;

pub struct GetMonitorInformationSnapshot;

pub struct GetSettingsSnapshot;

pub struct ClearShaderCache;

pub struct GetWallpaperOptionsSnapshot {
    pub wallpaper_id: String,
}

pub struct InjectWallpaperForTest {
    pub id: String,
    pub title: String,
    pub kind: BridgeWallpaperKind,
}

pub struct InjectSceneWallpaperConfigForTest {
    pub id: String,
    pub title: String,
}

pub struct InjectSceneProjectForTest {
    pub id: String,
    pub title: String,
    pub project_json: String,
}

pub struct InjectDisplayForTest {
    pub display_id: String,
    pub title: String,
}

pub struct ReplaceLibraryForTest {
    pub entries: Vec<BridgeWallpaperEntry>,
}

pub struct ReplaceWallpaperConfigForTest {
    pub id: String,
    pub config: WallpaperConfig,
}

pub struct SelectWallpaper {
    pub id: String,
}

pub struct RefreshLibrary;

pub struct RefreshDisplays;

pub struct PollMousePosition;

pub struct SetFilter {
    pub kind: BridgeWallpaperKind,
    pub enabled: bool,
}

pub struct SetDisplayEnabled {
    pub display_id: String,
    pub enabled: bool,
}

pub struct SetDisplayMode {
    pub display_id: String,
    pub mode: BridgeDisplayMode,
}

pub struct SetDisplayHorizontalFlip {
    pub display_id: String,
    pub enabled: bool,
}

pub struct SetMirrorTarget {
    pub display_id: String,
    pub target_display_id: String,
}

pub struct SetMirrorScalingMode {
    pub display_id: String,
    pub mode: BridgeScalingMode,
}

pub struct SetMirrorScalingFactor {
    pub display_id: String,
    pub factor: f64,
}

pub struct SetMirrorTargetFps {
    pub display_id: String,
    pub fps: u32,
}

pub struct SetMirrorVolume {
    pub display_id: String,
    pub volume: f32,
}

pub struct SetMirrorMuted {
    pub display_id: String,
    pub muted: bool,
}

pub struct EjectWallpaperFromDisplay {
    pub display_id: String,
    pub wallpaper_id: String,
}

pub struct SetGlobalPlayback {
    pub playback_state: BridgePlaybackState,
    pub paused: bool,
}

pub struct Shutdown;

pub struct SetVolume {
    pub wallpaper_id: String,
    pub volume: f32,
}

pub struct SetMuted {
    pub wallpaper_id: String,
    pub muted: bool,
}

pub struct SetAudioResponseEnabled {
    pub wallpaper_id: String,
    pub enabled: bool,
}

pub struct SetDisplayConfigEnabled {
    pub wallpaper_id: String,
    pub display_id: String,
    pub enabled: bool,
}

pub struct SetScalingMode {
    pub wallpaper_id: String,
    pub display_id: String,
    pub mode: BridgeScalingMode,
}

pub struct SetScalingFactor {
    pub wallpaper_id: String,
    pub display_id: String,
    pub factor: f64,
}

pub struct SetTargetFps {
    pub wallpaper_id: String,
    pub display_id: String,
    pub fps: u32,
}

pub struct SetLaunchAtLogin {
    pub enabled: bool,
}

pub struct SetPauseOnBatteryPower {
    pub enabled: bool,
}

pub struct SetPowerSource {
    pub source: PowerSource,
    pub initial_sample: bool,
}

pub struct InitialFrameReady;

pub struct SetWorkshopDir {
    pub dir: String,
}

pub struct SetAssetsDir {
    pub dir: String,
}
pub struct EditProperty {
    pub wallpaper_id: String,
    pub property_id: String,
    pub value: BridgePropertyValue,
}

pub struct RestorePropertyDefault {
    pub wallpaper_id: String,
    pub property_id: String,
}

pub struct ApplyWallpaperOptions {
    pub wallpaper_id: String,
}

pub struct CancelWallpaperOptions {
    pub wallpaper_id: String,
}

pub struct CommitApplyAfterReconcile {
    pub wallpaper_id: String,
    pub candidates: ApplyCandidates,
    pub scenes: Vec<wallpaper_core::project::SceneDesc>,
    pub generation: u64,
}

pub struct CommitDisplayAfterReconcile {
    pub app_config: AppConfig,
    pub wallpaper_configs: BTreeMap<String, WallpaperConfig>,
    pub display_settings: BTreeMap<String, BridgeDisplaySettingsRow>,
    pub scenes: Vec<wallpaper_core::project::SceneDesc>,
    pub generation: u64,
}

pub struct CompleteRestoreAfterReconcile {
    pub result: Result<Vec<wallpaper_core::project::SceneDesc>, BridgeError>,
    pub generation: u64,
}

pub struct ReconcileFailed {
    pub error: BridgeError,
    pub generation: u64,
}

pub type AllSnapshotsReply = Result<BridgeSnapshotBundle, BridgeError>;
pub type BootstrapReply = AllSnapshotsReply;
pub type AppSnapshotReply = Result<BridgeAppSnapshot, BridgeError>;
pub type LibrarySnapshotReply = Result<BridgeLibrarySnapshot, BridgeError>;
pub type MonitorInformationSnapshotReply = Result<BridgeMonitorInformationSnapshot, BridgeError>;
pub type SettingsSnapshotReply = Result<BridgeSettingsSnapshot, BridgeError>;
pub type ClearShaderCacheReply = Result<BridgeSettingsSnapshot, BridgeError>;
pub type SetWorkshopDirReply = Result<BridgeSnapshotBundle, BridgeError>;
pub type SetAssetsDirReply = Result<BridgeSnapshotBundle, BridgeError>;
pub type WallpaperOptionsSnapshotReply = Result<BridgeWallpaperOptionsSnapshot, BridgeError>;
pub type TestMutationReply = Result<(), BridgeError>;
pub type SelectWallpaperReply = AllSnapshotsReply;
pub type RefreshLibraryReply = AllSnapshotsReply;
pub type RefreshDisplaysReply = AllSnapshotsReply;
pub type PollMousePositionReply = Result<(), BridgeError>;
pub type SetFilterReply = AllSnapshotsReply;
pub type DisplayMutationReply = Result<BridgeDisplayMutationBundle, crate::api::BridgeError>;
pub type SetDisplayEnabledReply = DisplayMutationReply;
pub type SetDisplayModeReply = DisplayMutationReply;
pub type SetDisplayHorizontalFlipReply = DisplayMutationReply;
pub type SetMirrorTargetReply = DisplayMutationReply;
pub type SetMirrorScalingModeReply = DisplayMutationReply;
pub type SetMirrorScalingFactorReply = DisplayMutationReply;
pub type SetMirrorTargetFpsReply = DisplayMutationReply;
pub type SetMirrorVolumeReply = DisplayMutationReply;
pub type SetMirrorMutedReply = DisplayMutationReply;
pub type EjectWallpaperFromDisplayReply = DisplayMutationReply;
pub type SetGlobalPlaybackReply = AllSnapshotsReply;
pub type SetPauseOnBatteryPowerReply = AllSnapshotsReply;
pub type SetPowerSourceReply = AllSnapshotsReply;
pub type InitialFrameReadyReply = AllSnapshotsReply;
pub type ShutdownReply = Result<(), BridgeError>;
pub type WallpaperMutationReply = Result<BridgeWallpaperMutationBundle, crate::api::BridgeError>;
pub type CommitApplyAfterReconcileReply = WallpaperMutationReply;
pub type CommitDisplayAfterReconcileReply = DisplayMutationReply;
pub type CompleteRestoreAfterReconcileReply = Result<(), BridgeError>;
pub type ReconcileFailedReply = Result<(), BridgeError>;
