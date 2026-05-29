//! Legacy builtin call codegen.

use linkme::distributed_slice;

use super::{
    CodegenStage, CodegenStrategy, Emitable, GENERAL_POLICIES, LEGACY_BUILTINS, StrategyContext,
    TEXTURE_SAMPLING, TextureSamplingCall, TextureSamplingFunction,
};
use crate::{
    ShaderResult, SourceSpan,
    codegen::{ExpressionReplacement, Fixup},
    syntax::{FunctionCall, FunctionCalls, SyntaxItem},
    tokenizer::TokenCursor,
};

/// Rewrites legacy HLSL and Wallpaper Engine builtin calls.
struct LegacyBuiltinsStrategy;

#[distributed_slice(GENERAL_POLICIES)]
static LEGACY_BUILTINS_POLICY: CodegenStrategy = CodegenStrategy {
    name: LEGACY_BUILTINS,
    stage: CodegenStage::CompatibilityExpansion,
    after: &[TEXTURE_SAMPLING],
    emitter: &LegacyBuiltinsStrategy,
};

impl Emitable for LegacyBuiltinsStrategy {
    fn emit(&self, context: &mut StrategyContext<'_, '_, '_>) -> ShaderResult<()> {
        let tokens = context.context().module.token_stream().cursor();
        for call in context.context().module.function_calls() {
            Self::emit_call(context, tokens, call)?;
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
                Self::emit_call(context, cursor, call)?;
            }
        }
        Ok(())
    }
}

impl LegacyBuiltinsStrategy {
    /// Emits legacy builtin fixups for one syntactic call.
    fn emit_call(
        context: &mut StrategyContext<'_, '_, '_>,
        tokens: TokenCursor<'_>,
        call: FunctionCall,
    ) -> ShaderResult<()> {
        if TextureSamplingCall::classify_call(tokens, &call, &context.context().declarations)
            .is_some()
        {
            return Ok(());
        }

        match call.name() {
            "lerp" => LegacyBuiltinCall::Rename { call, name: "mix" }.emit(context, tokens)?,
            "frac" => LegacyBuiltinCall::Rename {
                call,
                name: "fract",
            }
            .emit(context, tokens)?,
            "atan2" => LegacyBuiltinCall::Rename { call, name: "atan" }.emit(context, tokens)?,
            "ddx" => LegacyBuiltinCall::Rename { call, name: "dFdx" }.emit(context, tokens)?,
            "CAST2" => LegacyBuiltinCall::Rename { call, name: "vec2" }.emit(context, tokens)?,
            "CAST3" => LegacyBuiltinCall::Rename { call, name: "vec3" }.emit(context, tokens)?,
            "CAST4" => LegacyBuiltinCall::Rename { call, name: "vec4" }.emit(context, tokens)?,
            "CAST3X3" => LegacyBuiltinCall::Rename { call, name: "mat3" }.emit(context, tokens)?,
            "tex2D" | "texSample2D" | "texture2D" | "texSample2DLod" | "textureLod" => {
                let name = TextureSamplingFunction::classify_name(call.name()).map_or(
                    "texture",
                    |function| match function {
                        TextureSamplingFunction::ImplicitLod => "texture",
                        TextureSamplingFunction::ExplicitLod => "textureLod",
                    },
                );
                LegacyBuiltinCall::Rename { call, name }.emit(context, tokens)?;
            }
            "saturate" => LegacyBuiltinCall::Saturate { call }.emit(context, tokens)?,
            "log10" => LegacyBuiltinCall::Log10 { call }.emit(context, tokens)?,
            "fmod" => LegacyBuiltinCall::Fmod { call }.emit(context, tokens)?,
            "ddy" => LegacyBuiltinCall::Ddy { call }.emit(context, tokens)?,
            _ => {}
        }
        Ok(())
    }
}

/// Legacy builtin call classification.
#[derive(Clone, Debug, Eq, PartialEq)]
enum LegacyBuiltinCall {
    /// Direct function name replacement.
    Rename {
        /// Original call expression.
        call: FunctionCall,
        /// Replacement function name.
        name: &'static str,
    },
    /// `saturate(x)` to `clamp(x, 0.0, 1.0)`.
    Saturate {
        /// Original call expression.
        call: FunctionCall,
    },
    /// `log10(x)` to `(log2(x) * C)`.
    Log10 {
        /// Original call expression.
        call: FunctionCall,
    },
    /// `fmod(x, y)` to the C++ compatibility expression.
    Fmod {
        /// Original call expression.
        call: FunctionCall,
    },
    /// `ddy(x)` to `dFdy(-(x))`.
    Ddy {
        /// Original call expression.
        call: FunctionCall,
    },
}

impl LegacyBuiltinCall {
    /// Emits fixups for this builtin call.
    fn emit(
        self,
        context: &mut StrategyContext<'_, '_, '_>,
        tokens: TokenCursor<'_>,
    ) -> ShaderResult<()> {
        match self {
            Self::Rename { call, name } => {
                context
                    .context()
                    .fixups
                    .push(Fixup::replace(call.name_span(), name));
            }
            Self::Saturate { call } => {
                context
                    .context()
                    .fixups
                    .push(Fixup::replace(call.name_span(), "clamp"));
                context.context().fixups.push(Fixup::insert(
                    SourceSpan::new(
                        tokens[call.close_index].span().start(),
                        tokens[call.close_index].span().start(),
                    )?,
                    ", 0.0, 1.0".to_owned(),
                ));
            }
            Self::Log10 { call } => {
                let Some(first_argument) = call.first_argument() else {
                    return Ok(());
                };
                let argument = first_argument.argument_span();
                let replacement = ExpressionReplacement::new()
                    .with_text("(log2(")
                    .with_source(argument)
                    .with_text(") * 0.301029995663981)");
                context
                    .context()
                    .fixups
                    .push(Fixup::replace(call.span(), replacement));
            }
            Self::Fmod { call } => {
                let Some(first_argument) = call.first_argument() else {
                    return Ok(());
                };
                let left = first_argument.argument_span();
                let Some(right) = first_argument.remaining_argument_span() else {
                    return Ok(());
                };
                let replacement = ExpressionReplacement::new()
                    .with_text("((")
                    .with_source(left)
                    .with_text(") - (")
                    .with_source(right)
                    .with_text(") * trunc((")
                    .with_source(left)
                    .with_text(") / (")
                    .with_source(right)
                    .with_text(")))");
                context
                    .context()
                    .fixups
                    .push(Fixup::replace(call.span(), replacement));
            }
            Self::Ddy { call } => {
                let Some(first_argument) = call.first_argument() else {
                    return Ok(());
                };
                let argument = first_argument.argument_span();
                let replacement = ExpressionReplacement::new()
                    .with_text("dFdy(-(")
                    .with_source(argument)
                    .with_text("))");
                context
                    .context()
                    .fixups
                    .push(Fixup::replace(call.span(), replacement));
            }
        }
        Ok(())
    }
}
