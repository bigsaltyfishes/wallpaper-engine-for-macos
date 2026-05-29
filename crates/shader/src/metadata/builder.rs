use std::collections::BTreeMap;

use crate::{
    ComboName, DefaultTextureValue, DefaultUniformValue, MaterialAlias, ShaderComboValue,
    ShaderMetadata, ShaderResult, ShaderTextureInfo, TextureSlot,
    metadata::annotation_json::{AnnotationDefaultValue, ParsedAnnotation, TextureUniformName},
    syntax::{ShaderDeclaration, TopLevelQualifier},
};

/// Mutable metadata accumulator preserving first-seen combo order.
#[derive(Debug)]
pub(super) struct MetadataBuilder {
    /// Normalized combo names in first-seen order.
    pub(super) combo_order: Vec<String>,
    /// Latest combo values keyed by normalized name.
    pub(super) combos: BTreeMap<String, ShaderComboValue>,
    /// Material aliases discovered on uniforms.
    pub(super) aliases: Vec<MaterialAlias>,
    /// Scalar/vector uniform defaults.
    pub(super) default_uniforms: Vec<DefaultUniformValue>,
    /// Texture default paths.
    pub(super) default_textures: Vec<DefaultTextureValue>,
}

impl MetadataBuilder {
    /// Handles a standalone `[COMBO]` annotation.
    pub(super) fn handle_combo_annotation(&mut self, text: &str) -> ShaderResult<()> {
        let Some(annotation) = ParsedAnnotation::from_annotation_text(text)? else {
            return Ok(());
        };
        let Some(name) = annotation.combo() else {
            return Ok(());
        };
        self.set_combo(name, annotation.combo_default_value().unwrap_or("0"))
    }

    /// Handles a JSON annotation attached to a uniform declaration.
    pub(super) fn handle_uniform_annotation(
        &mut self,
        declaration: &ShaderDeclaration<'_>,
        text: &str,
        textures: &[ShaderTextureInfo],
    ) -> ShaderResult<()> {
        if declaration.qualifier() != Some(TopLevelQualifier::Uniform) {
            return Ok(());
        }
        let Some(uniform_name) = declaration.name() else {
            return Ok(());
        };
        let Some(annotation) = ParsedAnnotation::from_annotation_text(text)? else {
            return Ok(());
        };

        if let Some(material) = annotation.material() {
            self.aliases
                .push(MaterialAlias::new(material, uniform_name.to_owned())?);
        }

        if let Some(slot) = (TextureUniformName {
            source: uniform_name,
        })
        .slot()?
        {
            self.handle_texture_uniform(slot, &annotation, textures)
        } else {
            self.handle_scalar_uniform(uniform_name, &annotation)
        }
    }

    /// Handles defaults and combos for a texture uniform annotation.
    fn handle_texture_uniform(
        &mut self,
        slot: TextureSlot,
        annotation: &ParsedAnnotation<'_>,
        textures: &[ShaderTextureInfo],
    ) -> ShaderResult<()> {
        if let Some(AnnotationDefaultValue::String(path)) = annotation.default() {
            self.default_textures
                .push(DefaultTextureValue::new(slot, (*path).to_owned())?);
        }

        let texture = textures
            .iter()
            .find(|info| info.slot() == slot && TextureMetadataState::from(*info).is_present());
        if let Some(combo) = annotation.combo() {
            let value = if texture.is_some() { "1" } else { "0" };
            self.set_combo(combo, value)?;
        }

        let Some(texture) = texture else {
            return Ok(());
        };

        let texture_state = TextureMetadataState::from(texture);
        if !texture_state.components_are_enabled() {
            return Ok(());
        }

        for (component, combo) in texture
            .components()
            .iter()
            .zip(annotation.component_combos())
        {
            if !component.is_enabled() {
                continue;
            }
            if let Some(combo) = combo {
                self.set_combo(combo, "1")?;
            }
        }

        Ok(())
    }

    /// Handles defaults and combos for a non-texture uniform annotation.
    fn handle_scalar_uniform(
        &mut self,
        uniform_name: &str,
        annotation: &ParsedAnnotation<'_>,
    ) -> ShaderResult<()> {
        if let Some(default) = annotation.default() {
            let value = match default {
                AnnotationDefaultValue::String(source) => {
                    crate::PropertyValue::try_from(ParsedScalarDefault { source })?
                }
                AnnotationDefaultValue::Property(value) => value.clone(),
            };
            self.default_uniforms
                .push(DefaultUniformValue::new(uniform_name.to_owned(), value)?);
        }

        if let Some(combo) = annotation.combo() {
            self.set_combo(combo, "1")?;
        }

        Ok(())
    }

    /// Inserts or replaces a combo while preserving first-seen ordering.
    fn set_combo(&mut self, name: &str, value: &str) -> ShaderResult<()> {
        let combo = ShaderComboValue::new(ComboName::new(name.to_owned())?, value.to_owned());
        let key = combo.name().normalized();
        if let std::collections::btree_map::Entry::Vacant(entry) = self.combos.entry(key.clone()) {
            self.combo_order.push(key);
            let _old = entry.insert(combo);
        } else if let Some(existing) = self.combos.get_mut(&key) {
            *existing = combo;
        }
        Ok(())
    }

    /// Converts accumulated fields into immutable metadata.
    pub(super) fn finish(mut self) -> ShaderMetadata {
        let mut combos = Vec::with_capacity(self.combo_order.len());
        for key in self.combo_order {
            if let Some(combo) = self.combos.remove(&key) {
                combos.push(combo);
            }
        }

        ShaderMetadata::new(
            combos.into_boxed_slice(),
            self.aliases.into_boxed_slice(),
            self.default_uniforms.into_boxed_slice(),
            self.default_textures.into_boxed_slice(),
        )
    }
}

/// String scalar default annotation that may contain scalar/vector text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ParsedScalarDefault<'src> {
    /// Decoded default string content.
    source: &'src str,
}

impl TryFrom<ParsedScalarDefault<'_>> for crate::PropertyValue {
    type Error = crate::ShaderError;

    fn try_from(default: ParsedScalarDefault<'_>) -> Result<Self, Self::Error> {
        let mut values = Vec::new();
        for part in default
            .source
            .split(|character: char| character.is_ascii_whitespace() || character == ',')
        {
            if part.is_empty() {
                continue;
            }
            values.push(
                part.parse::<f32>().map_err(|_| {
                    crate::ShaderError::invalid_request("metadata number is invalid")
                })?,
            );
        }

        match values.as_slice() {
            [one] => Ok(crate::PropertyValue::Number(*one)),
            [x, y, z] => Ok(crate::PropertyValue::Vec3([*x, *y, *z])),
            _ => Ok(crate::PropertyValue::String(default.source.to_owned())),
        }
    }
}

/// Material texture state used by metadata combo extraction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TextureMetadataState<'texture> {
    /// Texture metadata supplied by the renderer.
    texture: &'texture ShaderTextureInfo,
}

impl<'texture> From<&'texture ShaderTextureInfo> for TextureMetadataState<'texture> {
    fn from(texture: &'texture ShaderTextureInfo) -> Self {
        Self { texture }
    }
}

impl TextureMetadataState<'_> {
    /// Returns whether this slot represents a material texture rather than an
    /// empty placeholder inserted only to preserve slot indices.
    const fn is_present(self) -> bool {
        self.texture.is_present()
    }

    /// Returns whether component-level combos should be emitted.
    const fn components_are_enabled(self) -> bool {
        self.texture.is_enabled()
    }
}
