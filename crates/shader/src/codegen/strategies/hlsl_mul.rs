//! HLSL `mul` call codegen.

use linkme::distributed_slice;

use super::{
    CodegenStage, CodegenStrategy, Emitable, GENERAL_POLICIES, HLSL_MUL, LEGACY_BUILTINS,
    StrategyContext,
};
use crate::{
    ShaderResult, SourceSpan,
    codegen::{ExpressionReplacement, Fixup},
    syntax::{FunctionCall, FunctionCalls},
    tokenizer::{TokenCursor, TypedTokenFacts},
};

/// Rewrites two-argument HLSL `mul(a, b)` calls to GLSL multiplication order.
struct HlslMulStrategy;

#[distributed_slice(GENERAL_POLICIES)]
static HLSL_MUL_POLICY: CodegenStrategy = CodegenStrategy {
    name: HLSL_MUL,
    stage: CodegenStage::CompatibilityExpansion,
    after: &[LEGACY_BUILTINS],
    emitter: &HlslMulStrategy,
};

impl Emitable for HlslMulStrategy {
    fn emit(&self, context: &mut StrategyContext<'_, '_, '_>) -> ShaderResult<()> {
        let tokens = context.context().module.token_stream().cursor();
        let facts = context.context().module.token_facts();
        let mut fixups = Vec::new();
        for call in context.context().module.function_calls() {
            if let Some(mul_call) = HlslMulCall::classify_call(call) {
                fixups.push(mul_call.fixup(tokens, facts));
            }
        }
        for fixup in fixups {
            context.context().fixups.push(fixup);
        }
        Ok(())
    }
}

/// HLSL-style two-argument matrix/vector multiplication call.
#[derive(Clone, Debug, Eq, PartialEq)]
struct HlslMulCall {
    /// Original syntactic function call.
    call: FunctionCall,
    /// Source span for the first call argument.
    first: SourceSpan,
    /// Source span for the second call argument.
    second: SourceSpan,
}

impl HlslMulCall {
    /// Classifies a call as a two-argument HLSL `mul(a, b)` expression.
    fn classify_call(call: FunctionCall) -> Option<Self> {
        if call.name() != "mul" || call.argument_count() != 2 {
            return None;
        }

        let first_argument = call.first_argument()?;
        Some(Self {
            call,
            first: first_argument.argument_span(),
            second: first_argument.remaining_argument_span()?,
        })
    }

    /// Returns a source edit that rewrites `mul(a, b)` into `(b * a)`.
    fn fixup(self, tokens: TokenCursor<'_>, facts: &TypedTokenFacts) -> Fixup {
        Fixup::replace(self.call.span(), self.replacement(tokens, facts))
    }

    /// Returns the full expression replacement used when this call is copied.
    fn replacement(
        self,
        tokens: TokenCursor<'_>,
        facts: &TypedTokenFacts,
    ) -> ExpressionReplacement {
        let first = Self::rewrite_span(tokens, facts, self.first);
        let second = Self::rewrite_span(tokens, facts, self.second);

        ExpressionReplacement::new()
            .with_text("((")
            .with_replacement(second)
            .with_text(") * (")
            .with_replacement(first)
            .with_text("))")
    }

    /// Copies the source span while recursively rewriting nested `mul` calls.
    fn rewrite_span(
        tokens: TokenCursor<'_>,
        facts: &TypedTokenFacts,
        span: SourceSpan,
    ) -> ExpressionReplacement {
        let mut replacement = ExpressionReplacement::new();
        let mut copied = span.start();
        let range = tokens.contained_range(span);
        if range.is_empty() {
            return replacement.with_source(span);
        }
        for call in FunctionCalls::new(tokens, facts.calls()).in_range(range) {
            if call.name() != "mul" || call.span().start() < copied {
                continue;
            }
            let Some(mul_call) = HlslMulCall::classify_call(call.clone()) else {
                continue;
            };

            if let Ok(span) = SourceSpan::new(copied, call.span().start()) {
                replacement = replacement.with_source(span);
            }
            replacement = replacement.with_replacement(mul_call.replacement(tokens, facts));
            copied = call.span().end();
        }
        if let Ok(span) = SourceSpan::new(copied, span.end()) {
            replacement = replacement.with_source(span);
        }
        replacement
    }
}
