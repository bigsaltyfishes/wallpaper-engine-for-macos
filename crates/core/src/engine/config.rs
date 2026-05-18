use crate::{DisplayIdentity, project::SceneTemplate};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct WallpaperEngineConfig {
    pub displays: Vec<DisplayConfig>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DisplayConfig {
    pub selector: DisplaySelector,
    pub window_active: bool,
    pub wallpaper: Option<WallpaperAssignment>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum DisplaySelector {
    Primary,
    Identity(DisplayIdentity),
    LiveDisplayId(u32),
}

#[derive(Clone, Debug, PartialEq)]
pub enum WallpaperAssignment {
    Direct(SceneTemplate),
    Mirror(DisplaySelector),
}
