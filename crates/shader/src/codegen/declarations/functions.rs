//! Compatibility helper requests and function declaration facts.

use std::fmt::Write as _;

use super::super::emission::SourceEmitter;
use crate::{ShaderResult, SourceSpan};

/// Compatibility helper functions requested during codegen.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct CompatibilityFunctionRequests {
    /// Whether generated `clip` overloads are needed.
    clip: bool,
    /// Whether generated `PerformLighting_V1` overloads are needed.
    perform_lighting: bool,
}

impl CompatibilityFunctionRequests {
    /// Requests generated `clip` overloads.
    pub(crate) fn require_clip(&mut self) {
        self.clip = true;
    }

    /// Requests generated `PerformLighting_V1` overloads.
    pub(crate) fn require_perform_lighting(&mut self) {
        self.perform_lighting = true;
    }

    /// Emits requested compatibility helper functions.
    pub(crate) fn emit(self, output: &mut String) -> ShaderResult<()> {
        if self.perform_lighting {
            writeln!(
                output,
                "vec3 PerformLighting_V1(vec3 world_pos, vec3 albedo, vec3 normal, vec3 \
                 view_vector,\nvec3 specular_tint, vec3 f0, float roughness, float metallic) \
                 {{\nreturn albedo * max(dot(normalize(normal), normalize(view_vector)), \
                 0.0);\n}}\nvec3 PerformLighting_V1(vec3 world_pos, vec3 albedo, vec3 normal, \
                 vec3 view_vector,\nvec3 specular_tint, vec3 f0, float roughness, float metallic, \
                 float ao) {{\nreturn albedo * ao * max(dot(normalize(normal), \
                 normalize(view_vector)), 0.0);\n}}"
            )
            .map_err(SourceEmitter::write_error)?;
        }

        if self.clip {
            writeln!(
                output,
                "void clip(float value) {{ if (value < 0.0) discard; }}\nvoid clip(vec2 value) {{ \
                 if (any(lessThan(value, vec2(0.0)))) discard; }}\nvoid clip(vec3 value) {{ if \
                 (any(lessThan(value, vec3(0.0)))) discard; }}\nvoid clip(vec4 value) {{ if \
                 (any(lessThan(value, vec4(0.0)))) discard; }}"
            )
            .map_err(SourceEmitter::write_error)?;
        }

        if self.perform_lighting || self.clip {
            writeln!(output).map_err(SourceEmitter::write_error)?;
        }
        Ok(())
    }
}

/// Parsed function declaration information needed by collision rewrites.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FunctionEntry<'src> {
    /// Function name from the parsed declaration.
    pub name: &'src str,
    /// Span covering only the function name token.
    pub name_span: SourceSpan,
}
