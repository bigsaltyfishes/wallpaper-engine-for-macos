use std::collections::BTreeSet;

use crate::{
    ComboName, DefaultTextureValue, DefaultUniformValue, MaterialAlias, ShaderComboValue,
    ShaderMetadata, ShaderProgramRequest, ShaderResult,
};

/// Merges stage metadata while preserving stage order.
#[derive(Debug, Default)]
pub(super) struct MetadataMerger {
    /// Merged combo values.
    combos: Vec<ShaderComboValue>,
    /// Merged aliases.
    aliases: Vec<MaterialAlias>,
    /// Merged default uniforms.
    default_uniforms: Vec<DefaultUniformValue>,
    /// Merged default textures.
    default_textures: Vec<DefaultTextureValue>,
}

impl MetadataMerger {
    /// Appends one stage metadata payload.
    pub(super) fn push(&mut self, metadata: &ShaderMetadata) {
        self.combos.extend_from_slice(metadata.combos());
        self.aliases.extend_from_slice(metadata.aliases());
        self.default_uniforms
            .extend_from_slice(metadata.default_uniforms());
        self.default_textures
            .extend_from_slice(metadata.default_textures());
    }

    /// Builds the merged metadata model.
    pub(super) fn finish(self) -> ShaderMetadata {
        ShaderMetadata::new(
            self.combos.into_boxed_slice(),
            self.aliases.into_boxed_slice(),
            self.default_uniforms.into_boxed_slice(),
            self.default_textures.into_boxed_slice(),
        )
    }
}

/// Builds a compile request with annotation combo defaults added only for
/// symbols the caller did not already provide.
#[derive(Debug)]
pub(super) struct RequestWithMetadataCombos<'request> {
    /// Original request.
    request: &'request ShaderProgramRequest,
    /// Normalized combo names already present in the request or added here.
    seen: BTreeSet<String>,
    /// Metadata combo defaults that are not overridden by request combos.
    defaults: Vec<ShaderComboValue>,
}

impl<'request> From<&'request ShaderProgramRequest> for RequestWithMetadataCombos<'request> {
    fn from(request: &'request ShaderProgramRequest) -> Self {
        Self {
            request,
            seen: request
                .combos()
                .iter()
                .map(|combo| combo.name().normalized())
                .collect(),
            defaults: Vec::new(),
        }
    }
}

impl RequestWithMetadataCombos<'_> {
    /// Adds a metadata combo default unless a request/default value already
    /// exists for the same normalized name.
    pub(super) fn push_default(&mut self, combo: &ShaderComboValue) -> ShaderResult<()> {
        let normalized = combo.name().normalized();
        if !self.seen.insert(normalized) {
            return Ok(());
        }

        self.defaults.push(ShaderComboValue::new(
            ComboName::new(combo.name().as_str())?,
            combo.value().to_owned(),
        ));
        Ok(())
    }

    /// Returns an owned request when defaults were added.
    pub(super) fn finish(self) -> ShaderResult<Option<ShaderProgramRequest>> {
        if self.defaults.is_empty() {
            return Ok(None);
        }

        let mut builder = ShaderProgramRequest::builder(self.request.shader_name().clone())
            .target(self.request.target())
            .cache_policy(self.request.cache_policy().clone());

        for stage in self.request.stages() {
            builder = builder.stage(stage.clone());
        }
        for combo in &self.defaults {
            builder = builder.combo(combo.clone());
        }
        for combo in self.request.combos() {
            builder = builder.combo(combo.clone());
        }
        for texture in self.request.textures() {
            builder = builder.texture(texture.clone());
        }
        for property in self.request.properties() {
            builder = builder.property(property.clone());
        }

        builder.build().map(Some)
    }
}
