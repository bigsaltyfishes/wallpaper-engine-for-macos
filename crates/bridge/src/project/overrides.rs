//! Helpers that convert between the GUI's per-wallpaper override map
//! (`BTreeMap<String, PropertyValue>`) and the flat JSON string that
//! `SceneDesc.property_override_json` consumes.

use std::collections::BTreeMap;

use super::{ProjectModel, ProjectProperty, PropertyKind, PropertyValue};

pub trait OverrideMapExt {
    fn to_override_json(&self) -> String;
}

impl OverrideMapExt for BTreeMap<String, PropertyValue> {
    fn to_override_json(&self) -> String {
        let mut obj = serde_json::Map::new();

        for (key, value) in self {
            obj.insert(key.clone(), value.to_json());
        }

        serde_json::Value::Object(obj).to_string()
    }
}

impl ProjectModel {
    #[must_use]
    pub fn override_values(
        &self,
        values: &BTreeMap<String, serde_json::Value>,
    ) -> BTreeMap<String, PropertyValue> {
        values
            .iter()
            .map(|(id, value)| {
                let value = self.property(id).map_or_else(
                    || PropertyValue::from_json(value),
                    |property| property.value_from_json(value),
                );
                (id.clone(), value)
            })
            .collect()
    }

    #[must_use]
    pub fn apply_edit(
        &self,
        mut values: BTreeMap<String, PropertyValue>,
        id: &str,
        new_value: PropertyValue,
    ) -> BTreeMap<String, PropertyValue> {
        if let Some(property) = self.property(id) {
            if property.value_is_default(&new_value) {
                values.remove(id);
            } else {
                values.insert(id.to_string(), new_value);
            }
        } else {
            values.insert(id.to_string(), new_value);
        }

        values
    }

    pub fn edit_overrides(
        &self,
        values: &mut BTreeMap<String, serde_json::Value>,
        id: &str,
        new_value: PropertyValue,
    ) {
        *values = self
            .apply_edit(self.override_values(values), id, new_value)
            .into_iter()
            .map(|(id, value)| (id, value.to_json()))
            .collect();
    }

    fn property(&self, id: &str) -> Option<&ProjectProperty> {
        self.properties.iter().find(|property| property.id == id)
    }
}

impl ProjectProperty {
    #[must_use]
    pub fn value_from_json(&self, raw: &serde_json::Value) -> PropertyValue {
        let default = self.default_value();
        match self.kind {
            PropertyKind::Slider => raw
                .as_f64()
                .or_else(|| raw.as_str().and_then(|value| value.parse().ok()))
                .filter(|value| value.is_finite())
                .map_or_else(|| default.clone(), PropertyValue::Number),
            PropertyKind::Bool => raw
                .as_bool()
                .or_else(|| {
                    raw.as_str()
                        .and_then(|value| match value.to_ascii_lowercase().as_str() {
                            "true" | "1" => Some(true),
                            "false" | "0" => Some(false),
                            _ => None,
                        })
                })
                .map_or_else(|| default.clone(), PropertyValue::Bool),
            PropertyKind::Combo | PropertyKind::TextInput | PropertyKind::Directory => {
                PropertyValue::String(PropertyValue::json_scalar_to_string(raw))
            }
            PropertyKind::Color => match PropertyValue::from_json(raw) {
                value @ PropertyValue::ColorRgb(..) => value,
                _ => default,
            },
            _ => PropertyValue::from_json(raw),
        }
    }

    #[must_use]
    pub fn default_value(&self) -> PropertyValue {
        match self.kind {
            PropertyKind::Combo | PropertyKind::TextInput | PropertyKind::Directory => {
                PropertyValue::String(self.default_value.to_property_string())
            }
            _ => self.default_value.clone(),
        }
    }

    #[must_use]
    pub fn effective_value(&self, values: &BTreeMap<String, PropertyValue>) -> PropertyValue {
        values
            .get(&self.id)
            .cloned()
            .unwrap_or_else(|| self.default_value())
    }

    #[must_use]
    pub fn value_is_default(&self, value: &PropertyValue) -> bool {
        value.matches_with_override_semantics(&self.default_value())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{PropertyKind, PropertyMetadata};

    fn project_model_with_property(id: &str, default_value: PropertyValue) -> ProjectModel {
        ProjectModel {
            workshop_id: "1".into(),
            title: String::new(),
            description_html: String::new(),
            project_type: wallpaper_core::project::WallpaperProjectType::Unknown,
            preview_file: None,
            properties: vec![ProjectProperty {
                id: id.into(),
                kind: PropertyKind::Bool,
                default_value,
                label_html: String::new(),
                order: 0,
                index: 0,
                condition: None,
                metadata: PropertyMetadata::Bool,
            }],
        }
    }

    #[test]
    fn serialize_empty_is_empty_object() {
        let map = BTreeMap::new();
        assert_eq!(map.to_override_json(), "{}");
    }

    #[test]
    fn apply_edit_inserts_nondefault_value() {
        let model = project_model_with_property("enabled", PropertyValue::Bool(false));
        let map = BTreeMap::new();

        let updated = model.apply_edit(map, "enabled", PropertyValue::Bool(true));

        assert_eq!(updated.get("enabled"), Some(&PropertyValue::Bool(true)));
    }

    #[test]
    fn apply_edit_removes_default_value() {
        let model = project_model_with_property("enabled", PropertyValue::Bool(false));
        let mut map = BTreeMap::new();
        map.insert("enabled".into(), PropertyValue::Bool(true));

        let updated = model.apply_edit(map, "enabled", PropertyValue::Bool(false));

        assert!(!updated.contains_key("enabled"));
    }

    #[test]
    fn apply_edit_removes_nearly_equal_numeric_default() {
        let model = project_model_with_property("opacity", PropertyValue::Number(0.3));
        let mut map = BTreeMap::new();
        map.insert("opacity".into(), PropertyValue::Number(0.4));

        let updated = model.apply_edit(map, "opacity", PropertyValue::Number(0.3 + f64::EPSILON));

        assert!(!updated.contains_key("opacity"));
    }

    #[test]
    fn value_for_text_input_keeps_numeric_triplet_as_string() {
        let prop = ProjectProperty {
            id: "text".into(),
            kind: PropertyKind::TextInput,
            default_value: PropertyValue::String(String::new()),
            label_html: String::new(),
            order: 0,
            index: 0,
            condition: None,
            metadata: PropertyMetadata::TextInput,
        };

        assert_eq!(
            prop.value_from_json(&serde_json::json!("0.1 0.2 0.3")),
            PropertyValue::String("0.1 0.2 0.3".to_string())
        );
    }

    #[test]
    fn combo_default_coerces_triplet_color_back_to_string() {
        let prop = ProjectProperty {
            id: "combo".into(),
            kind: PropertyKind::Combo,
            default_value: PropertyValue::ColorRgb(0.1, 0.2, 0.3),
            label_html: String::new(),
            order: 0,
            index: 0,
            condition: None,
            metadata: PropertyMetadata::Combo {
                options: Vec::new(),
            },
        };

        assert_eq!(
            prop.default_value(),
            PropertyValue::String("0.1 0.2 0.3".to_string())
        );
    }

    #[test]
    fn apply_edit_removes_kind_aware_default_value() {
        let mut model =
            project_model_with_property("combo", PropertyValue::ColorRgb(0.1, 0.2, 0.3));
        model.properties[0].id = "combo".to_string();
        model.properties[0].kind = PropertyKind::Combo;
        model.properties[0].metadata = PropertyMetadata::Combo {
            options: Vec::new(),
        };

        let mut map = BTreeMap::new();
        map.insert("combo".into(), PropertyValue::String("other".to_string()));

        let updated = model.apply_edit(
            map,
            "combo",
            PropertyValue::String("0.1 0.2 0.3".to_string()),
        );

        assert!(!updated.contains_key("combo"));
    }

    #[test]
    fn effective_value_prefers_override_over_default() {
        let prop = ProjectProperty {
            id: "enabled".into(),
            kind: PropertyKind::Bool,
            default_value: PropertyValue::Bool(false),
            label_html: String::new(),
            order: 0,
            index: 0,
            condition: None,
            metadata: PropertyMetadata::Bool,
        };
        let mut map = BTreeMap::new();
        map.insert("enabled".into(), PropertyValue::Bool(true));

        assert_eq!(prop.effective_value(&map), PropertyValue::Bool(true));
    }

    #[test]
    fn effective_value_falls_back_to_default() {
        let prop = ProjectProperty {
            id: "enabled".into(),
            kind: PropertyKind::Bool,
            default_value: PropertyValue::Bool(false),
            label_html: String::new(),
            order: 0,
            index: 0,
            condition: None,
            metadata: PropertyMetadata::Bool,
        };
        let map = BTreeMap::new();

        assert_eq!(prop.effective_value(&map), PropertyValue::Bool(false));
    }

    #[test]
    fn effective_value_uses_kind_aware_default() {
        let prop = ProjectProperty {
            id: "combo".into(),
            kind: PropertyKind::Combo,
            default_value: PropertyValue::ColorRgb(0.1, 0.2, 0.3),
            label_html: String::new(),
            order: 0,
            index: 0,
            condition: None,
            metadata: PropertyMetadata::Combo {
                options: Vec::new(),
            },
        };
        let map = BTreeMap::new();

        assert_eq!(
            prop.effective_value(&map),
            PropertyValue::String("0.1 0.2 0.3".to_string())
        );
    }

    #[test]
    fn value_matches_default_uses_tolerant_numeric_comparison() {
        let prop = ProjectProperty {
            id: "opacity".into(),
            kind: PropertyKind::Slider,
            default_value: PropertyValue::Number(0.3),
            label_html: String::new(),
            order: 0,
            index: 0,
            condition: None,
            metadata: PropertyMetadata::Slider {
                min: 0.0,
                max: 1.0,
                step: 0.01,
                precision: 2,
                fraction: false,
            },
        };

        assert!(prop.value_is_default(&PropertyValue::Number(0.3 + f64::EPSILON)));
    }

    #[test]
    fn serialize_is_stable_order() {
        let mut map = BTreeMap::new();
        map.insert("zeta".into(), PropertyValue::Bool(true));
        map.insert("alpha".into(), PropertyValue::Null);

        assert_eq!(map.to_override_json(), r#"{"alpha":null,"zeta":true}"#);
    }

    #[test]
    fn serialize_uses_property_value_json_shapes() {
        let mut map = BTreeMap::new();
        map.insert("color".into(), PropertyValue::ColorRgb(0.1, 0.2, 0.3));
        map.insert("quote".into(), PropertyValue::String(r#"a"b"#.into()));
        map.insert("scale".into(), PropertyValue::Number(1.5));

        assert_eq!(
            map.to_override_json(),
            r#"{"color":"0.1 0.2 0.3","quote":"a\"b","scale":1.5}"#
        );
    }
}
