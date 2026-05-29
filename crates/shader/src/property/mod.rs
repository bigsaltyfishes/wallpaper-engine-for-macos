//! Project property values used by shader requests.

use std::fmt;

use crate::{ShaderError, ShaderResult};

/// Name of a project property visible to shader material bindings.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(transparent)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct PropertyName(String);

impl PropertyName {
    /// Creates a validated property name.
    ///
    /// # Errors
    ///
    /// Returns an error when the name is empty or contains a NUL byte.
    pub fn new(name: impl Into<String>) -> ShaderResult<Self> {
        let name = name.into();
        name.as_str().validate_non_empty_no_nul("property name")?;
        Ok(Self(name))
    }

    /// Returns the property name as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
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
    /// Three-component numeric vector.
    Vec3([f32; 3]),
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

/// Shared validation for non-empty strings that must not contain NUL bytes.
pub(crate) trait NonEmptyNoNulStrExt {
    /// Validates a labeled string for model constructors.
    fn validate_non_empty_no_nul(&self, label: &str) -> ShaderResult<()>;
}

impl NonEmptyNoNulStrExt for str {
    fn validate_non_empty_no_nul(&self, label: &str) -> ShaderResult<()> {
        if self.is_empty() {
            return Err(ShaderError::invalid_request(format!("{label} is empty")));
        }

        if self.contains('\0') {
            return Err(ShaderError::invalid_request(format!(
                "{label} contains nul byte"
            )));
        }

        Ok(())
    }
}
