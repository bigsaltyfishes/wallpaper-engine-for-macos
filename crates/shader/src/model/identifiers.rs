use std::fmt;

use crate::{ShaderError, ShaderResult, property::NonEmptyNoNulStrExt};

/// Validated shader program name.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(transparent)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct ShaderName(String);

impl ShaderName {
    /// Creates a validated shader name.
    ///
    /// # Errors
    ///
    /// Returns an error when the name is empty or contains a NUL byte.
    pub fn new(name: impl Into<String>) -> ShaderResult<Self> {
        let name = name.into().normalize_separators();
        name.as_str().validate_non_empty_no_nul("shader name")?;
        Ok(Self(name))
    }

    /// Returns the shader name as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ShaderName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for ShaderName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

/// Validated relative include path.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(transparent)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct IncludePath(String);

impl IncludePath {
    /// Creates a validated include path.
    ///
    /// # Errors
    ///
    /// Returns an error when the path is empty, absolute, contains `..`, or
    /// contains a NUL byte.
    pub fn new(path: impl Into<String>) -> ShaderResult<Self> {
        let path = path.into().normalize_separators();
        path.as_str().validate_non_empty_no_nul("include path")?;

        if path.starts_with("//") {
            return Err(ShaderError::invalid_request("include path is unc path"));
        }

        if path.starts_with('/') {
            return Err(ShaderError::invalid_request("include path is absolute"));
        }

        if path.as_str().has_windows_drive_prefix() {
            return Err(ShaderError::invalid_request(
                "include path has drive prefix",
            ));
        }

        if path.split('/').any(|component| component == "..") {
            return Err(ShaderError::invalid_request(
                "include path contains parent component",
            ));
        }

        Ok(Self(path))
    }

    /// Returns the include path as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for IncludePath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for IncludePath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

/// Validated shader combo name.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[repr(transparent)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct ComboName(String);

impl ComboName {
    /// Creates a validated combo name.
    ///
    /// # Errors
    ///
    /// Returns an error when the name is empty or is not an ASCII
    /// identifier-like name.
    pub fn new(name: impl Into<String>) -> ShaderResult<Self> {
        let name = name.into();
        name.as_str().validate_ascii_identifier("combo name")?;
        Ok(Self(name))
    }

    /// Returns the combo name as provided.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns the normalized combo name used for duplicate detection.
    #[must_use]
    pub fn normalized(&self) -> String {
        self.0.to_ascii_lowercase()
    }
}

impl fmt::Display for ComboName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for ComboName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

/// Validated shader symbol or reflection path segment name.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(transparent)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct ShaderSymbolName(String);

impl ShaderSymbolName {
    /// Creates a validated shader symbol name.
    ///
    /// # Errors
    ///
    /// Returns an error when the name is empty, contains a NUL byte, or has a
    /// non-identifier path segment.
    pub fn new(name: impl Into<String>) -> ShaderResult<Self> {
        let name = name.into();
        name.as_str()
            .validate_non_empty_no_nul("shader symbol name")?;

        for segment in name.split('.') {
            segment.validate_ascii_identifier("shader symbol name segment")?;
        }

        Ok(Self(name))
    }

    /// Returns the symbol name as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ShaderSymbolName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for ShaderSymbolName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

/// Vertex input/output location index.
///
/// Valid locations are `0..=31`, matching the conservative vertex interface
/// limit used by the current renderer path and Vulkan minimum guarantees.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(transparent)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct LocationIndex(u32);

impl LocationIndex {
    /// Maximum accepted location index.
    pub const MAX: u32 = 31;

    /// Creates a validated location index.
    ///
    /// # Errors
    ///
    /// Returns an error when the index is greater than 31.
    pub fn new(index: u32) -> ShaderResult<Self> {
        if index > Self::MAX {
            return Err(ShaderError::invalid_request(
                "location index is out of range",
            ));
        }

        Ok(Self(index))
    }

    /// Returns the location index.
    #[must_use]
    pub const fn index(self) -> u32 {
        self.0
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for LocationIndex {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = u32::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

/// Descriptor binding set.
///
/// Valid binding sets are `0..=3`. Task 1 only uses set 0, but reserving four
/// sets keeps the core model compatible with Vulkan's minimum descriptor set
/// limit without accepting unbounded renderer-facing values.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(transparent)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct BindingSet(u32);

impl BindingSet {
    /// Maximum accepted descriptor set.
    pub const MAX: u32 = 3;

    /// Creates a validated descriptor binding set.
    ///
    /// # Errors
    ///
    /// Returns an error when the set is greater than 3.
    pub fn new(set: u32) -> ShaderResult<Self> {
        if set > Self::MAX {
            return Err(ShaderError::invalid_request("binding set is out of range"));
        }

        Ok(Self(set))
    }

    /// Returns the descriptor binding set.
    #[must_use]
    pub const fn set(self) -> u32 {
        self.0
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for BindingSet {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = u32::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

/// Descriptor binding index.
///
/// Valid binding indices are `0..=31`, matching the conservative descriptor
/// indexing range used by the current renderer path.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(transparent)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct BindingIndex(u32);

impl BindingIndex {
    /// Maximum accepted descriptor binding index.
    pub const MAX: u32 = 31;

    /// Creates a validated descriptor binding index.
    ///
    /// # Errors
    ///
    /// Returns an error when the binding index is greater than 31.
    pub fn new(binding: u32) -> ShaderResult<Self> {
        if binding > Self::MAX {
            return Err(ShaderError::invalid_request(
                "binding index is out of range",
            ));
        }

        Ok(Self(binding))
    }

    /// Returns the descriptor binding index.
    #[must_use]
    pub const fn binding(self) -> u32 {
        self.0
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for BindingIndex {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = u32::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

/// String validation helpers shared by shader model newtypes.
trait ShaderModelStrExt {
    /// Validates that the string is a non-empty ASCII identifier-like token.
    fn validate_ascii_identifier(&self, label: &str) -> ShaderResult<()>;
    /// Returns whether the string starts with a Windows drive prefix.
    fn has_windows_drive_prefix(&self) -> bool;
}

impl ShaderModelStrExt for str {
    fn validate_ascii_identifier(&self, label: &str) -> ShaderResult<()> {
        self.validate_non_empty_no_nul(label)?;

        let mut chars = self.chars();
        let Some(first) = chars.next() else {
            return Err(ShaderError::invalid_request(format!("{label} is empty")));
        };

        if !(first == '_' || first.is_ascii_alphabetic()) {
            return Err(ShaderError::invalid_request(format!(
                "{label} is not an ascii identifier"
            )));
        }

        if chars.any(|character| !(character == '_' || character.is_ascii_alphanumeric())) {
            return Err(ShaderError::invalid_request(format!(
                "{label} is not an ascii identifier"
            )));
        }

        Ok(())
    }

    fn has_windows_drive_prefix(&self) -> bool {
        let bytes = self.as_bytes();
        bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
    }
}

/// Owned string helpers used before model validation.
trait ShaderModelStringExt {
    /// Normalizes Windows path separators to forward slashes.
    fn normalize_separators(self) -> Self;
}

impl ShaderModelStringExt for String {
    fn normalize_separators(self) -> Self {
        self.replace('\\', "/")
    }
}
