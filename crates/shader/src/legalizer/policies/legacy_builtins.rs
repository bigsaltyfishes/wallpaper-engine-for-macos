//! Legacy builtin call legalization.

use linkme::distributed_slice;

use super::{
    Emitable, GENERAL_POLICIES, PolicyContext, TextureSamplingCall, TextureSamplingFunction,
};
use crate::{
    ShaderResult, SourceSpan,
    legalizer::{
        DefineDirectiveTokenExt, ExpressionReplacement, Fixup, FunctionCall, FunctionCallIndex,
    },
};

/// Rewrites legacy HLSL and Wallpaper Engine builtin calls.
struct LegacyBuiltinsPolicy;

#[distributed_slice(GENERAL_POLICIES)]
static LEGACY_BUILTINS_POLICY: &dyn Emitable = &LegacyBuiltinsPolicy;

impl Emitable for LegacyBuiltinsPolicy {
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

impl LegacyBuiltinsPolicy {
    /// Emits legacy builtin fixups for one syntactic call.
    fn emit_call(
        context: &mut PolicyContext<'_, '_, '_>,
        call: FunctionCall<'_, '_>,
    ) -> ShaderResult<()> {
        if TextureSamplingCall::from_call(call, &context.context().declarations).is_some() {
            return Ok(());
        }

        match call.name() {
            "lerp" => LegacyBuiltinCall::Rename { call, name: "mix" }.emit(context)?,
            "frac" => LegacyBuiltinCall::Rename {
                call,
                name: "fract",
            }
            .emit(context)?,
            "atan2" => LegacyBuiltinCall::Rename { call, name: "atan" }.emit(context)?,
            "ddx" => LegacyBuiltinCall::Rename { call, name: "dFdx" }.emit(context)?,
            "CAST2" => LegacyBuiltinCall::Rename { call, name: "vec2" }.emit(context)?,
            "CAST3" => LegacyBuiltinCall::Rename { call, name: "vec3" }.emit(context)?,
            "CAST4" => LegacyBuiltinCall::Rename { call, name: "vec4" }.emit(context)?,
            "CAST3X3" => LegacyBuiltinCall::Rename { call, name: "mat3" }.emit(context)?,
            "tex2D" | "texSample2D" | "texture2D" | "texSample2DLod" | "textureLod" => {
                let name =
                    TextureSamplingFunction::from_name(call.name()).map_or("texture", |function| {
                        match function {
                            TextureSamplingFunction::ImplicitLod => "texture",
                            TextureSamplingFunction::ExplicitLod => "textureLod",
                        }
                    });
                LegacyBuiltinCall::Rename { call, name }.emit(context)?;
            }
            "saturate" => LegacyBuiltinCall::Saturate { call }.emit(context)?,
            "log10" => LegacyBuiltinCall::Log10 { call }.emit(context)?,
            "fmod" => LegacyBuiltinCall::Fmod { call }.emit(context)?,
            "ddy" => LegacyBuiltinCall::Ddy { call }.emit(context)?,
            _ => {}
        }
        Ok(())
    }
}

/// Legacy builtin call classification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LegacyBuiltinCall<'module, 'src> {
    /// Direct function name replacement.
    Rename {
        /// Original call expression.
        call: FunctionCall<'module, 'src>,
        /// Replacement function name.
        name: &'static str,
    },
    /// `saturate(x)` to `clamp(x, 0.0, 1.0)`.
    Saturate {
        /// Original call expression.
        call: FunctionCall<'module, 'src>,
    },
    /// `log10(x)` to `(log2(x) * C)`.
    Log10 {
        /// Original call expression.
        call: FunctionCall<'module, 'src>,
    },
    /// `fmod(x, y)` to the C++ compatibility expression.
    Fmod {
        /// Original call expression.
        call: FunctionCall<'module, 'src>,
    },
    /// `ddy(x)` to `dFdy(-(x))`.
    Ddy {
        /// Original call expression.
        call: FunctionCall<'module, 'src>,
    },
}

impl LegacyBuiltinCall<'_, '_> {
    /// Emits fixups for this builtin call.
    fn emit(self, context: &mut PolicyContext<'_, '_, '_>) -> ShaderResult<()> {
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
                        call.tokens[call.close_index].span.start(),
                        call.tokens[call.close_index].span.start(),
                    )?,
                    ", 0.0, 1.0".to_owned(),
                ));
            }
            Self::Log10 { call } => {
                let Some(argument) = call
                    .first_argument()
                    .and_then(|arg| arg.argument_span(call.tokens))
                else {
                    return Ok(());
                };
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
                let Some(left) = first_argument.argument_span(call.tokens) else {
                    return Ok(());
                };
                let Some(right) = first_argument.remaining_argument_span(call.tokens) else {
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
                let Some(argument) = call
                    .first_argument()
                    .and_then(|arg| arg.argument_span(call.tokens))
                else {
                    return Ok(());
                };
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
