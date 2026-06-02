/// Shader stage kind.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ShaderStageKind {
    /// Vertex shader stage.
    Vertex,
    /// Fragment shader stage.
    Fragment,
}

/// Source text for one shader stage.
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ShaderStageSource {
    /// Stage this source text will be compiled for.
    kind: ShaderStageKind,
    /// Raw GLSL-like source text for the stage.
    source: String,
}

impl ShaderStageSource {
    /// Creates stage source from owned or borrowed source text.
    #[must_use]
    pub fn new(kind: ShaderStageKind, source: impl Into<String>) -> Self {
        Self {
            kind,
            source: source.into(),
        }
    }

    /// Returns the shader stage kind.
    #[must_use]
    pub const fn kind(&self) -> ShaderStageKind {
        self.kind
    }

    /// Returns the stage source text.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }
}

/// Requested shader compilation target.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ShaderTarget {
    /// Vulkan-compatible SPIR-V output.
    #[default]
    VulkanSpirv,
}

/// Shader cache behavior for a request.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ShaderCacheStrategy {
    /// Do not read or write shader cache entries.
    #[default]
    Disabled,
    /// Cache with the provided scene identifier.
    Enabled {
        /// Scene identifier used as part of cache key construction.
        scene_id: String,
    },
}

/// Compact shader cache behavior used after request validation.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum CompactShaderCacheStrategy {
    /// Do not read or write shader cache entries.
    #[default]
    Disabled,
    /// Cache with the compact scene identifier.
    Enabled {
        /// Scene identifier used as part of cache key construction.
        scene_id: smol_str::SmolStr,
    },
}

impl CompactShaderCacheStrategy {
    /// Returns the compact scene identifier when caching is enabled.
    #[cfg(test)]
    #[must_use]
    pub const fn scene_id(&self) -> Option<&smol_str::SmolStr> {
        match self {
            Self::Disabled => None,
            Self::Enabled { scene_id } => Some(scene_id),
        }
    }
}

impl From<&ShaderCacheStrategy> for CompactShaderCacheStrategy {
    fn from(strategy: &ShaderCacheStrategy) -> Self {
        match strategy {
            ShaderCacheStrategy::Disabled => Self::Disabled,
            ShaderCacheStrategy::Enabled { scene_id } => Self::Enabled {
                scene_id: scene_id.into(),
            },
        }
    }
}
