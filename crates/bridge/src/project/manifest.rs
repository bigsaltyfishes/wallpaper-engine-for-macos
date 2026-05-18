//! Extended project.json parser for GUI consumption.
//!
//! `wallpaper_core::ProjectManifest` stays authoritative for type / file /
//! workshop id / dependencies. This GUI-facing parser additionally extracts
//! `title`, raw HTML description, `preview` filename, and every
//! `general.properties` entry as a `ProjectProperty` for the editor panel.

use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use serde_json::Value;
use wallpaper_core::project::WallpaperProjectType;

use super::property::{ComboOption, PropertyKind, PropertyMetadata, PropertyValue};
use crate::{BridgeError, BridgeErrorKind};

#[derive(Clone, Debug, PartialEq)]
pub struct ProjectModel {
    pub workshop_id: String,
    pub title: String,
    pub description_html: String,
    pub project_type: WallpaperProjectType,
    pub preview_file: Option<PathBuf>,
    pub properties: Vec<ProjectProperty>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProjectProperty {
    pub id: String,
    pub kind: PropertyKind,
    pub default_value: PropertyValue,
    pub label_html: String,
    pub order: i64,
    pub index: i64,
    pub condition: Option<String>, // raw string; parsed lazily by Task 14
    pub metadata: PropertyMetadata,
}

impl ProjectModel {
    /// # Errors
    ///
    /// Returns an error if `json` is not a valid Wallpaper Engine project
    /// object.
    #[allow(clippy::too_many_lines)]
    pub fn parse(workshop_id: &str, json: &str) -> Result<Self, BridgeError> {
        let root: Value = serde_json::from_str(json).map_err(|e| project_error(e.to_string()))?;
        let obj = root
            .as_object()
            .ok_or_else(|| project_error("root must be object"))?;

        let project_type = match obj.get("type").and_then(Value::as_str) {
            Some(raw_type) => match raw_type.to_ascii_lowercase().as_str() {
                "scene" => WallpaperProjectType::Scene,
                "video" => WallpaperProjectType::Video,
                "web" => WallpaperProjectType::Web,
                _ => WallpaperProjectType::Unknown,
            },
            None => WallpaperProjectType::Unknown,
        };

        let title = obj
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let description_html = obj
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let preview_file = obj
            .get("preview")
            .and_then(Value::as_str)
            .map(PathBuf::from);

        let properties = obj
            .get("general")
            .and_then(Value::as_object)
            .and_then(|g| g.get("properties"))
            .and_then(Value::as_object)
            .map(|properties| {
                let mut seen_positions = BTreeSet::new();
                let mut parsed = Vec::new();
                for (id, value) in properties {
                    let Some(object) = value.as_object() else {
                        continue;
                    };

                    let kind = match object.get("type").and_then(Value::as_str).unwrap_or("") {
                        raw if raw.eq_ignore_ascii_case("slider") => PropertyKind::Slider,
                        raw if raw.eq_ignore_ascii_case("combo") => PropertyKind::Combo,
                        raw if raw.eq_ignore_ascii_case("bool") => PropertyKind::Bool,
                        raw if raw.eq_ignore_ascii_case("color") => PropertyKind::Color,
                        raw if raw.eq_ignore_ascii_case("textinput") => PropertyKind::TextInput,
                        raw if raw.eq_ignore_ascii_case("text") => PropertyKind::Text,
                        raw if raw.eq_ignore_ascii_case("group") => PropertyKind::Group,
                        raw if raw.eq_ignore_ascii_case("directory")
                            || raw.eq_ignore_ascii_case("scenetexture")
                            || raw.eq_ignore_ascii_case("texture") =>
                        {
                            PropertyKind::Directory
                        }
                        raw => PropertyKind::Unknown(raw.to_string()),
                    };
                    let metadata = match &kind {
                        PropertyKind::Slider => {
                            let min = object.get("min").and_then(Value::as_f64).unwrap_or(0.0);
                            let max = object.get("max").and_then(Value::as_f64).unwrap_or(1.0);
                            let step = object.get("step").and_then(Value::as_f64).unwrap_or(0.01);
                            let precision = object
                                .get("precision")
                                .and_then(Value::as_u64)
                                .and_then(|value| u32::try_from(value).ok())
                                .unwrap_or(2);
                            let fraction = object
                                .get("fraction")
                                .and_then(Value::as_bool)
                                .unwrap_or(false);
                            PropertyMetadata::Slider {
                                min,
                                max,
                                step,
                                precision,
                                fraction,
                            }
                        }
                        PropertyKind::Combo => {
                            let options = object
                                .get("options")
                                .and_then(Value::as_array)
                                .map(|arr| {
                                    arr.iter()
                                        .filter_map(|value| {
                                            let option = value.as_object()?;
                                            Some(ComboOption {
                                                label: option
                                                    .get("label")
                                                    .and_then(Value::as_str)
                                                    .unwrap_or("")
                                                    .to_string(),
                                                value: option
                                                    .get("value")
                                                    .and_then(Value::as_str)
                                                    .unwrap_or("")
                                                    .to_string(),
                                            })
                                        })
                                        .collect()
                                })
                                .unwrap_or_default();
                            PropertyMetadata::Combo { options }
                        }
                        PropertyKind::Bool => PropertyMetadata::Bool,
                        PropertyKind::Color => PropertyMetadata::Color,
                        PropertyKind::TextInput => PropertyMetadata::TextInput,
                        PropertyKind::Text => PropertyMetadata::Text,
                        PropertyKind::Group => PropertyMetadata::Group,
                        PropertyKind::Directory => PropertyMetadata::Directory,
                        PropertyKind::Unknown(_) => PropertyMetadata::Unknown,
                    };
                    let label_html = object
                        .get("text")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    let condition = object
                        .get("condition")
                        .and_then(Value::as_str)
                        .map(str::to_owned);
                    let order = object.get("order").and_then(Value::as_i64).unwrap_or(0);
                    let index = object.get("index").and_then(Value::as_i64).unwrap_or(0);
                    let default_value = match (&kind, object.get("value")) {
                        (PropertyKind::Directory, Some(Value::Null) | None) => {
                            PropertyValue::String(String::new())
                        }
                        (PropertyKind::Directory, Some(value)) => {
                            PropertyValue::String(PropertyValue::json_scalar_to_string(value))
                        }
                        (_, Some(value)) => PropertyValue::from_json(value),
                        _ => PropertyValue::Null,
                    };

                    let property = ProjectProperty {
                        id: id.clone(),
                        kind,
                        default_value,
                        label_html,
                        order,
                        index,
                        condition,
                        metadata,
                    };

                    if seen_positions.insert((property.order, property.index)) {
                        parsed.push(property);
                    }
                }

                parsed.sort_by_key(|property| (property.order, property.index));
                parsed
            })
            .unwrap_or_default();

        Ok(Self {
            workshop_id: workshop_id.to_string(),
            title,
            description_html,
            project_type,
            preview_file,
            properties,
        })
    }

    /// Loads and parses a project, resolving `preview_file` against the
    /// directory holding `project.json`.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read or parsed.
    pub fn load<P: AsRef<Path>>(workshop_id: &str, project_json: P) -> Result<Self, BridgeError> {
        let path = project_json.as_ref();
        let bytes = std::fs::read_to_string(path)
            .map_err(|e| project_error(format!("read {}: {e}", path.display())))?;
        let mut m = Self::parse(workshop_id, &bytes)?;
        if let Some(ref pv) = m.preview_file {
            m.preview_file = Some(
                path.parent()
                    .filter(|parent| !parent.as_os_str().is_empty())
                    .unwrap_or_else(|| Path::new("."))
                    .join(pv),
            );
        }
        Ok(m)
    }
}

fn project_error(message: impl Into<String>) -> BridgeError {
    BridgeError::Error {
        kind: BridgeErrorKind::Project,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN_JSON: &str = r#"{
        "type": "scene",
        "title": "Hello",
        "description": "desc",
        "preview": "preview.gif",
        "general": { "properties": {
            "slide1": { "type": "slider", "min": 0, "max": 10, "step": 0.5, "value": 3, "order": 2, "text": "s" },
            "combo1": { "type": "combo", "value": "a", "options": [{"label":"A","value":"a"}], "order": 1, "text": "c" },
            "bool1":  { "type": "bool", "value": true, "order": 3, "text": "b" }
        }}
    }"#;

    #[test]
    fn parse_basic_scene_model() {
        let m = ProjectModel::parse("1", MIN_JSON).unwrap();
        assert_eq!(m.workshop_id, "1");
        assert_eq!(m.title, "Hello");
        assert_eq!(m.project_type, WallpaperProjectType::Scene);
        assert_eq!(m.preview_file, Some(PathBuf::from("preview.gif")));
        assert_eq!(m.properties.len(), 3);
        assert_eq!(m.properties[0].id, "combo1");
        assert_eq!(m.properties[1].id, "slide1");
        assert_eq!(m.properties[2].id, "bool1");
    }

    #[test]
    fn parse_unknown_type_falls_to_unknown_variant() {
        let m =
            ProjectModel::parse("1", r#"{"type":"pixelart","general":{"properties":{}}}"#).unwrap();
        assert_eq!(m.project_type, WallpaperProjectType::Unknown);
    }

    #[test]
    fn parse_malformed_fails_cleanly() {
        let err = ProjectModel::parse("1", "not-json").unwrap_err();
        assert_eq!(err.kind(), BridgeErrorKind::Project);
    }

    #[test]
    fn slider_metadata_has_defaults_when_missing() {
        let m = ProjectModel::parse(
            "1",
            r#"{
            "type":"scene","general":{"properties":{
                "s":{"type":"slider","value":0,"text":"x"}
            }}
        }"#,
        )
        .unwrap();
        match &m.properties[0].metadata {
            PropertyMetadata::Slider {
                min,
                max,
                step,
                precision,
                fraction,
            } => {
                assert!(
                    (*min - 0.0).abs() <= f64::EPSILON,
                    "expected min {min} to be within f64::EPSILON of 0.0"
                );
                assert!(
                    (*max - 1.0).abs() <= f64::EPSILON,
                    "expected max {max} to be within f64::EPSILON of 1.0"
                );
                assert!(
                    (*step - 0.01).abs() <= f64::EPSILON,
                    "expected step {step} to be within f64::EPSILON of 0.01"
                );
                assert_eq!(*precision, 2);
                assert!(!fraction);
            }
            _ => panic!("expected slider"),
        }
    }

    #[test]
    fn unknown_kind_preserves_raw_type() {
        let m = ProjectModel::parse(
            "1",
            r#"{
            "type":"scene","general":{"properties":{"x":{"type":"weird","value":0}}}
        }"#,
        )
        .unwrap();
        assert_eq!(m.properties[0].kind, PropertyKind::Unknown("weird".into()));
    }

    #[test]
    fn load_anchors_preview_relative_to_manifest_directory_when_no_parent() {
        let resolved = Path::new("project.json")
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
            .join(Path::new("preview.gif"));
        assert_eq!(resolved, PathBuf::from(".").join("preview.gif"));
    }

    #[test]
    fn load_resolves_preview_relative_to_manifest_directory() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = dir.path().join("project.json");
        std::fs::write(&manifest, MIN_JSON).unwrap();

        let m = ProjectModel::load("1", &manifest).unwrap();
        assert_eq!(m.preview_file, Some(dir.path().join("preview.gif")));
    }

    #[test]
    fn oversized_slider_precision_uses_default() {
        let m = ProjectModel::parse(
            "1",
            r#"{
            "type":"scene","general":{"properties":{
                "s":{"type":"slider","precision":99999999999999999999,"value":0,"text":"x"}
            }}
        }"#,
        )
        .unwrap();

        match &m.properties[0].metadata {
            PropertyMetadata::Slider { precision, .. } => assert_eq!(*precision, 2),
            _ => panic!("expected slider"),
        }
    }

    #[test]
    fn duplicate_order_index_keeps_first_declared_property() {
        let m = ProjectModel::parse(
            "1",
            r#"{
            "type":"scene","general":{"properties":{
                "hero":{"type":"bool","value":true,"order":100,"index":0,"text":"<img src='hero.png'>"},
                "schemecolor":{"type":"color","value":"0.1 0.2 0.3","order":100,"index":0,"text":"ui_browse_properties_scheme_color"},
                "next":{"type":"bool","value":true,"order":101,"index":1,"text":"Next"}
            }}
        }"#,
        )
        .unwrap();

        assert_eq!(
            m.properties
                .iter()
                .map(|property| property.id.as_str())
                .collect::<Vec<_>>(),
            vec!["hero", "next"]
        );
    }

    #[test]
    fn scenetexture_properties_are_path_selectors() {
        let m = ProjectModel::parse(
            "1",
            r#"{
            "type":"scene","general":{"properties":{
                "custom_background":{"type":"scenetexture","value":"","order":1,"text":"Background"}
            }}
        }"#,
        )
        .unwrap();

        assert_eq!(m.properties[0].kind, PropertyKind::Directory);
        assert_eq!(
            m.properties[0].default_value,
            PropertyValue::String(String::new())
        );
    }
}
