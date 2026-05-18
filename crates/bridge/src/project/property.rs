//! Property-kind enum, metadata variants, and runtime value.

#[derive(Clone, Debug, PartialEq)]
pub enum PropertyKind {
    Slider,
    Combo,
    Bool,
    Color,
    TextInput,
    Text,
    Group,
    Directory,
    Unknown(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum PropertyMetadata {
    Slider {
        min: f64,
        max: f64,
        step: f64,
        precision: u32,
        fraction: bool,
    },
    Combo {
        options: Vec<ComboOption>,
    },
    Bool,
    Color,
    TextInput,
    Text,
    Group,
    Directory,
    Unknown,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ComboOption {
    pub label: String,
    pub value: String,
}

/// Editor value. Serde-compatible for round-trip to/from
/// `property_override_json`.
#[derive(Clone, Debug, PartialEq)]
pub enum PropertyValue {
    Bool(bool),
    Number(f64),
    String(String),
    ColorRgb(f32, f32, f32), // 0..=1
    Null,
}

impl PropertyValue {
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        use serde_json::Value;

        match self {
            Self::Bool(b) => Value::Bool(*b),
            Self::Number(n) => serde_json::Number::from_f64(*n).map_or(Value::Null, Value::Number),
            Self::String(s) => Value::String(s.clone()),
            Self::ColorRgb(r, g, b) => Value::String(format!("{r} {g} {b}")),
            Self::Null => Value::Null,
        }
    }

    #[must_use]
    pub fn json_scalar_to_string(value: &serde_json::Value) -> String {
        match value {
            serde_json::Value::String(value) => value.clone(),
            serde_json::Value::Number(value) => value.to_string(),
            serde_json::Value::Bool(value) => value.to_string(),
            serde_json::Value::Null => String::new(),
            other => other.to_string(),
        }
    }

    #[must_use]
    pub fn from_json(v: &serde_json::Value) -> Self {
        match v {
            serde_json::Value::Bool(b) => Self::Bool(*b),
            serde_json::Value::Number(n) => Self::Number(n.as_f64().unwrap_or(0.0)),
            serde_json::Value::String(s) => {
                let parts: Vec<_> = s.split_whitespace().collect();
                if parts.len() == 3 {
                    let parsed = (
                        parts[0].parse::<f32>().ok(),
                        parts[1].parse::<f32>().ok(),
                        parts[2].parse::<f32>().ok(),
                    );
                    if let (Some(r), Some(g), Some(b)) = parsed
                        && (0.0..=1.0).contains(&r)
                        && (0.0..=1.0).contains(&g)
                        && (0.0..=1.0).contains(&b)
                    {
                        return Self::ColorRgb(r, g, b);
                    }
                }

                Self::String(s.clone())
            }
            serde_json::Value::Null => Self::Null,
            _ => Self::String(v.to_string()),
        }
    }

    #[must_use]
    pub fn to_property_string(&self) -> String {
        match self {
            Self::String(value) => value.clone(),
            Self::Number(value) => value.to_string(),
            Self::Bool(value) => value.to_string(),
            Self::ColorRgb(r, g, b) => format!("{r} {g} {b}"),
            Self::Null => String::new(),
        }
    }

    pub fn matches_with_override_semantics(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Number(left), Self::Number(right)) => {
                if !left.is_finite() || !right.is_finite() {
                    return left
                        .partial_cmp(right)
                        .is_some_and(std::cmp::Ordering::is_eq);
                }

                let scale = left.abs().max(right.abs()).max(1.0);
                (left - right).abs() <= f64::EPSILON * scale
            }
            (Self::ColorRgb(left_r, left_g, left_b), Self::ColorRgb(right_r, right_g, right_b)) => {
                [
                    (*left_r, *right_r),
                    (*left_g, *right_g),
                    (*left_b, *right_b),
                ]
                .into_iter()
                .all(|(left, right)| {
                    if !left.is_finite() || !right.is_finite() {
                        return left
                            .partial_cmp(&right)
                            .is_some_and(std::cmp::Ordering::is_eq);
                    }

                    let scale = left.abs().max(right.abs()).max(1.0);
                    (left - right).abs() <= f32::EPSILON * scale
                })
            }
            _ => self == other,
        }
    }

    #[must_use]
    pub fn as_color_rgb(&self) -> Option<(f32, f32, f32)> {
        match self {
            Self::ColorRgb(r, g, b) => Some((*r, *g, *b)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn property_value_round_trip_bool() {
        let v = PropertyValue::Bool(true);
        assert_eq!(PropertyValue::from_json(&v.to_json()), v);
    }

    #[test]
    fn property_value_round_trip_number() {
        let v = PropertyValue::Number(3.25);
        assert_eq!(PropertyValue::from_json(&v.to_json()), v);
    }

    #[test]
    fn property_value_color_from_string() {
        let json = serde_json::json!("0.5 0.25 0.75");
        assert_eq!(
            PropertyValue::from_json(&json),
            PropertyValue::ColorRgb(0.5, 0.25, 0.75)
        );
    }

    #[test]
    fn property_value_plain_string_is_not_misinterpreted_as_color() {
        let json = serde_json::json!("hello world");
        assert_eq!(
            PropertyValue::from_json(&json),
            PropertyValue::String("hello world".into())
        );
    }

    #[test]
    fn property_value_json_scalar_to_string_preserves_scalar_text() {
        assert_eq!(
            PropertyValue::json_scalar_to_string(&serde_json::json!("0.1 0.2 0.3")),
            "0.1 0.2 0.3"
        );
        assert_eq!(
            PropertyValue::json_scalar_to_string(&serde_json::json!(1.5)),
            "1.5"
        );
        assert_eq!(
            PropertyValue::json_scalar_to_string(&serde_json::json!(true)),
            "true"
        );
    }

    #[test]
    fn property_value_to_property_string_formats_all_override_scalars() {
        assert_eq!(PropertyValue::Bool(true).to_property_string(), "true");
        assert_eq!(PropertyValue::Number(1.5).to_property_string(), "1.5");
        assert_eq!(
            PropertyValue::ColorRgb(0.1, 0.2, 0.3).to_property_string(),
            "0.1 0.2 0.3"
        );
        assert_eq!(PropertyValue::Null.to_property_string(), String::new());
    }

    #[test]
    fn combo_option_equality() {
        assert_eq!(
            ComboOption {
                label: "a".into(),
                value: "b".into()
            },
            ComboOption {
                label: "a".into(),
                value: "b".into()
            },
        );
    }
}
