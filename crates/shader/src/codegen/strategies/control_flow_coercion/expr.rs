use super::symbols::{NumericScalarType, SymbolFacts};
use crate::{
    SourceSpan,
    tokenizer::{LogicalOperator, OperatorType::Logical, TokenCursor, TypedToken, TypedTokenFacts},
};

/// Boolean-valued expression marker.
pub(super) struct BoolExpression;

/// Input used to classify a token range as boolean-valued.
#[derive(Clone, Copy)]
pub(super) struct BoolExpressionInput<'statement, 'src> {
    /// First expression token.
    pub start: usize,
    /// Last expression token.
    pub end: usize,
    /// Known symbol facts.
    pub facts: &'statement SymbolFacts<'src>,
    /// Cached tokenizer facts for call lookups.
    pub token_facts: &'statement TypedTokenFacts,
}

impl BoolExpression {
    /// Classifies a token range as boolean-valued.
    pub(super) fn classify(
        input: BoolExpressionInput<'_, '_>,
        tokens: TokenCursor<'_>,
    ) -> Result<Self, ()> {
        if input.start > input.end {
            return Err(());
        }
        if input.start == input.end && input.facts.bool_identifier(tokens, input.start) {
            return Ok(Self);
        }
        if matches!(
            tokens[input.start].kind(),
            TypedToken::Operator(Logical(LogicalOperator::Not))
        ) {
            return Ok(Self);
        }
        if tokens[input.start].kind().is_left_paren()
            && tokens[input.end].kind().is_right_paren()
            && BoolExpression::classify(
                BoolExpressionInput {
                    start: input.start + 1,
                    end: input.end.saturating_sub(1),
                    facts: input.facts,
                    token_facts: input.token_facts,
                },
                tokens,
            )
            .is_ok()
        {
            return Ok(Self);
        }

        let mut paren_depth = 0usize;
        let mut bracket_depth = 0usize;
        for token in &tokens[input.start..input.end + 1] {
            match token.kind() {
                kind if kind.is_left_paren() => paren_depth += 1,
                kind if kind.is_right_paren() => paren_depth = paren_depth.saturating_sub(1),
                kind if kind.is_left_square() => bracket_depth += 1,
                kind if kind.is_right_square() => {
                    bracket_depth = bracket_depth.saturating_sub(1);
                }
                TypedToken::Operator(operator)
                    if paren_depth == 0
                        && bracket_depth == 0
                        && (operator.is_comparison() || operator.is_logical_not()) =>
                {
                    return Ok(Self);
                }
                TypedToken::Operator(Logical(LogicalOperator::And | LogicalOperator::Or))
                    if paren_depth == 0 && bracket_depth == 0 =>
                {
                    return Ok(Self);
                }
                _ => {}
            }
        }

        Err(())
    }
}

/// Token-backed numeric condition expression.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct NumericCondition {
    /// Source span covering the expression.
    pub span: SourceSpan,
    /// Scalar numeric type of the expression.
    pub ty: NumericScalarType,
}

impl NumericCondition {
    /// Returns the zero literal that matches the expression scalar type.
    pub(super) const fn zero_literal(self) -> &'static str {
        match self.ty {
            NumericScalarType::Float => "0.0",
            NumericScalarType::Int => "0",
            NumericScalarType::Uint => "0u",
        }
    }
}

/// Input used to classify condition token ranges that GLSL requires as bools.
#[derive(Clone, Copy)]
pub(super) struct NumericConditionInput<'statement, 'src> {
    /// First expression token.
    pub start: usize,
    /// Last expression token.
    pub end: usize,
    /// Known symbol facts.
    pub facts: &'statement SymbolFacts<'src>,
    /// Cached tokenizer facts for call lookups.
    pub token_facts: &'statement TypedTokenFacts,
}

impl NumericCondition {
    /// Extracts a numeric condition expression that strict GLSL requires as
    /// bool.
    pub(super) fn extract(
        input: NumericConditionInput<'_, '_>,
        tokens: TokenCursor<'_>,
    ) -> Result<Self, ()> {
        if BoolExpression::classify(
            BoolExpressionInput {
                start: input.start,
                end: input.end,
                facts: input.facts,
                token_facts: input.token_facts,
            },
            tokens,
        )
        .is_ok()
        {
            return Err(());
        }
        let Some(ty) =
            input
                .facts
                .numeric_expression_type(tokens, input.token_facts, input.start, input.end)
        else {
            return Err(());
        };
        SourceSpan::new(
            tokens[input.start].span().start(),
            tokens[input.end].span().end(),
        )
        .map(|span| Self { span, ty })
        .map_err(|_error| ())
    }
}
