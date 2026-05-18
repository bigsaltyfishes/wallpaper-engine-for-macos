#![allow(dead_code)]

use crate::{
    DisplayDesc, DisplaySelector, WallpaperAssignment,
    media::audio::AudioVolume,
    project::{ScalingMode, SceneDesc, SceneHandle, SceneResult},
};

pub struct Ping;

#[cfg(test)]
pub struct DisplayRecordCountForTest;

#[cfg(test)]
pub struct SequenceForTest {
    pub expected: u64,
}

pub struct RefreshDisplays;

pub struct RefreshDisplayDescriptors {
    pub primary: DisplayDesc,
    pub displays: Vec<DisplayDesc>,
}

pub struct ReconcileScenes {
    pub scenes: Vec<SceneDesc>,
}

pub struct CreateWindowForDisplay {
    pub selector: DisplaySelector,
}

pub struct DestroyWindowForDisplay {
    pub selector: DisplaySelector,
}

pub struct SetWallpaperForDisplay {
    pub selector: DisplaySelector,
    pub wallpaper: WallpaperAssignment,
}

pub struct SetScalingMode {
    pub handle: SceneHandle,
    pub mode: ScalingMode,
}

pub struct SetScalingFactor {
    pub handle: SceneHandle,
    pub factor: f64,
}

pub struct SetFps {
    pub handle: SceneHandle,
    pub fps: u32,
}

pub struct SetPaused {
    pub handle: SceneHandle,
    pub paused: bool,
}

pub struct SetAllPaused {
    pub paused: bool,
}

pub struct SetRenderResolution {
    pub handle: SceneHandle,
    pub width: u32,
    pub height: u32,
}

pub struct SetAudioResponseEnabled {
    pub handle: SceneHandle,
    pub enabled: bool,
}

pub struct SetAudioVolume {
    pub handle: SceneHandle,
    pub volume: AudioVolume,
}

pub struct SetAudioMuted {
    pub handle: SceneHandle,
    pub muted: bool,
}

pub struct SetPropertyOverride {
    pub handle: SceneHandle,
    pub flat_json: String,
}

pub struct ResetPropertyOverride {
    pub handle: SceneHandle,
}

pub struct CloseAllScenes;

pub type ReconcileReply = Result<Vec<SceneResult>, crate::EngineError>;
pub type SceneHandleReply = Result<Option<SceneHandle>, crate::EngineError>;
