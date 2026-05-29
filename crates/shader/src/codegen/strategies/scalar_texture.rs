//! Scalar texture assignment codegen.

use linkme::distributed_slice;

use super::{
    CodegenStage, CodegenStrategy, Emitable, GENERAL_POLICIES, SCALAR_TEXTURE, StrategyContext,
    TEXTURE_SAMPLING, TYPE_COERCION, TextureSamplingCall,
};
use crate::{
    ShaderResult, SourceSpan,
    codegen::Fixup,
    tokenizer::{AssignmentOperator, OperatorType as ShaderOp, PrimitiveType, TypedToken},
};

/// Selects the first component when a texture sample initializes a scalar.
struct ScalarTextureStrategy;

#[distributed_slice(GENERAL_POLICIES)]
static SCALAR_TEXTURE_POLICY: CodegenStrategy = CodegenStrategy {
    name: SCALAR_TEXTURE,
    stage: CodegenStage::TypeCodegen,
    after: &[TYPE_COERCION, TEXTURE_SAMPLING],
    emitter: &ScalarTextureStrategy,
};

impl Emitable for ScalarTextureStrategy {
    fn emit(&self, context: &mut StrategyContext<'_, '_, '_>) -> ShaderResult<()> {
        let tokens = context.context().module.token_stream().cursor();
        for call in context.context().module.function_calls() {
            if !matches!(
                call.name(),
                "texture" | "texture2D" | "tex2D" | "texSample2D" | "texSample2DLod" | "textureLod"
            ) {
                continue;
            }
            let Some(_texture_call) =
                TextureSamplingCall::classify_call(tokens, &call, &context.context().declarations)
            else {
                continue;
            };
            if call.has_trailing_swizzle() {
                continue;
            }

            let Some(equals) = tokens.previous_non_comment(call.name_index) else {
                continue;
            };
            if !matches!(
                tokens[equals].kind(),
                TypedToken::Operator(ShaderOp::Assignment(AssignmentOperator::Assign,))
            ) {
                continue;
            }
            let Some(name) = tokens.previous_non_comment(equals) else {
                continue;
            };
            let Some(ty) = tokens.previous_non_comment(name) else {
                continue;
            };
            if !matches!(
                tokens[ty].kind(),
                TypedToken::TypeMark(PrimitiveType::Float)
            ) {
                continue;
            }
            let Some(semicolon) = tokens
                .iter()
                .enumerate()
                .skip(call.close_index + 1)
                .find_map(|(index, token)| {
                    matches!(token.kind(), TypedToken::Semicolon).then_some(index)
                })
            else {
                continue;
            };

            context.context().fixups.push(Fixup::insert(
                SourceSpan::new(call.span().start(), call.span().start())
                    .unwrap_or_else(|_| call.name_span()),
                "(".to_owned(),
            ));
            context.context().fixups.push(Fixup::insert(
                SourceSpan::new(
                    tokens[semicolon].span().start(),
                    tokens[semicolon].span().start(),
                )
                .unwrap_or_else(|_| call.span()),
                ").x".to_owned(),
            ));
        }
        Ok(())
    }
}
