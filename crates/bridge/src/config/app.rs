use serde::{Deserialize, Serialize};
use wallpaper_core::{DisplayIdentity, DisplaySelector, project::ScalingMode};

pub const SCHEMA_VERSION: u32 = 1;
const DEFAULT_MONITOR_VOLUME: f32 = 1.0;
const DEFAULT_MONITOR_FPS: u32 = 60;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub general: GeneralCfg,
    #[serde(default)]
    pub power: PowerCfg,
    #[serde(default)]
    pub ui: UiCfg,
    #[serde(default)]
    pub monitors: Vec<MonitorCfg>,
    #[serde(default)]
    pub monitor_settings: Vec<MonitorSettingsCfg>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            general: GeneralCfg::default(),
            power: PowerCfg::default(),
            ui: UiCfg::default(),
            monitors: Vec::new(),
            monitor_settings: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeneralCfg {
    pub last_selected_wallpaper: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workshop_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assets_dir: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PowerCfg {
    #[serde(default)]
    pub pause_on_battery_power: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiCfg {
    #[serde(default = "default_selector_window")]
    pub selector_window: WindowGeom,
    #[serde(default = "default_settings_window")]
    pub settings_window: WindowGeom,
    #[serde(default)]
    pub filter: FilterCfg,
}

impl Default for UiCfg {
    fn default() -> Self {
        Self {
            selector_window: default_selector_window(),
            settings_window: default_settings_window(),
            filter: FilterCfg::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowGeom {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilterCfg {
    pub scene: bool,
    pub video: bool,
    pub web: bool,
    pub unknown: bool,
}

impl Default for FilterCfg {
    fn default() -> Self {
        Self {
            scene: true,
            video: true,
            web: true,
            unknown: true,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum SerializedSelector {
    #[default]
    Primary,
    Identity {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        uuid: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        vendor_id: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model_id: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        serial_number: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        unit_number: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
    #[serde(rename = "live_display_id")]
    LiveDisplayId { display_id: u32 },
}

impl SerializedSelector {
    #[must_use]
    pub fn to_selector(&self) -> DisplaySelector {
        match self {
            Self::Primary => DisplaySelector::Primary,
            Self::Identity {
                uuid,
                vendor_id,
                model_id,
                serial_number,
                unit_number,
                name,
            } => DisplaySelector::Identity(DisplayIdentity {
                uuid: uuid.clone(),
                vendor_id: *vendor_id,
                model_id: *model_id,
                serial_number: *serial_number,
                unit_number: *unit_number,
                name: name.clone(),
            }),
            Self::LiveDisplayId { display_id } => DisplaySelector::LiveDisplayId(*display_id),
        }
    }

    #[must_use]
    pub fn from_selector(sel: &DisplaySelector) -> Self {
        match sel {
            DisplaySelector::Primary => Self::Primary,
            DisplaySelector::Identity(identity) => Self::Identity {
                uuid: identity.uuid.clone(),
                vendor_id: identity.vendor_id,
                model_id: identity.model_id,
                serial_number: identity.serial_number,
                unit_number: identity.unit_number,
                name: identity.name.clone(),
            },
            DisplaySelector::LiveDisplayId(display_id) => Self::LiveDisplayId {
                display_id: *display_id,
            },
        }
    }

    #[must_use]
    /// # Panics
    ///
    /// Panics if a display identity selector cannot be serialized to JSON.
    pub fn id(&self) -> String {
        const PRIMARY_DISPLAY_ID: &str = "primary";
        const IDENTITY_DISPLAY_ID_PREFIX: &str = "identity:";

        match self {
            SerializedSelector::Primary => PRIMARY_DISPLAY_ID.to_string(),
            SerializedSelector::LiveDisplayId { display_id } => display_id.to_string(),
            SerializedSelector::Identity { .. } => {
                let DisplaySelector::Identity(identity) = self.to_selector() else {
                    unreachable!("identity selector must convert to identity")
                };
                format!(
                    "{IDENTITY_DISPLAY_ID_PREFIX}{}",
                    serde_json::to_string(&identity)
                        .expect("display identity selector should serialize")
                )
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonitorCfg {
    #[serde(flatten, default)]
    pub selector: SerializedSelector,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_monitor_mode")]
    pub mode: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wallpaper: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mirror_target: Option<SerializedSelector>,
}

impl Default for MonitorCfg {
    fn default() -> Self {
        Self {
            selector: SerializedSelector::default(),
            enabled: true,
            mode: default_monitor_mode(),
            wallpaper: None,
            mirror_target: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MonitorSettingsCfg {
    #[serde(flatten, default)]
    pub selector: SerializedSelector,
    #[serde(default = "default_scaling_mode")]
    pub scaling_mode: String,
    #[serde(default = "default_scaling_factor")]
    pub scaling_factor: f64,
    #[serde(default = "default_target_fps")]
    pub target_fps: u32,
    #[serde(default = "default_monitor_volume")]
    pub volume: f32,
    #[serde(default)]
    pub muted: bool,
}

impl Default for MonitorSettingsCfg {
    fn default() -> Self {
        Self {
            selector: SerializedSelector::default(),
            scaling_mode: default_scaling_mode(),
            scaling_factor: default_scaling_factor(),
            target_fps: default_target_fps(),
            volume: default_monitor_volume(),
            muted: false,
        }
    }
}

impl MonitorSettingsCfg {
    #[must_use]
    pub fn parse_scaling_mode(&self) -> ScalingMode {
        match self.scaling_mode.to_ascii_lowercase().as_str() {
            "none" => ScalingMode::None,
            "stretch" => ScalingMode::Stretch,
            "fill" => ScalingMode::Fill,
            _ => ScalingMode::Fit,
        }
    }
}

#[allow(clippy::single_call_fn)]
fn default_schema_version() -> u32 {
    SCHEMA_VERSION
}

#[allow(clippy::single_call_fn)]
fn default_true() -> bool {
    true
}

fn default_monitor_mode() -> String {
    "independent".to_string()
}

fn default_scaling_mode() -> String {
    "fit".to_string()
}

fn default_scaling_factor() -> f64 {
    1.0
}

fn default_target_fps() -> u32 {
    DEFAULT_MONITOR_FPS
}

fn default_monitor_volume() -> f32 {
    DEFAULT_MONITOR_VOLUME
}

fn default_selector_window() -> WindowGeom {
    WindowGeom {
        x: 200,
        y: 200,
        width: 1100,
        height: 720,
    }
}

fn default_settings_window() -> WindowGeom {
    WindowGeom {
        x: 520,
        y: 260,
        width: 520,
        height: 440,
    }
}
