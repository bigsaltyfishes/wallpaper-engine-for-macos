//! Project property values used by shader requests.

use std::fmt;

use smol_str::SmolStr;

use crate::{ShaderError, ShaderResult};

/// Name of a project property visible to shader material bindings.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(transparent)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct PropertyName(SmolStr);

impl PropertyName {
    /// Creates a validated property name.
    ///
    /// # Errors
    ///
    /// Returns an error when the name is empty or contains a NUL byte.
    pub fn new(name: impl Into<String>) -> ShaderResult<Self> {
        let name = name.into();
        if name.is_empty() {
            return Err(ShaderError::invalid_request("property name is empty"));
        }
        if name.contains('\0') {
            return Err(ShaderError::invalid_request(
                "property name contains nul byte",
            ));
        }
        Ok(Self(SmolStr::new(name)))
    }

    /// Returns the property name as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Display for PropertyName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for PropertyName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

/// Project property value supported by the shader model.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PropertyValue {
    /// String value.
    String(String),
    /// Numeric scalar value.
    Number(f32),
    /// Boolean value.
    Bool(bool),
    /// Two-component numeric vector.
    Vec2([f32; 2]),
    /// Three-component numeric vector.
    Vec3([f32; 3]),
    /// Four-component numeric vector.
    Vec4([f32; 4]),
    /// Four-by-four numeric matrix in source order.
    Matrix4([f32; 16]),
    /// Parsed nullable value that is rejected by active shader requests.
    None,
}

/// Binding from a typed project property name to a concrete shader request
/// value.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ProjectPropertyBinding {
    /// Validated project property name.
    name: PropertyName,
    /// Concrete value supplied for the property.
    value: PropertyValue,
}

impl ProjectPropertyBinding {
    /// Creates a project property binding.
    #[must_use]
    pub const fn new(name: PropertyName, value: PropertyValue) -> Self {
        Self { name, value }
    }

    /// Returns the property name.
    #[must_use]
    pub const fn name(&self) -> &PropertyName {
        &self.name
    }

    /// Returns the property value.
    #[must_use]
    pub const fn value(&self) -> &PropertyValue {
        &self.value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn property_name_uses_compact_storage() {
        let PropertyName(name) = PropertyName::new("opacity").expect("valid property");
        let _: &SmolStr = &name;
    }
}
