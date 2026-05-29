//! Scalar texture assignment legalization.

use linkme::distributed_slice;

use super::{Emitable, GENERAL_POLICIES, PolicyContext, TextureSamplingCall};
use crate::{
    ShaderResult, SourceSpan,
    legalizer::{Fixup, FunctionCallIndex, TokenSearch},
    lexer::TokenKind,
};

/// Selects the first component when a texture sample initializes a scalar.
struct ScalarTexturePolicy;

#[distributed_slice(GENERAL_POLICIES)]
static SCALAR_TEXTURE_POLICY: &dyn Emitable = &ScalarTexturePolicy;

impl Emitable for ScalarTexturePolicy {
    fn emit(&self, context: &mut PolicyContext<'_, '_, '_>) -> ShaderResult<()> {
        let calls = FunctionCallIndex::new(context.context().module.tokens());
        for call in calls.iter() {
            if !matches!(
                call.name(),
                "texture" | "texture2D" | "tex2D" | "texSample2D" | "texSample2DLod" | "textureLod"
            ) {
                continue;
            }
            let Some(_texture_call) =
                TextureSamplingCall::from_call(call, &context.context().declarations)
            else {
                continue;
            };
            if call.has_trailing_swizzle() {
                continue;
            }

            let tokens = context.context().module.tokens();
            let Some(equals) = TokenSearch::new(tokens).previous_non_comment(call.name_index)
            else {
                continue;
            };
            if !matches!(tokens[equals].kind, TokenKind::Punctuation('=')) {
                continue;
            }
            let Some(name) = TokenSearch::new(tokens).previous_non_comment(equals) else {
                continue;
            };
            let Some(ty) = TokenSearch::new(tokens).previous_non_comment(name) else {
                continue;
            };
            if !matches!(tokens[ty].kind, TokenKind::Identifier("float")) {
                continue;
            }
            let Some(semicolon) = tokens
                .iter()
                .enumerate()
                .skip(call.close_index + 1)
                .find_map(|(index, token)| {
                    matches!(token.kind, TokenKind::Semicolon).then_some(index)
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
                    tokens[semicolon].span.start(),
                    tokens[semicolon].span.start(),
                )
                .unwrap_or_else(|_| call.span()),
                ").x".to_owned(),
            ));
        }
        Ok(())
    }
}
