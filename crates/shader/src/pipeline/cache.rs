use std::fmt::Write as _;

use crate::{
    ShaderCacheKey, ShaderProgramRequest, ShaderStageKind, ShaderTextureInfo,
    legalize::LegalizedStageSource,
    pipeline::revision::{COMPILER_OPTIONS_CACHE_SALT, ShaderPipelineRevision},
    preprocess::PreprocessedStage,
};

/// Stable cache-key builder.
#[derive(Debug)]
pub(super) struct CacheKeyBuilder {
    /// Deterministic FNV-1a 64-bit digest state.
    digest: StableDigest,
}

impl From<CacheKeySeed<'_>> for CacheKeyBuilder {
    fn from(seed: CacheKeySeed<'_>) -> Self {
        let mut builder = Self {
            digest: StableDigest::default(),
        };
        builder.push("shader-pipeline-cache-v1");
        builder.push_u64(seed.revision.value());
        builder.push(seed.request.shader_name().as_str());
        builder.push(match seed.request.target() {
            crate::ShaderTarget::VulkanSpirv => "vulkan_spirv",
        });
        builder.push_cache_policy(seed.request.cache_policy());

        for combo in seed.request.combos() {
            builder.push("combo");
            builder.push(combo.name().as_str());
            builder.push(combo.value());
        }
        for texture in seed.request.textures() {
            builder.push_texture(texture);
        }
        for property in seed.request.properties() {
            builder.push("property");
            builder.push(property.name().as_str());
            builder.push_property_value(property.value());
        }

        builder
    }
}

impl CacheKeyBuilder {
    /// Adds one preprocessed/legalized stage to the key.
    pub(super) fn push_stage(
        &mut self,
        stage: &PreprocessedStage,
        legalized: &LegalizedStageSource,
    ) {
        self.push("stage");
        self.push(match stage.kind() {
            ShaderStageKind::Vertex => "vertex",
            ShaderStageKind::Fragment => "fragment",
        });
        self.push(stage.source());
        self.push(legalized.source());
        self.push(COMPILER_OPTIONS_CACHE_SALT);
    }

    /// Adds cache policy data.
    fn push_cache_policy(&mut self, policy: &crate::ShaderCachePolicy) {
        match policy {
            crate::ShaderCachePolicy::Disabled => self.push("cache-disabled"),
            crate::ShaderCachePolicy::Enabled { scene_id } => {
                self.push("cache-enabled");
                self.push(scene_id);
            }
        }
    }

    /// Adds texture metadata.
    fn push_texture(&mut self, texture: &ShaderTextureInfo) {
        self.push("texture");
        self.push_u64(u64::from(texture.slot().index()));
        self.push(if texture.is_present() {
            "present"
        } else {
            "absent"
        });
        self.push(if texture.is_enabled() {
            "enabled"
        } else {
            "disabled"
        });
        self.push(match texture.format() {
            crate::TextureFormatHint::Unknown => "unknown",
            crate::TextureFormatHint::R8 => "r8",
            crate::TextureFormatHint::Rg8 => "rg8",
            crate::TextureFormatHint::Rgba8 => "rgba8",
        });
        for component in texture.components() {
            self.push(if component.is_enabled() { "1" } else { "0" });
        }
    }

    /// Adds one project property value.
    fn push_property_value(&mut self, value: &crate::PropertyValue) {
        match value {
            crate::PropertyValue::String(value) => {
                self.push("string");
                self.push(value);
            }
            crate::PropertyValue::Number(value) => {
                self.push("number");
                self.push(&value.to_bits().to_string());
            }
            crate::PropertyValue::Bool(value) => self.push(if *value { "true" } else { "false" }),
            crate::PropertyValue::Vec3(value) => {
                self.push("vec3");
                for component in value {
                    self.push(&component.to_bits().to_string());
                }
            }
            crate::PropertyValue::None => self.push("none"),
        }
    }

    /// Adds a string with a length delimiter.
    fn push(&mut self, value: &str) {
        self.digest.push_usize(value.len());
        self.digest.push_bytes(value.as_bytes());
    }

    /// Adds an integer.
    fn push_u64(&mut self, value: u64) {
        self.digest.push_bytes(&value.to_le_bytes());
    }

    /// Finishes the cache key.
    pub(super) fn finish(self) -> ShaderCacheKey {
        let mut value = String::with_capacity(16);
        let _result = write!(&mut value, "{:016x}", self.digest.finish());
        ShaderCacheKey::new(value)
    }
}

/// Request data used to seed a cache key.
#[derive(Clone, Copy, Debug)]
pub(super) struct CacheKeySeed<'request> {
    /// Shader pipeline revision.
    pub(super) revision: ShaderPipelineRevision,
    /// Request whose stable fields seed the cache key.
    pub(super) request: &'request ShaderProgramRequest,
}

/// Deterministic FNV-1a 64-bit digest used for generated cache keys.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct StableDigest {
    /// Current digest state.
    state: u64,
}

impl Default for StableDigest {
    fn default() -> Self {
        Self {
            state: 0xcbf2_9ce4_8422_2325,
        }
    }
}

impl StableDigest {
    /// Adds bytes to the digest.
    fn push_bytes(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.state ^= u64::from(*byte);
            self.state = self.state.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    /// Adds a platform-size delimiter to the digest in a stable width.
    fn push_usize(&mut self, value: usize) {
        self.push_bytes(&(value as u64).to_le_bytes());
    }

    /// Returns the final digest value.
    const fn finish(self) -> u64 {
        self.state
    }
}
