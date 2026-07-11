#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum BridgeWallpaperKind {
    ProjectScene,
    Video,
    Webpage,
    Unknown,
}

impl From<wallpaper_core::project::WallpaperProjectType> for BridgeWallpaperKind {
    fn from(value: wallpaper_core::project::WallpaperProjectType) -> Self {
        match value {
            wallpaper_core::project::WallpaperProjectType::Scene => Self::ProjectScene,
            wallpaper_core::project::WallpaperProjectType::Video => Self::Video,
            wallpaper_core::project::WallpaperProjectType::Web => Self::Webpage,
            wallpaper_core::project::WallpaperProjectType::Unknown => Self::Unknown,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum BridgeScalingMode {
    None,
    Stretch,
    Match,
    Fill,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum BridgeDisplayMode {
    Standalone,
    Mirror,
}

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum BridgePropertyValue {
    Bool { value: bool },
    Number { value: f64 },
    String { value: String },
    ColorRgb { red: f64, green: f64, blue: f64 },
    Empty,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum BridgePropertyKind {
    Slider,
    Combo,
    Bool,
    Color,
    TextInput,
    Text,
    Group,
    Directory,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum BridgePlaybackState {
    Playing,
    Paused,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct BridgeAppSnapshot {
    pub playback_state: BridgePlaybackState,
    pub selected_wallpaper_id: Option<String>,
    pub active_wallpaper_ids: Vec<String>,
    pub errors: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct BridgeLibraryScanStatus {
    pub scanning: bool,
    pub done: u64,
    pub total: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct BridgeWallpaperEntry {
    pub id: String,
    pub title: String,
    pub kind: BridgeWallpaperKind,
    pub supported: bool,
    pub active: bool,
    pub selected: bool,
    pub preview_path: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct BridgeLibrarySnapshot {
    pub wallpapers: Vec<BridgeWallpaperEntry>,
    pub scan_status: BridgeLibraryScanStatus,
    pub scene_count: u64,
    pub video_count: u64,
    pub webpage_count: u64,
    pub unknown_count: u64,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
#[allow(clippy::struct_excessive_bools)]
pub struct BridgeDisplayConfigRow {
    pub display_id: String,
    pub title: String,
    pub enabled: bool,
    pub scaling_mode: BridgeScalingMode,
    pub scaling_factor: f64,
    pub target_fps: u32,
    pub max_fps: u32,
    pub muted: bool,
    pub volume: f32,
    pub dirty: bool,
    pub can_restore_defaults: bool,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct BridgeSliderMetadata {
    pub min: f64,
    pub max: f64,
    pub step: f64,
    pub precision: u32,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct BridgePropertyDescriptor {
    pub id: String,
    pub kind: BridgePropertyKind,
    pub label_html: String,
    pub value: BridgePropertyValue,
    pub default_value: BridgePropertyValue,
    pub slider: Option<BridgeSliderMetadata>,
    pub dirty: bool,
    pub can_restore_defaults: bool,
    pub enabled: bool,
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct BridgeWallpaperOptionsSnapshot {
    pub wallpaper_id: String,
    pub title: String,
    pub kind: BridgeWallpaperKind,
    pub supported: bool,
    pub dirty: bool,
    pub properties: Vec<BridgePropertyDescriptor>,
    pub display_configurations: Vec<BridgeDisplayConfigRow>,
    pub audio_response_enabled: bool,
    pub muted: bool,
    pub volume: f32,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct BridgeMonitorInfoRow {
    pub display_id: String,
    pub title: String,
    pub wallpaper_id: String,
    pub wallpaper_title: String,
    pub mirror_target_display_id: Option<String>,
    pub mirror_target_title: Option<String>,
    pub scaling_mode: String,
    pub target_fps: String,
    pub audio_response: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct BridgeMonitorInformationSnapshot {
    pub rows: Vec<BridgeMonitorInfoRow>,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct BridgeDisplaySettingsRow {
    pub display_id: String,
    pub title: String,
    pub enabled: bool,
    pub mode: BridgeDisplayMode,
    pub mirror_targets: Vec<String>,
    pub selected_mirror_target: Option<String>,
    pub scaling_mode: BridgeScalingMode,
    pub scaling_factor: f64,
    pub target_fps: u32,
    pub max_fps: u32,
    pub muted: bool,
    pub volume: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum BridgeLogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct BridgeLogStatus {
    pub logs_root: String,
    pub active_session: String,
    pub active_file: String,
    pub active_file_size_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct BridgeStorageStatus {
    pub shader_cache_size_bytes: u64,
    pub logs: BridgeLogStatus,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct BridgeSettingsSnapshot {
    pub displays: Vec<BridgeDisplaySettingsRow>,
    pub launch_at_login_available: bool,
    pub launch_at_login_enabled: bool,
    pub pause_on_battery_power: bool,
    pub git_sha: String,
    pub bridge_version: String,
    pub core_version: String,
    pub shader_pipeline_version: String,
    pub storage: BridgeStorageStatus,
    pub workshop_dir: String,
    pub assets_dir: String,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct BridgeSnapshotBundle {
    pub app: BridgeAppSnapshot,
    pub library: BridgeLibrarySnapshot,
    pub wallpaper_options: Option<BridgeWallpaperOptionsSnapshot>,
    pub monitor_information: BridgeMonitorInformationSnapshot,
    pub settings: BridgeSettingsSnapshot,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct BridgeWallpaperMutationBundle {
    pub app: BridgeAppSnapshot,
    pub library: BridgeLibrarySnapshot,
    pub wallpaper_options: BridgeWallpaperOptionsSnapshot,
    pub monitor_information: BridgeMonitorInformationSnapshot,
    pub settings: BridgeSettingsSnapshot,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct BridgeDisplayMutationBundle {
    pub app: BridgeAppSnapshot,
    pub library: BridgeLibrarySnapshot,
    pub monitor_information: BridgeMonitorInformationSnapshot,
    pub settings: BridgeSettingsSnapshot,
}
