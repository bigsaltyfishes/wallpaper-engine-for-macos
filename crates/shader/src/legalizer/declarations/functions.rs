//! Compatibility helper requests and function declaration facts.

use std::fmt::Write as _;

use super::super::emission::SourceEmitter;
use crate::{
    ShaderResult, SourceSpan,
    lexer::TokenKind,
    syntax::{FunctionDecl, ShaderModule},
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
/// Compatibility helper functions requested during legalization.
pub(super) struct CompatibilityFunctionRequests {
    /// Whether generated `clip` overloads are needed.
    clip: bool,
    /// Whether generated `PerformLighting_V1` overloads are needed.
    perform_lighting: bool,
}

impl CompatibilityFunctionRequests {
    /// Requests generated `clip` overloads.
    pub(super) fn require_clip(&mut self) {
        self.clip = true;
    }

    /// Requests generated `PerformLighting_V1` overloads.
    pub(super) fn require_perform_lighting(&mut self) {
        self.perform_lighting = true;
    }

    /// Emits requested compatibility helper functions.
    pub(super) fn emit(self, output: &mut String) -> ShaderResult<()> {
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Parsed function declaration information needed by collision rewrites.
pub struct FunctionEntry<'src> {
    /// Function name from the parsed declaration.
    pub(super) name: &'src str,
    /// Span covering only the function name token.
    pub(crate) name_span: SourceSpan,
}

/// Parsed function plus owning module used to locate source spans.
pub(super) struct FunctionSource<'module, 'src> {
    /// Parsed shader module containing the function.
    pub(super) module: &'module ShaderModule<'src>,
    /// Function declaration whose name span is extracted.
    pub(super) function: &'module FunctionDecl<'src>,
}

impl<'src> From<FunctionSource<'_, 'src>> for FunctionEntry<'src> {
    fn from(source: FunctionSource<'_, 'src>) -> Self {
        let module = source.module;
        let function = source.function;
        let signature = function.signature_span();
        let name_span = module
            .tokens()
            .iter()
            .find(|token| {
                token.span.start() >= signature.start()
                    && token.span.end() <= signature.end()
                    && matches!(token.kind, TokenKind::Identifier(text) if text == function.name())
            })
            .map_or(function.signature_span(), |token| token.span);
        Self {
            name: function.name(),
            name_span,
        }
    }
}
