use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use wallpaper_core::project::ScalingMode;

use super::app::SerializedSelector;
use crate::project::PropertyValue;

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WallpaperConfig {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub workshop_id: String,
    #[serde(default, rename = "type")]
    pub r#type: String,
    #[serde(default)]
    pub audio: AudioCfg,
    #[serde(default)]
    pub monitors: Vec<MonitorRender>,
    #[serde(default)]
    pub property_overrides: BTreeMap<String, Value>,
}

impl Default for WallpaperConfig {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            workshop_id: String::new(),
            r#type: String::new(),
            audio: AudioCfg::default(),
            monitors: Vec::new(),
            property_overrides: BTreeMap::new(),
        }
    }
}

impl WallpaperConfig {
    #[must_use]
    pub fn new_for(workshop_id: impl Into<String>, type_str: impl Into<String>) -> Self {
        Self {
            workshop_id: workshop_id.into(),
            r#type: type_str.into(),
            ..Self::default()
        }
    }

    #[must_use]
    pub fn override_json(&self, id: &str) -> Option<&Value> {
        self.property_overrides.get(id)
    }

    pub fn override_value(&self, id: &str) -> Option<PropertyValue> {
        self.override_json(id).map(PropertyValue::from_json)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AudioCfg {
    #[serde(default = "default_audio_volume")]
    pub volume: f32,
    #[serde(default)]
    pub response_enabled: bool,
    #[serde(default)]
    pub muted: bool,
}

impl Default for AudioCfg {
    fn default() -> Self {
        Self {
            volume: default_audio_volume(),
            response_enabled: false,
            muted: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MonitorRender {
    #[serde(default)]
    pub selector: SerializedSelector,
    #[serde(default = "default_scaling_mode")]
    pub scaling_mode: String,
    #[serde(default = "default_scaling_factor")]
    pub scaling_factor: f64,
    #[serde(default = "default_fps")]
    pub fps: u32,
}

impl Default for MonitorRender {
    fn default() -> Self {
        Self {
            selector: SerializedSelector::default(),
            scaling_mode: default_scaling_mode(),
            scaling_factor: default_scaling_factor(),
            fps: default_fps(),
        }
    }
}

impl MonitorRender {
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

fn default_schema_version() -> u32 {
    SCHEMA_VERSION
}

fn default_audio_volume() -> f32 {
    1.0
}

fn default_scaling_mode() -> String {
    "fit".to_string()
}

fn default_scaling_factor() -> f64 {
    1.0
}

fn default_fps() -> u32 {
    60
}
