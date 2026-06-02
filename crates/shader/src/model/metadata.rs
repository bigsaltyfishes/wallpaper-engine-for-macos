use smol_str::SmolStr;

use super::{ComboName, TextureSlot};
use crate::{PropertyValue, ShaderError, ShaderResult};

/// Shader combo value.
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ShaderComboValue {
    /// Validated combo key.
    name: ComboName,
    /// Combo value as serialized by Wallpaper Engine material metadata.
    value: SmolStr,
}

impl ShaderComboValue {
    /// Creates a shader combo value.
    #[must_use]
    pub fn new(name: ComboName, value: impl Into<String>) -> Self {
        Self {
            name,
            value: SmolStr::new(value.into()),
        }
    }

    /// Returns the combo name.
    #[must_use]
    pub const fn name(&self) -> &ComboName {
        &self.name
    }

    /// Returns the combo value.
    #[must_use]
    pub fn value(&self) -> &str {
        self.value.as_str()
    }
}

/// Extracted material metadata for a shader stage.
#[derive(Clone, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ShaderMetadata {
    /// Combo values extracted from shader material metadata.
    combos: Box<[ShaderComboValue]>,
    /// Material property aliases extracted from metadata annotations.
    aliases: Box<[MaterialAlias]>,
    /// Default scalar and vector uniform values extracted from metadata.
    default_uniforms: Box<[DefaultUniformValue]>,
    /// Default texture paths extracted from metadata.
    default_textures: Box<[DefaultTextureValue]>,
    /// Texture slots proven active by reflection.
    active_texture_slots: Box<[TextureSlot]>,
}

impl ShaderMetadata {
    /// Creates shader metadata from validated components.
    #[must_use]
    pub fn new(
        combos: Box<[ShaderComboValue]>,
        aliases: Box<[MaterialAlias]>,
        default_uniforms: Box<[DefaultUniformValue]>,
        default_textures: Box<[DefaultTextureValue]>,
    ) -> Self {
        Self {
            combos,
            aliases,
            default_uniforms,
            default_textures,
            active_texture_slots: Box::from([]),
        }
    }

    /// Returns metadata with reflected active texture slots attached.
    #[must_use]
    pub fn with_active_texture_slots(mut self, active_texture_slots: Box<[TextureSlot]>) -> Self {
        self.active_texture_slots = active_texture_slots;
        self
    }

    /// Returns extracted combo values.
    #[must_use]
    pub fn combos(&self) -> &[ShaderComboValue] {
        &self.combos
    }

    /// Returns material-to-uniform aliases.
    #[must_use]
    pub fn aliases(&self) -> &[MaterialAlias] {
        &self.aliases
    }

    /// Returns default scalar/vector uniform values.
    #[must_use]
    pub fn default_uniforms(&self) -> &[DefaultUniformValue] {
        &self.default_uniforms
    }

    /// Returns default texture values.
    #[must_use]
    pub fn default_textures(&self) -> &[DefaultTextureValue] {
        &self.default_textures
    }

    /// Returns texture slots proven active by reflection.
    #[must_use]
    pub fn active_texture_slots(&self) -> &[TextureSlot] {
        &self.active_texture_slots
    }
}

/// Material property alias for a shader uniform.
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MaterialAlias {
    /// Material property name as written in metadata.
    material: SmolStr,
    /// Shader uniform name the material property maps to.
    uniform: SmolStr,
}

impl MaterialAlias {
    /// Creates a material alias.
    ///
    /// # Errors
    ///
    /// Returns an error when either name is empty or contains a NUL byte.
    pub fn new(material: impl Into<String>, uniform: impl Into<String>) -> ShaderResult<Self> {
        let material = material.into();
        let uniform = uniform.into();
        if material.is_empty() {
            return Err(ShaderError::invalid_request("material alias is empty"));
        }
        if material.contains('\0') {
            return Err(ShaderError::invalid_request(
                "material alias contains nul byte",
            ));
        }
        if uniform.is_empty() {
            return Err(ShaderError::invalid_request("uniform name is empty"));
        }
        if uniform.contains('\0') {
            return Err(ShaderError::invalid_request(
                "uniform name contains nul byte",
            ));
        }
        Ok(Self {
            material: SmolStr::new(material),
            uniform: SmolStr::new(uniform),
        })
    }

    /// Returns the material property name.
    #[must_use]
    pub fn material(&self) -> &str {
        self.material.as_str()
    }

    /// Returns the shader uniform name.
    #[must_use]
    pub fn uniform(&self) -> &str {
        self.uniform.as_str()
    }
}

/// Default value for a scalar or vector uniform.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DefaultUniformValue {
    /// Uniform that receives this default value.
    uniform: SmolStr,
    /// Non-null default value for the uniform.
    value: PropertyValue,
}

impl DefaultUniformValue {
    /// Creates a default uniform value.
    ///
    /// # Errors
    ///
    /// Returns an error when the uniform name is empty, contains a NUL byte, or
    /// the value is [`PropertyValue::None`].
    pub fn new(uniform: impl Into<String>, value: PropertyValue) -> ShaderResult<Self> {
        let uniform = uniform.into();
        if uniform.is_empty() {
            return Err(ShaderError::invalid_request("uniform name is empty"));
        }
        if uniform.contains('\0') {
            return Err(ShaderError::invalid_request(
                "uniform name contains nul byte",
            ));
        }
        if matches!(value, PropertyValue::None) {
            return Err(ShaderError::invalid_request(
                "default uniform value cannot be none",
            ));
        }
        Ok(Self {
            uniform: SmolStr::new(uniform),
            value,
        })
    }

    /// Returns the shader uniform name.
    #[must_use]
    pub fn uniform(&self) -> &str {
        self.uniform.as_str()
    }

    /// Returns the default value.
    #[must_use]
    pub const fn value(&self) -> &PropertyValue {
        &self.value
    }
}

/// Default texture value for a texture slot.
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DefaultTextureValue {
    /// Texture slot that receives this default path.
    slot: TextureSlot,
    /// Default texture path from material metadata.
    path: String,
}

impl DefaultTextureValue {
    /// Creates a default texture value.
    ///
    /// # Errors
    ///
    /// Returns an error when the path is empty or contains a NUL byte.
    pub fn new(slot: TextureSlot, path: impl Into<String>) -> ShaderResult<Self> {
        let path = path.into();
        if path.is_empty() {
            return Err(ShaderError::invalid_request(
                "default texture path is empty",
            ));
        }
        if path.contains('\0') {
            return Err(ShaderError::invalid_request(
                "default texture path contains nul byte",
            ));
        }
        Ok(Self { slot, path })
    }

    /// Returns the texture slot.
    #[must_use]
    pub const fn slot(&self) -> TextureSlot {
        self.slot
    }

    /// Returns the default texture path.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use smol_str::SmolStr;

    use super::*;

    fn accepts_smol_str(_value: &SmolStr) {}

    #[test]
    fn metadata_identifier_values_use_compact_storage() {
        let ShaderComboValue { value, .. } =
            ShaderComboValue::new(ComboName::new("QUALITY").expect("valid combo"), "2");
        accepts_smol_str(&value);

        let MaterialAlias { material, uniform } =
            MaterialAlias::new("brightness", "g_Brightness").expect("valid alias");
        accepts_smol_str(&material);
        accepts_smol_str(&uniform);

        let DefaultUniformValue { uniform, .. } =
            DefaultUniformValue::new("g_Exposure", PropertyValue::Number(1.0))
                .expect("valid default");
        accepts_smol_str(&uniform);
    }
}
