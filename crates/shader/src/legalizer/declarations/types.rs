//! Legacy declaration type names.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Legacy Wallpaper Engine/HLSL type spelling.
pub(super) struct LegacyTypeName<'src> {
    /// Source type spelling.
    source: &'src str,
}

impl<'src> LegacyTypeName<'src> {
    /// Wraps a source type spelling for GLSL conversion.
    pub(super) const fn new(source: &'src str) -> Self {
        Self { source }
    }

    /// Returns the GLSL type spelling.
    pub(super) const fn glsl(self) -> &'src str {
        match self.source.as_bytes() {
            b"float2" => "vec2",
            b"float1" => "float",
            b"float3" => "vec3",
            b"float4" => "vec4",
            _ => self.source,
        }
    }
}
