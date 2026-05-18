use std::{
    collections::BTreeMap,
    fmt::{Display, Formatter},
};

use serde_json::{Map, Value};

pub type FlatScenePropertyOverride = BTreeMap<String, Value>;

#[derive(Debug)]
pub enum ScenePropertyOverrideError {
    InvalidJson(serde_json::Error),
    RootMustBeObject,
    EmptyPath,
    UnsupportedLeafType(String),
}

impl Display for ScenePropertyOverrideError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidJson(error) => write!(f, "{error}"),
            Self::RootMustBeObject => write!(f, "override root must be a JSON object"),
            Self::EmptyPath => write!(f, "override path must not be empty"),
            Self::UnsupportedLeafType(path) => {
                write!(
                    f,
                    "override leaf must be bool, number, string, or null: {path}"
                )
            }
        }
    }
}

impl std::error::Error for ScenePropertyOverrideError {}

impl From<ScenePropertyOverrideError> for crate::EngineError {
    fn from(value: ScenePropertyOverrideError) -> Self {
        crate::EngineError::InvalidInput(value.to_string())
    }
}

pub trait SerdeValudeExt {
    /// Flattens nested JSON object properties into underscore-delimited keys.
    ///
    /// # Errors
    ///
    /// Returns an error when the root value is not an object, a generated path
    /// is empty, or a leaf value uses an unsupported JSON type.
    fn flatten(self) -> Result<BTreeMap<String, Value>, ScenePropertyOverrideError>;
}

impl SerdeValudeExt for Value {
    fn flatten(self) -> Result<BTreeMap<String, Value>, ScenePropertyOverrideError> {
        let object = self
            .as_object()
            .ok_or(ScenePropertyOverrideError::RootMustBeObject)?;
        let mut flat = BTreeMap::new();
        flatten_object(&mut flat, Vec::new(), object)?;
        Ok(flat)
    }
}

fn flatten_object(
    out: &mut FlatScenePropertyOverride,
    mut prefix: Vec<String>,
    object: &Map<String, Value>,
) -> Result<(), ScenePropertyOverrideError> {
    for (key, value) in object {
        prefix.push(key.clone());
        match value {
            Value::Object(child) => flatten_object(out, prefix.clone(), child)?,
            Value::Bool(_) | Value::Number(_) | Value::String(_) => {
                let flat_key = prefix.join("_");
                if flat_key.is_empty() {
                    return Err(ScenePropertyOverrideError::EmptyPath);
                }
                out.insert(flat_key, value.clone());
            }
            Value::Null => {
                out.insert(prefix.join("_"), Value::String(String::new()));
            }
            Value::Array(_) => {
                return Err(ScenePropertyOverrideError::UnsupportedLeafType(
                    prefix.join("_"),
                ));
            }
        }
        prefix.pop();
    }
    Ok(())
}
