use smol_str::SmolStr;

use crate::{
    ComboName, DefaultTextureValue, DefaultUniformValue, MaterialAlias, ShaderComboValue,
    ShaderMetadata, ShaderResult, ShaderTextureInfo, TextureSlot,
    metadata::annotation_json::{AnnotationDefaultValue, ParsedAnnotation, TextureUniformName},
    syntax::{ShaderDeclaration, TopLevelQualifier},
};

/// Mutable metadata accumulator preserving first-seen combo order.
#[derive(Debug)]
pub(super) struct MetadataBuilder {
    /// Latest combo values keyed by first-seen normalized names.
    pub combos: Vec<MetadataComboEntry>,
    /// Material aliases discovered on uniforms.
    pub aliases: Vec<MaterialAlias>,
    /// Scalar/vector uniform defaults.
    pub default_uniforms: Vec<DefaultUniformValue>,
    /// Texture default paths.
    pub default_textures: Vec<DefaultTextureValue>,
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
                .push(MaterialAlias::new(material, uniform_name)?);
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
                .push(DefaultTextureValue::new(slot, *path)?);
        }

        let texture = textures
            .iter()
            .find(|info| info.slot() == slot && info.is_present());
        if let Some(combo) = annotation.combo() {
            let value = if texture.is_some() { "1" } else { "0" };
            self.set_combo(combo, value)?;
        }

        let Some(texture) = texture else {
            return Ok(());
        };

        if !texture.is_enabled() {
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
                    crate::PropertyValue::parse_metadata_default(source)?
                }
                AnnotationDefaultValue::Property(value) => value.clone(),
            };
            self.default_uniforms
                .push(DefaultUniformValue::new(uniform_name, value)?);
        }

        if let Some(combo) = annotation.combo() {
            self.set_combo(combo, "1")?;
        }

        Ok(())
    }

    /// Inserts or replaces a combo while preserving first-seen ordering.
    fn set_combo(&mut self, name: &str, value: &str) -> ShaderResult<()> {
        let combo = ShaderComboValue::new(ComboName::new(name)?, value);
        let key = combo.name().normalized_compact();
        if let Some(entry) = self.combos.iter_mut().find(|entry| entry.has_key(&key)) {
            entry.replace(combo);
        } else {
            self.combos.push(MetadataComboEntry { key, value: combo });
        }
        Ok(())
    }

    /// Converts accumulated fields into immutable metadata.
    pub(super) fn finish(self) -> ShaderMetadata {
        ShaderMetadata::new(
            self.combos.into_iter().map(|entry| entry.value).collect(),
            self.aliases.into_boxed_slice(),
            self.default_uniforms.into_boxed_slice(),
            self.default_textures.into_boxed_slice(),
        )
    }
}

/// First-seen metadata combo slot keyed by normalized combo name.
#[derive(Debug)]
pub(super) struct MetadataComboEntry {
    /// Normalized combo key.
    key: SmolStr,
    /// Current combo value for that key.
    value: ShaderComboValue,
}

impl MetadataComboEntry {
    /// Returns whether this entry uses the provided normalized key.
    fn has_key(&self, key: &str) -> bool {
        self.key.as_str() == key
    }

    /// Replaces the combo value without changing first-seen order.
    fn replace(&mut self, value: ShaderComboValue) {
        self.value = value;
    }
}
