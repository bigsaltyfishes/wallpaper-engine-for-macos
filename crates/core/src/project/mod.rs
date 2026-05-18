//! Wallpaper Engine project metadata parsing.
//!
//! These types replace the old native manifest/property helpers with idiomatic
//! Rust loaders and parsers. They intentionally keep parsed data small and
//! behavior-oriented: callers ask for project type, source paths, dependency
//! paths, or runtime scalar values instead of working with raw JSON objects.

use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path, PathBuf},
};

use serde_json::Value;

mod r#override;
mod resolver;
mod scene;

pub use r#override::{FlatScenePropertyOverride, ScenePropertyOverrideError, SerdeValudeExt};
pub use resolver::{SceneSourcePaths, SceneSourceResolution};
pub use scene::{
    ScalingMode, SceneDesc, SceneDescBuilder, SceneDescSliceExt, SceneFile, SceneHandle, SceneIr,
    SceneRenderTargetIr, SceneResult, SceneTemplate,
};

/// High-level type of a Wallpaper Engine project.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum WallpaperProjectType {
    /// Scene wallpaper backed by a scene file or scene package.
    Scene,
    /// Video wallpaper backed by a media file.
    Video,
    /// Web wallpaper. Parsing support is limited to metadata at this layer.
    Web,
    /// Missing or unrecognized project type.
    #[default]
    Unknown,
}

/// Parsed `project.json` metadata.
///
/// The manifest keeps only the fields wallpaper-core needs for source
/// resolution and smoke coverage. Unknown JSON fields are ignored.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProjectManifest {
    project_type: WallpaperProjectType,
    file: String,
    workshop_id: String,
    dependencies: Vec<String>,
}

impl ProjectManifest {
    /// Loads and parses a project manifest from disk.
    ///
    /// # Errors
    ///
    /// Returns [`crate::EngineError::InvalidInput`] if the file cannot be read
    /// or the manifest JSON is invalid.
    pub fn load<T: AsRef<Path>>(path: T) -> Result<Self, crate::EngineError> {
        let content = fs::read_to_string(path.as_ref()).map_err(|error| {
            crate::EngineError::InvalidInput(format!("failed to read project manifest: {error}"))
        })?;
        Self::parse(content)
    }

    /// Parses a project manifest from JSON text.
    ///
    /// # Errors
    ///
    /// Returns [`crate::EngineError::InvalidInput`] if the JSON is invalid, the
    /// root is not an object, or recognized manifest fields have the wrong
    /// type.
    pub fn parse<T: AsRef<str>>(json: T) -> Result<Self, crate::EngineError> {
        let project: Value = serde_json::from_str(json.as_ref()).map_err(|error| {
            crate::EngineError::InvalidInput(format!("failed to parse project manifest: {error}"))
        })?;
        let object = project.as_object().ok_or_else(|| {
            crate::EngineError::InvalidInput("project manifest root must be an object".to_string())
        })?;

        let project_type = match object.get("type") {
            Some(Value::String(value)) => WallpaperProjectType::from(value.as_str()),
            Some(_) => {
                return Err(crate::EngineError::InvalidInput(
                    "project manifest `type` must be a string".to_string(),
                ));
            }
            None => WallpaperProjectType::Unknown,
        };

        let file = match object.get("file") {
            Some(Value::String(value)) => value.clone(),
            Some(_) => {
                return Err(crate::EngineError::InvalidInput(
                    "project manifest `file` must be a string".to_string(),
                ));
            }
            None => String::new(),
        };

        let workshop_id = match object.get("workshopid") {
            Some(Value::String(value)) => value.clone(),
            Some(Value::Number(value)) => value.to_string(),
            _ => String::new(),
        };

        let dependencies = match object.get("dependencies") {
            Some(Value::Array(values)) => values
                .iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect(),
            _ => Vec::new(),
        };

        Ok(Self {
            project_type,
            file,
            workshop_id,
            dependencies,
        })
    }

    /// Returns the recognized project type.
    #[must_use]
    pub fn project_type(&self) -> WallpaperProjectType {
        self.project_type
    }

    /// Returns the project file entry exactly as declared by the manifest.
    #[must_use]
    pub fn file(&self) -> &str {
        &self.file
    }

    /// Returns the Steam workshop ID if the manifest declared one.
    #[must_use]
    pub fn workshop_id(&self) -> &str {
        &self.workshop_id
    }

    /// Resolves manifest dependency entries relative to `project_root`.
    #[must_use]
    pub fn dependency_paths<T: AsRef<Path>>(&self, project_root: T) -> Vec<PathBuf> {
        self.dependencies
            .iter()
            .map(|dependency| project_root.as_ref().join(dependency))
            .collect()
    }
}

impl From<&str> for WallpaperProjectType {
    fn from(value: &str) -> Self {
        if value.eq_ignore_ascii_case("scene") {
            Self::Scene
        } else if value.eq_ignore_ascii_case("video") {
            Self::Video
        } else if value.eq_ignore_ascii_case("web") {
            Self::Web
        } else {
            Self::Unknown
        }
    }
}

/// Project properties converted into runtime scalar values.
///
/// Wallpaper Engine stores configurable project properties under
/// `general.properties`. This type normalizes those values so scene runtime
/// code can apply overrides without retaining the full JSON tree.
#[derive(Clone, Debug, PartialEq)]
pub struct ProjectProperties(BTreeMap<String, RuntimeScalarValue>);

impl ProjectProperties {
    /// Loads project properties from disk.
    ///
    /// # Errors
    ///
    /// Returns [`crate::EngineError::InvalidInput`] if the file cannot be read
    /// or the properties JSON is invalid.
    pub fn load<T: AsRef<Path>>(path: T) -> Result<Self, crate::EngineError> {
        let content = fs::read_to_string(path.as_ref()).map_err(|error| {
            crate::EngineError::InvalidInput(format!("failed to read project properties: {error}"))
        })?;
        Self::parse(content)
    }

    /// Parses project properties from JSON text.
    ///
    /// # Errors
    ///
    /// Returns [`crate::EngineError::InvalidInput`] if the JSON is invalid or
    /// does not contain an object at `general.properties`.
    pub fn parse<T: AsRef<str>>(json: T) -> Result<Self, crate::EngineError> {
        let project: Value = serde_json::from_str(json.as_ref()).map_err(|error| {
            crate::EngineError::InvalidInput(format!("failed to parse project properties: {error}"))
        })?;
        let properties = project
            .get("general")
            .and_then(Value::as_object)
            .and_then(|general| general.get("properties"))
            .and_then(Value::as_object)
            .ok_or_else(|| {
                crate::EngineError::InvalidInput(
                    "project general section must contain object `properties`".to_string(),
                )
            })?;

        let values = properties
            .iter()
            .filter_map(|(name, property)| {
                property
                    .as_object()
                    .and_then(|object| object.get("value"))
                    .map(|value| (name.clone(), RuntimeScalarValue::from_json(value)))
            })
            .collect();

        Ok(Self(values))
    }

    /// Applies a flat project-property override JSON object.
    ///
    /// Override keys replace or add values in the returned copy. The original
    /// `ProjectProperties` value is left unchanged.
    ///
    /// # Errors
    ///
    /// Returns [`crate::EngineError::InvalidInput`] if the override JSON is
    /// invalid or its root is not an object.
    pub fn apply_override<T: AsRef<str>>(&self, json: T) -> Result<Self, crate::EngineError> {
        let override_json: Value = serde_json::from_str(json.as_ref()).map_err(|error| {
            crate::EngineError::InvalidInput(format!("failed to parse project override: {error}"))
        })?;
        let object = override_json.as_object().ok_or_else(|| {
            crate::EngineError::InvalidInput("override json must be an object".to_string())
        })?;

        let mut merged = self.0.clone();
        for (name, value) in object {
            merged.insert(name.clone(), RuntimeScalarValue::from_json(value));
        }
        Ok(Self(merged))
    }

    /// Returns one normalized runtime value by property name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&RuntimeScalarValue> {
        self.0.get(name)
    }
}

/// Scalar project-property value as consumed by scene runtime code.
#[derive(Clone, Debug, PartialEq)]
pub struct RuntimeScalarValue {
    kind: RuntimeScalarKind,
    bool_value: bool,
    float_value: f64,
    string_value: String,
}

impl RuntimeScalarValue {
    /// Creates a boolean runtime value.
    #[must_use]
    pub fn bool(value: bool) -> Self {
        Self {
            kind: RuntimeScalarKind::Bool,
            bool_value: value,
            float_value: if value { 1.0 } else { 0.0 },
            string_value: value.to_string(),
        }
    }

    /// Creates a floating-point runtime value.
    #[must_use]
    pub fn float(value: f64) -> Self {
        Self {
            kind: RuntimeScalarKind::Float,
            bool_value: value != 0.0,
            float_value: value,
            string_value: value.to_string(),
        }
    }

    /// Creates a string runtime value.
    #[must_use]
    pub fn string(value: impl Into<String>) -> Self {
        let string_value = value.into();
        Self {
            kind: RuntimeScalarKind::String,
            bool_value: string_is_true(&string_value),
            float_value: string_value.parse::<f64>().unwrap_or(0.0),
            string_value,
        }
    }

    /// Returns the value converted to Wallpaper Engine boolean semantics.
    #[must_use]
    pub fn as_bool(&self) -> bool {
        match self.kind {
            RuntimeScalarKind::Bool => self.bool_value,
            RuntimeScalarKind::Float => self.float_value != 0.0,
            RuntimeScalarKind::String => string_is_true(&self.string_value),
        }
    }

    /// Returns the value converted to Wallpaper Engine numeric semantics.
    #[must_use]
    pub fn as_float(&self) -> f64 {
        match self.kind {
            RuntimeScalarKind::Bool => {
                if self.bool_value {
                    1.0
                } else {
                    0.0
                }
            }
            RuntimeScalarKind::Float => self.float_value,
            RuntimeScalarKind::String => self.string_value.parse::<f64>().unwrap_or(0.0),
        }
    }

    /// Returns the value as the runtime string representation.
    #[must_use]
    pub fn as_string(&self) -> &str {
        &self.string_value
    }

    fn from_json(value: &Value) -> Self {
        match value {
            Value::Bool(value) => Self::bool(*value),
            Value::Number(value) => Self::float(value.as_f64().unwrap_or(0.0)),
            Value::String(value) => Self::string(value.clone()),
            Value::Null => Self::string(""),
            other => Self::string(other.to_string()),
        }
    }
}

/// Original scalar category used to derive conversion behavior.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RuntimeScalarKind {
    /// Boolean property value.
    Bool,
    /// Numeric property value.
    Float,
    /// String or non-scalar JSON fallback value.
    String,
}

fn string_is_true(value: &str) -> bool {
    value.eq_ignore_ascii_case("true") || value == "1"
}

/// Validates that `path` is a non-empty relative path with normal components.
///
/// # Errors
///
/// Returns [`crate::EngineError::InvalidInput`] if the path is empty, absolute,
/// contains non-normal components, or contains Windows path separators.
pub fn validate_relative_normal_path(path: &Path, label: &str) -> Result<(), crate::EngineError> {
    if path.as_os_str().is_empty() {
        return Err(crate::EngineError::InvalidInput(format!(
            "{label} must not be empty"
        )));
    }

    let mut has_component = false;
    for component in path.components() {
        match component {
            Component::Normal(value) if !value.to_string_lossy().contains('\\') => {
                has_component = true;
            }
            _ => {
                return Err(crate::EngineError::InvalidInput(format!(
                    "{label} must be a relative path with normal components"
                )));
            }
        }
    }

    if !has_component {
        return Err(crate::EngineError::InvalidInput(format!(
            "{label} must not be empty"
        )));
    }

    Ok(())
}
