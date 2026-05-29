//! Texture sampling call codegen.

use linkme::distributed_slice;

use super::{
    CodegenStage, CodegenStrategy, Emitable, GENERAL_POLICIES, StrategyContext, TEXTURE_SAMPLING,
};
use crate::{
    ShaderResult, SourceSpan,
    codegen::{DeclarationPlan, Fixup},
    syntax::{FunctionCall, FunctionCalls, SyntaxItem},
    tokenizer::{TokenCursor, TypedToken},
};

/// Rewrites source `sampler2D` calls to Naga-compatible separated handles.
struct TextureSamplingStrategy;

#[distributed_slice(GENERAL_POLICIES)]
static TEXTURE_SAMPLING_POLICY: CodegenStrategy = CodegenStrategy {
    name: TEXTURE_SAMPLING,
    stage: CodegenStage::CompatibilityExpansion,
    after: &[],
    emitter: &TextureSamplingStrategy,
};

impl Emitable for TextureSamplingStrategy {
    fn emit(&self, context: &mut StrategyContext<'_, '_, '_>) -> ShaderResult<()> {
        let tokens = context.context().module.token_stream().cursor();
        for call in context.context().module.function_calls() {
            Self::emit_call(context, tokens, &call)?;
        }

        for directive in context.context().module.items().iter().filter_map(|item| {
            let SyntaxItem::Directive(directive) = item else {
                return None;
            };
            Some(directive)
        }) {
            let Some(tokens) = directive.define_body_tokens_in(context.context().module)? else {
                continue;
            };
            let cursor = tokens.cursor();
            let facts = tokens.facts();
            for call in FunctionCalls::new(cursor, facts.calls()) {
                Self::emit_call(context, cursor, &call)?;
            }
        }
        Ok(())
    }
}

impl TextureSamplingStrategy {
    /// Emits texture-sampling fixups for one syntactic call.
    fn emit_call(
        context: &mut StrategyContext<'_, '_, '_>,
        tokens: TokenCursor<'_>,
        call: &FunctionCall,
    ) -> ShaderResult<()> {
        if let Some(texture_call) =
            TextureSamplingCall::classify_call(tokens, call, &context.context().declarations)
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

/// Texture sampling call that requires a separated Naga sampler wrapper.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct TextureSamplingCall {
    /// Original syntactic function call.
    call: FunctionCall,
    /// Sampling function family used by this call.
    function: TextureSamplingFunction,
    /// Source span for the first texture argument.
    texture: SourceSpan,
    /// Generated sampler paired to the source texture declaration.
    sampler_name: String,
}

impl TextureSamplingCall {
    /// Classifies a call as a sampling call against a source `sampler2D`
    /// declaration.
    pub(super) fn classify_call(
        tokens: TokenCursor<'_>,
        call: &FunctionCall,
        declarations: &DeclarationPlan<'_>,
    ) -> Option<Self> {
        let function = TextureSamplingFunction::classify_name(call.name())?;

        let first_argument = call.first_argument()?;
        let TypedToken::Identifier(name) = tokens[first_argument.start()].kind() else {
            return None;
        };
        let sampler_name = declarations.texture_sampler_name(name)?;

        Some(Self {
            call: call.clone(),
            function,
            texture: first_argument.argument_span(),
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

/// Texture sampling function family used by WE shaders.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum TextureSamplingFunction {
    /// `texture(...)` style implicit LOD sampling.
    ImplicitLod,
    /// `textureLod(...)` style explicit LOD sampling.
    ExplicitLod,
}

impl TextureSamplingFunction {
    /// Classifies a function name as a supported texture sampling function.
    pub(super) const fn classify_name(name: &str) -> Option<Self> {
        match name.as_bytes() {
            b"texture" | b"texture2D" | b"tex2D" | b"texSample2D" => Some(Self::ImplicitLod),
            b"textureLod" | b"texSample2DLod" => Some(Self::ExplicitLod),
            _ => None,
        }
    }
}
