use smol_str::SmolStr;

use super::{
    CompactShaderCacheStrategy, ShaderCacheStrategy, ShaderComboValue, ShaderName, ShaderStageKind,
    ShaderStageSource, ShaderTarget, ShaderTextureInfo,
};
use crate::{ProjectPropertyBinding, PropertyValue, ShaderError, ShaderResult};

/// Shader program compilation request.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct ShaderProgramRequest {
    /// Program name used for diagnostics and cache keys.
    shader_name: ShaderName,
    /// Stage sources that must include one vertex and one fragment stage.
    stages: Box<[ShaderStageSource]>,
    /// Validated combo values in request order.
    combos: Box<[ShaderComboValue]>,
    /// Texture slots available to the request.
    textures: Box<[ShaderTextureInfo]>,
    /// Project property bindings available to material uniforms.
    properties: Box<[ProjectPropertyBinding]>,
    /// Backend target requested by the caller.
    target: ShaderTarget,
    /// Cache behavior requested for this program.
    cache_strategy: ShaderCacheStrategy,
    /// Compact cache behavior used by internal cache-key construction.
    #[cfg_attr(feature = "serde", serde(skip))]
    compact_cache_strategy: CompactShaderCacheStrategy,
}

impl ShaderProgramRequest {
    /// Starts building a shader program request.
    #[must_use = "call build() on the returned builder to create a shader program request"]
    pub fn builder(shader_name: ShaderName) -> ShaderProgramRequestBuilder {
        ShaderProgramRequestBuilder {
            shader_name,
            stages: Vec::with_capacity(2),
            combos: Vec::new(),
            textures: Vec::new(),
            properties: Vec::new(),
            target: ShaderTarget::default(),
            cache_strategy: ShaderCacheStrategy::default(),
        }
    }

    /// Returns the shader name.
    #[must_use]
    pub const fn shader_name(&self) -> &ShaderName {
        &self.shader_name
    }

    /// Returns the stage sources.
    #[must_use]
    pub fn stages(&self) -> &[ShaderStageSource] {
        &self.stages
    }

    /// Returns the combo values.
    #[must_use]
    pub fn combos(&self) -> &[ShaderComboValue] {
        &self.combos
    }

    /// Returns the texture infos.
    #[must_use]
    pub fn textures(&self) -> &[ShaderTextureInfo] {
        &self.textures
    }

    /// Returns the project property bindings.
    #[must_use]
    pub fn properties(&self) -> &[ProjectPropertyBinding] {
        &self.properties
    }

    /// Returns the shader target.
    #[must_use]
    pub const fn target(&self) -> ShaderTarget {
        self.target
    }

    /// Returns the cache strategy.
    #[must_use]
    pub const fn cache_strategy(&self) -> &ShaderCacheStrategy {
        &self.cache_strategy
    }

    /// Returns the compact internal cache strategy.
    #[must_use]
    pub const fn compact_cache_strategy(&self) -> &CompactShaderCacheStrategy {
        &self.compact_cache_strategy
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for ShaderProgramRequest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(serde::Deserialize)]
        #[serde(rename = "ShaderProgramRequest")]
        struct ShaderProgramRequestFields {
            shader_name: ShaderName,
            stages: Vec<ShaderStageSource>,
            combos: Vec<ShaderComboValue>,
            textures: Vec<ShaderTextureInfo>,
            properties: Vec<ProjectPropertyBinding>,
            target: ShaderTarget,
            cache_strategy: ShaderCacheStrategy,
        }

        let dto = ShaderProgramRequestFields::deserialize(deserializer)?;
        let mut builder = ShaderProgramRequest::builder(dto.shader_name)
            .target(dto.target)
            .cache_strategy(dto.cache_strategy);

        for stage in dto.stages {
            builder = builder.stage(stage);
        }
        for combo in dto.combos {
            builder = builder.combo(combo);
        }
        for texture in dto.textures {
            builder = builder.texture(texture);
        }
        for property in dto.properties {
            builder = builder.property(property);
        }

        builder.build().map_err(serde::de::Error::custom)
    }
}

/// Builder for [`ShaderProgramRequest`].
#[derive(Clone, Debug)]
#[must_use = "call build() to create a shader program request"]
pub struct ShaderProgramRequestBuilder {
    /// Program name carried into the final request.
    shader_name: ShaderName,
    /// Stage sources accumulated by the builder.
    stages: Vec<ShaderStageSource>,
    /// Combo values plus duplicate handling mode accumulated by the builder.
    combos: Vec<PendingCombo>,
    /// Texture metadata accumulated by the builder.
    textures: Vec<ShaderTextureInfo>,
    /// Project property bindings accumulated by the builder.
    properties: Vec<ProjectPropertyBinding>,
    /// Selected shader target.
    target: ShaderTarget,
    /// Selected cache behavior.
    cache_strategy: ShaderCacheStrategy,
}

impl ShaderProgramRequestBuilder {
    /// Adds a shader stage source.
    #[must_use = "builder methods return an updated builder"]
    pub fn stage(mut self, stage: ShaderStageSource) -> Self {
        self.stages.push(stage);
        self
    }

    /// Adds a combo and rejects duplicate names during build.
    #[must_use = "builder methods return an updated builder"]
    pub fn combo(mut self, combo: ShaderComboValue) -> Self {
        self.combos.push(PendingCombo {
            value: combo,
            replace: false,
        });
        self
    }

    /// Adds or replaces a combo by normalized name.
    #[must_use = "builder methods return an updated builder"]
    pub fn replace_combo(mut self, combo: ShaderComboValue) -> Self {
        self.combos.push(PendingCombo {
            value: combo,
            replace: true,
        });
        self
    }

    /// Adds texture information.
    #[must_use = "builder methods return an updated builder"]
    pub fn texture(mut self, texture: ShaderTextureInfo) -> Self {
        self.textures.push(texture);
        self
    }

    /// Adds a project property binding.
    #[must_use = "builder methods return an updated builder"]
    pub fn property(mut self, property: ProjectPropertyBinding) -> Self {
        self.properties.push(property);
        self
    }

    /// Sets the shader target.
    #[must_use = "builder methods return an updated builder"]
    pub const fn target(mut self, target: ShaderTarget) -> Self {
        self.target = target;
        self
    }

    /// Sets the shader cache strategy.
    #[must_use = "builder methods return an updated builder"]
    pub fn cache_strategy(mut self, cache_strategy: ShaderCacheStrategy) -> Self {
        self.cache_strategy = cache_strategy;
        self
    }

    /// Builds and validates the shader program request.
    ///
    /// # Errors
    ///
    /// Returns an error for missing stages, duplicate stage kinds, duplicate
    /// texture slots, duplicate combo names not added through
    /// `replace_combo`, unsupported targets, or `PropertyValue::None`
    /// bindings.
    pub fn build(self) -> ShaderResult<ShaderProgramRequest> {
        let mut has_vertex = false;
        let mut has_fragment = false;

        for stage in &self.stages {
            match stage.kind() {
                ShaderStageKind::Vertex => {
                    if has_vertex {
                        return Err(ShaderError::invalid_request("duplicate vertex stage"));
                    }
                    has_vertex = true;
                }
                ShaderStageKind::Fragment => {
                    if has_fragment {
                        return Err(ShaderError::invalid_request("duplicate fragment stage"));
                    }
                    has_fragment = true;
                }
            }
        }

        match (has_vertex, has_fragment) {
            (true, true) => {}
            (false, false) => {
                return Err(ShaderError::invalid_request(
                    "shader request must include vertex and fragment stages",
                ));
            }
            (false, true) => {
                return Err(ShaderError::invalid_request(
                    "shader request missing vertex stage",
                ));
            }
            (true, false) => {
                return Err(ShaderError::invalid_request(
                    "shader request missing fragment stage",
                ));
            }
        }

        let mut slots = Vec::with_capacity(self.textures.len());

        for texture in &self.textures {
            if slots.contains(&texture.slot()) {
                return Err(ShaderError::invalid_request(format!(
                    "duplicate texture slot {}",
                    texture.slot().index()
                )));
            }
            slots.push(texture.slot());
        }

        for property in &self.properties {
            if matches!(property.value(), PropertyValue::None) {
                return Err(ShaderError::invalid_request(format!(
                    "property {} has no value",
                    property.name()
                )));
            }
        }

        let mut values = Vec::<RequestComboEntry>::with_capacity(self.combos.len());

        for combo in self.combos {
            let key = combo.value.name().normalized_compact();
            let existing = values.iter_mut().find(|entry| entry.has_key(&key));

            if existing.is_some() && !combo.replace {
                return Err(ShaderError::invalid_request(format!(
                    "duplicate combo name {key}"
                )));
            }

            if let Some(existing) = existing {
                existing.replace(combo.value);
            } else {
                values.push(RequestComboEntry {
                    key,
                    value: combo.value,
                });
            }
        }

        let compact_cache_strategy = CompactShaderCacheStrategy::from(&self.cache_strategy);

        Ok(ShaderProgramRequest {
            shader_name: self.shader_name,
            stages: self.stages.into_boxed_slice(),
            combos: values.into_iter().map(|entry| entry.value).collect(),
            textures: self.textures.into_boxed_slice(),
            properties: self.properties.into_boxed_slice(),
            target: self.target,
            cache_strategy: self.cache_strategy,
            compact_cache_strategy,
        })
    }
}

/// Combo value queued by the builder with its duplicate handling strategy.
#[derive(Clone, Debug)]
struct PendingCombo {
    /// Combo payload supplied by the caller.
    value: ShaderComboValue,
    /// Whether this value replaces a previously queued value with the same
    /// normalized name.
    replace: bool,
}

/// First-seen request combo slot keyed by normalized combo name.
#[derive(Clone, Debug)]
struct RequestComboEntry {
    /// Normalized combo key.
    key: SmolStr,
    /// Current combo value for that key.
    value: ShaderComboValue,
}

impl RequestComboEntry {
    /// Returns whether this entry uses the provided normalized key.
    fn has_key(&self, key: &str) -> bool {
        self.key.as_str() == key
    }

    /// Replaces the combo value without changing first-seen order.
    fn replace(&mut self, value: ShaderComboValue) {
        self.value = value;
    }
}

#[cfg(test)]
mod tests {
    use smol_str::SmolStr;

    use super::*;

    #[test]
    fn build_converts_public_cache_strategy_to_compact_internal_strategy() {
        let request = ShaderProgramRequest::builder(
            ShaderName::new("effects/genericimage").expect("valid shader name"),
        )
        .stage(ShaderStageSource::new(
            ShaderStageKind::Vertex,
            "void main() {}",
        ))
        .stage(ShaderStageSource::new(
            ShaderStageKind::Fragment,
            "void main() {}",
        ))
        .cache_strategy(ShaderCacheStrategy::Enabled {
            scene_id: "3611439897".to_owned(),
        })
        .build()
        .expect("request should be valid");

        let scene_id: SmolStr = request
            .compact_cache_strategy()
            .scene_id()
            .expect("cache should be enabled")
            .clone();

        assert_eq!(scene_id.as_str(), "3611439897");
    }
}
