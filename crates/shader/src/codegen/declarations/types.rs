//! Legacy declaration type names and shape classification.

use crate::{ShaderError, ShaderResult};

/// Legacy Wallpaper Engine/HLSL and backend GLSL type spelling.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LegacyTypeName<'src> {
    /// Source type spelling.
    source: &'src str,
}

impl<'src> LegacyTypeName<'src> {
    /// Wraps a source type spelling for type classification.
    #[must_use]
    pub(crate) const fn new(source: &'src str) -> Self {
        Self { source }
    }

    /// Returns the source type spelling.
    #[must_use]
    pub(crate) const fn as_str(self) -> &'src str {
        self.source
    }

    /// Returns the GLSL type spelling.
    #[must_use]
    pub(crate) const fn glsl(self) -> &'src str {
        match self.source.as_bytes() {
            b"float2" => "vec2",
            b"float1" => "float",
            b"float3" => "vec3",
            b"float4" => "vec4",
            _ => self.source,
        }
    }

    /// Returns the scalar/vector component width represented by this type.
    #[must_use]
    pub(crate) const fn vector_width(self) -> Option<u8> {
        match self.glsl().as_bytes() {
            b"float" => Some(1),
            b"vec2" => Some(2),
            b"vec3" => Some(3),
            b"vec4" => Some(4),
            _ => None,
        }
    }

    /// Returns whether this type is one covered by the C++ strategy.
    #[must_use]
    pub(crate) const fn is_builtin(self) -> bool {
        matches!(
            self.source.as_bytes(),
            b"bool"
                | b"int"
                | b"uint"
                | b"float"
                | b"float1"
                | b"float2"
                | b"float3"
                | b"float4"
                | b"vec2"
                | b"vec3"
                | b"vec4"
                | b"ivec2"
                | b"ivec3"
                | b"ivec4"
                | b"uvec2"
                | b"uvec3"
                | b"uvec4"
                | b"bvec2"
                | b"bvec3"
                | b"bvec4"
                | b"mat2"
                | b"mat3"
                | b"mat4"
                | b"mat2x2"
                | b"mat2x3"
                | b"mat2x4"
                | b"mat3x2"
                | b"mat3x3"
                | b"mat3x4"
                | b"mat4x2"
                | b"mat4x3"
                | b"mat4x4"
        )
    }

    /// Returns whether this type is accepted for local declaration scanning.
    #[must_use]
    pub(crate) const fn is_local(self) -> bool {
        self.is_builtin()
    }

    /// Returns a supported generated-uniform type or rejects user-defined and
    /// opaque types before emission into stages that cannot see declarations.
    pub(crate) fn validate_generated_uniform(self) -> ShaderResult<Self> {
        if self.is_builtin() {
            Ok(self)
        } else {
            Err(ShaderError::invalid_request(format!(
                "unsupported GlobalUniforms member type `{}`",
                self.source
            )))
        }
    }
}
