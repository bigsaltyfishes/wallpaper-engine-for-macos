//! Texture sampling call legalization.

use linkme::distributed_slice;

use super::{Emitable, GENERAL_POLICIES, PolicyContext};
use crate::{
    ShaderResult, SourceSpan,
    legalizer::{DeclarationPlan, DefineDirectiveTokenExt, Fixup, FunctionCall, FunctionCallIndex},
    lexer::TokenKind,
};

/// Rewrites source `sampler2D` calls to Naga-compatible separated handles.
struct TextureSamplingPolicy;

#[distributed_slice(GENERAL_POLICIES)]
static TEXTURE_SAMPLING_POLICY: &dyn Emitable = &TextureSamplingPolicy;

impl Emitable for TextureSamplingPolicy {
    fn emit(&self, context: &mut PolicyContext<'_, '_, '_>) -> ShaderResult<()> {
        let calls = FunctionCallIndex::new(context.context().module.tokens());
        for call in calls.iter() {
            Self::emit_call(context, call)?;
        }

        for directive in context.context().module.tokens() {
            let Some(tokens) = directive.define_body_tokens()? else {
                continue;
            };
            let calls = FunctionCallIndex::new(&tokens);
            for call in calls.iter() {
                Self::emit_call(context, call)?;
            }
        }
        Ok(())
    }
}

impl TextureSamplingPolicy {
    /// Emits texture-sampling fixups for one syntactic call.
    fn emit_call(
        context: &mut PolicyContext<'_, '_, '_>,
        call: FunctionCall<'_, '_>,
    ) -> ShaderResult<()> {
        if let Some(texture_call) =
            TextureSamplingCall::from_call(call, &context.context().declarations)
        {
            context.context().fixups.push(Fixup::replace(
                texture_call.name_span(),
                texture_call.glsl_name(),
            ));
            context.context().fixups.push(Fixup::insert(
                texture_call.texture_start()?,
                "sampler2D(".to_owned(),
            ));
            context.context().fixups.push(Fixup::insert(
                texture_call.texture_end()?,
                format!(", {})", texture_call.sampler_name),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Texture sampling call that requires a separated Naga sampler wrapper.
pub(super) struct TextureSamplingCall<'module, 'src> {
    /// Original syntactic function call.
    call: FunctionCall<'module, 'src>,
    /// Sampling function family used by this call.
    function: TextureSamplingFunction,
    /// Source span for the first texture argument.
    texture: SourceSpan,
    /// Generated sampler paired to the source texture declaration.
    sampler_name: String,
}

impl<'module, 'src> TextureSamplingCall<'module, 'src> {
    /// Classifies a call as a sampling call against a source `sampler2D`
    /// declaration.
    pub(super) fn from_call(
        call: FunctionCall<'module, 'src>,
        declarations: &DeclarationPlan<'src>,
    ) -> Option<Self> {
        let function = TextureSamplingFunction::from_name(call.name())?;

        let first_argument = call.first_argument()?;
        let TokenKind::Identifier(name) = call.tokens[first_argument.start()].kind else {
            return None;
        };
        let sampler_name = declarations.texture_sampler_name(name)?;

        Some(Self {
            call,
            function,
            texture: first_argument.argument_span(call.tokens)?,
            sampler_name,
        })
    }

    /// Returns the source span for the call name.
    pub(super) const fn name_span(&self) -> SourceSpan {
        self.call.name_span()
    }

    /// Returns the GLSL sampling function emitted for this call.
    pub(super) const fn glsl_name(&self) -> &'static str {
        match self.function {
            TextureSamplingFunction::ImplicitLod => "texture",
            TextureSamplingFunction::ExplicitLod => "textureLod",
        }
    }

    /// Returns the insertion point before the texture argument.
    pub(super) fn texture_start(&self) -> ShaderResult<SourceSpan> {
        SourceSpan::new(self.texture.start(), self.texture.start())
    }

    /// Returns the insertion point after the texture argument.
    pub(super) fn texture_end(&self) -> ShaderResult<SourceSpan> {
        SourceSpan::new(self.texture.end(), self.texture.end())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Texture sampling function family used by WE shaders.
pub(super) enum TextureSamplingFunction {
    /// `texture(...)` style implicit LOD sampling.
    ImplicitLod,
    /// `textureLod(...)` style explicit LOD sampling.
    ExplicitLod,
}

impl TextureSamplingFunction {
    /// Classifies a function name as a supported texture sampling function.
    pub(super) const fn from_name(name: &str) -> Option<Self> {
        match name.as_bytes() {
            b"texture" | b"texture2D" | b"tex2D" | b"texSample2D" => Some(Self::ImplicitLod),
            b"textureLod" | b"texSample2DLod" => Some(Self::ExplicitLod),
            _ => None,
        }
    }
}
