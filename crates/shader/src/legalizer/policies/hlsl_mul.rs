//! HLSL `mul` call legalization.

use linkme::distributed_slice;

use super::{Emitable, GENERAL_POLICIES, PolicyContext};
use crate::{
    ShaderResult, SourceSpan,
    legalizer::{
        ExpressionReplacement, Fixup, FunctionCall, FunctionCallIndex, fixups::SourceSpanExt,
    },
    lexer::Token,
};

/// Rewrites two-argument HLSL `mul(a, b)` calls to GLSL multiplication order.
struct HlslMulPolicy;

#[distributed_slice(GENERAL_POLICIES)]
static HLSL_MUL_POLICY: &dyn Emitable = &HlslMulPolicy;

impl Emitable for HlslMulPolicy {
    fn emit(&self, context: &mut PolicyContext<'_, '_, '_>) -> ShaderResult<()> {
        let calls = FunctionCallIndex::new(context.context().module.tokens());
        for call in calls.iter() {
            if let Some(mul_call) = HlslMulCall::from_call(call) {
                context.context().fixups.push(mul_call.fixup());
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// HLSL-style two-argument matrix/vector multiplication call.
struct HlslMulCall<'module, 'src> {
    /// Original syntactic function call.
    call: FunctionCall<'module, 'src>,
    /// Source span for the first call argument.
    first: SourceSpan,
    /// Source span for the second call argument.
    second: SourceSpan,
}

impl<'module, 'src> HlslMulCall<'module, 'src> {
    /// Classifies a call as a two-argument HLSL `mul(a, b)` expression.
    fn from_call(call: FunctionCall<'module, 'src>) -> Option<Self> {
        if call.name() != "mul" || call.argument_count() != 2 {
            return None;
        }

        let first_argument = call.first_argument()?;
        Some(Self {
            call,
            first: first_argument.argument_span(call.tokens)?,
            second: first_argument.remaining_argument_span(call.tokens)?,
        })
    }

    /// Returns a source edit that rewrites `mul(a, b)` into `(b * a)`.
    fn fixup(self) -> Fixup {
        Fixup::replace(self.call.span(), self.replacement())
    }

    /// Returns the full expression replacement used when this call is copied.
    fn replacement(self) -> ExpressionReplacement {
        let first = HlslMulExpression {
            tokens: self.call.tokens,
            span: self.first,
        }
        .rewrite();
        let second = HlslMulExpression {
            tokens: self.call.tokens,
            span: self.second,
        }
        .rewrite();

        ExpressionReplacement::new()
            .with_text("((")
            .with_replacement(second)
            .with_text(") * (")
            .with_replacement(first)
            .with_text("))")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Source expression renderer that recursively rewrites copied `mul` calls.
struct HlslMulExpression<'module, 'src> {
    /// Tokens searched for nested calls.
    tokens: &'module [Token<'src>],
    /// Source span being copied.
    span: SourceSpan,
}

impl HlslMulExpression<'_, '_> {
    /// Copies the source span while recursively rewriting nested `mul` calls.
    fn rewrite(self) -> ExpressionReplacement {
        let mut replacement = ExpressionReplacement::new();
        let mut copied = self.span.start();
        for call in FunctionCallIndex::new(self.tokens).iter() {
            if call.name() != "mul" || !self.span.contains(call.span()) {
                continue;
            }
            if call.span().start() < copied {
                continue;
            }
            let Some(mul_call) = HlslMulCall::from_call(call) else {
                continue;
            };

            if let Ok(span) = SourceSpan::new(copied, call.span().start()) {
                replacement = replacement.with_source(span);
            }
            replacement = replacement.with_replacement(mul_call.replacement());
            copied = call.span().end();
        }
        if let Ok(span) = SourceSpan::new(copied, self.span.end()) {
            replacement = replacement.with_source(span);
        }
        replacement
    }
}
