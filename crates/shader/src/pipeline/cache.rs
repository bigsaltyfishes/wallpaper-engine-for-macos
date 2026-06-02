use std::fmt::Write as _;

use crate::{
    ShaderCacheKey, ShaderStageKind, ShaderTextureInfo, legalize::CodegenStageSource,
    pipeline::revision::COMPILER_OPTIONS_CACHE_SALT, preprocess::PreprocessedStage,
};

/// Stable cache-key builder.
#[derive(Debug, Default)]
pub(super) struct CacheKeyBuilder {
    /// Deterministic FNV-1a 64-bit digest state.
    digest: StableDigest,
}

impl CacheKeyBuilder {
    /// Adds one preprocessed/legalized stage to the key.
    pub(super) fn push_stage(&mut self, stage: &PreprocessedStage, legalized: &CodegenStageSource) {
        self.push("stage");
        self.push(match stage.kind() {
            ShaderStageKind::Vertex => "vertex",
            ShaderStageKind::Fragment => "fragment",
        });
        self.push(stage.source());
        self.push(legalized.source());
        self.push(COMPILER_OPTIONS_CACHE_SALT);
    }

    /// Adds cache strategy data.
    pub(super) fn push_cache_strategy(
        &mut self,
        strategy: &crate::model::CompactShaderCacheStrategy,
    ) {
        match strategy {
            crate::model::CompactShaderCacheStrategy::Disabled => self.push("cache-disabled"),
            crate::model::CompactShaderCacheStrategy::Enabled { scene_id } => {
                self.push("cache-enabled");
                self.push(scene_id);
            }
        }
    }

    /// Adds texture metadata.
    pub(super) fn push_texture(&mut self, texture: &ShaderTextureInfo) {
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
    pub(super) fn push_property_value(&mut self, value: &crate::PropertyValue) {
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
            crate::PropertyValue::Vec2(value) => {
                self.push("vec2");
                for component in value {
                    self.push(&component.to_bits().to_string());
                }
            }
            crate::PropertyValue::Vec3(value) => {
                self.push("vec3");
                for component in value {
                    self.push(&component.to_bits().to_string());
                }
            }
            crate::PropertyValue::Vec4(value) => {
                self.push("vec4");
                for component in value {
                    self.push(&component.to_bits().to_string());
                }
            }
            crate::PropertyValue::Matrix4(value) => {
                self.push("matrix4");
                for component in value {
                    self.push(&component.to_bits().to_string());
                }
            }
            crate::PropertyValue::None => self.push("none"),
        }
    }

    /// Adds a string with a length delimiter.
    pub(super) fn push(&mut self, value: &str) {
        self.digest.push_usize(value.len());
        self.digest.push_bytes(value.as_bytes());
    }

    /// Adds an integer.
    pub(super) fn push_u64(&mut self, value: u64) {
        self.digest.push_bytes(&value.to_le_bytes());
    }

    /// Finishes the cache key.
    pub(super) fn finish(self) -> ShaderCacheKey {
        let mut value = String::with_capacity(16);
        let _result = write!(&mut value, "{:016x}", self.digest.finish());
        ShaderCacheKey::new(value)
    }
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
